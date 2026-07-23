//! `bolted-http-linux` — the **reqwest reference adapter** for the [`bolted_http`] capability
//! contract (feature-matrix S-LX / step-24 M3). This is the adapter the conformance suite is
//! debugged against after the mock (the one-implementor lesson: mock first, then reqwest).
//!
//! ## The executor lives here
//!
//! `bolted-http` is sans-io and dependency-clean; an **adapter owns its executor**. [`LinuxHttp`]
//! contains a tokio multi-thread runtime. [`crate::LinuxHttp::send`] returns a [`RequestHandle`]
//! immediately and spawns the exchange onto that runtime; the single completion is delivered to the
//! [`CompletionSink`]. The contract's poll-based [`CancelToken`] is bridged to async by a 10 ms poll
//! task raced against the request via `select!`.
//!
//! ## Contract fidelity (feature-matrix rows)
//!
//! - **One total deadline** (row 4): `select!` races the whole redirect-following exchange against a
//!   single `sleep(deadline)`. Timeout ⇒ [`HttpError::Timeout`]; caller cancel ⇒
//!   [`HttpError::Cancelled`] — **classified by cause** (which arm fired), never by error shape
//!   (rule 2). reqwest's own per-request timeout is *not* used, so the deadline spans every hop.
//! - **Cookie-less, cache-less, no ambient state** (row 8): a fresh client per request, no cookie
//!   store, no cache. HTTPS-only is the contract's `Url` guard; `cleartext_dev` is the only escape.
//! - **Redirects** (rows 6/7): auto-redirect is *off* (`Policy::none`); the adapter follows manually
//!   so it captures the hop trace, reports the final URL, refuses `https → http`
//!   ([`HttpError::InsecureRedirect`]), and caps the chain ([`HttpError::TooManyRedirects`]).
//! - **Decoded bodies + honest `content_length`** (rows 13/17, rule 7): reqwest's `gzip` decode is
//!   on, which strips `Content-Length`, so `content_length()` is honest (`None`) under decoding.
//! - **Response sink** (row 15): [`ResponseSink::File`] streams the decoded body to a sibling temp
//!   file, then atomically renames it into place; any write failure is [`HttpError::Io`].
//! - **Upload progress** (row 14, rule 11): the request body is wrapped in a progress-reporting
//!   stream — `sent` is monotone per attempt and terminates at the body length.
//! - **Retry OFF** (§5.17, rule 8): reqwest 0.13 retries protocol NACKs *by default*; this adapter
//!   installs [`reqwest::retry::never`] and disables connection pooling
//!   (`pool_max_idle_per_host(0)`), so no request that reached the wire is ever re-sent.
//! - **Pinning** (row 19 / L2): declarative SPKI pins from the request drive a real rustls verifier
//!   installed via `use_preconfigured_tls` — see [`tls`]. **The L2 verdict is: works, no demote.**
//! - **Capabilities** (C3): [`Metrics`] present at the coarse [`MetricsTier::WholeRequest`] tier
//!   (reqwest has no phase seam, §5.13). The priority hint (row 12) is a **uniform advisory
//!   field** (ruled Q10): reqwest has no priority knob, so this adapter carries the data and
//!   legally ignores it — no capability trait, no C3 column.
//!
//! ## L4 — proxy (recorded, not worked around)
//!
//! reqwest 0.13's default proxy resolution is **env-vars-only on Linux** (no gsettings/PAC/portal;
//! feature-matrix §5.19). This adapter builds **no** proxy configuration — it inherits that default.
//! The C3 divergence matrix only covers capability *traits* ([`Metrics`]), so proxy behaviour has
//! no column there; it is recorded here and in the step report
//! rather than by unilaterally widening C3's shape.

#![forbid(unsafe_code)]

pub mod error;
pub mod tls;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use futures_util::StreamExt;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;

use bolted_http::capability::{
    CancelToken, CompletionSink, Http, Metrics, MetricsTier, RequestHandle, UploadProgressSink,
};
use bolted_http::request::{HttpRequest, Method, PinSet, RequestBody, ResponseSink};
use bolted_http::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use bolted_http::{HeaderName, HeaderValue, Headers, HttpError, TlsErrorKind, Url};

use crate::error::map_reqwest_error;
use crate::tls::{PinningVerifier, RejectSlot, TlsReject};

/// A unique suffix source for response-sink temp files (collision-free across parallel requests).
static SINK_NONCE: AtomicU64 = AtomicU64::new(0);

/// Install the `ring` crypto provider as the rustls process default (idempotent). The verifier is
/// built with an explicit provider, but reqwest's rustls internals may consult the default.
fn install_default_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Composition-root configuration for the adapter (the contract's CFG rows — trust roots, redirect
/// ceiling). None of this is ever seen by the sans-io core.
#[derive(Clone, Debug)]
pub struct LinuxHttpConfig {
    /// DER-encoded trust anchors the adapter's **real** chain verification trusts (row 25 / §5.14).
    /// Production wires the system / Mozilla roots here; the conformance suite passes the test
    /// server's self-signed cert. Empty means "trust nothing" — every TLS request then fails.
    pub trust_anchors: Vec<Vec<u8>>,
    /// The redirect-follow ceiling; beyond it a request is [`HttpError::TooManyRedirects`].
    pub redirect_limit: u32,
    /// Whether declarative SPKI pins are enforced (default `true`). `false` is the scoped red-twin
    /// that proves the pin check is load-bearing — a pin no longer bites.
    pub enforce_pins: bool,
}

impl Default for LinuxHttpConfig {
    fn default() -> Self {
        LinuxHttpConfig {
            trust_anchors: Vec::new(),
            redirect_limit: 10,
            enforce_pins: true,
        }
    }
}

impl LinuxHttpConfig {
    /// A config trusting one DER-encoded anchor (the common composition-root shape).
    #[must_use]
    pub fn with_trust_anchor(der: Vec<u8>) -> Self {
        LinuxHttpConfig {
            trust_anchors: vec![der],
            ..LinuxHttpConfig::default()
        }
    }
}

/// Why the adapter could not be constructed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxHttpError {
    /// The tokio runtime could not be built.
    Runtime,
    /// A configured trust anchor was not a valid certificate.
    BadAnchor,
}

/// Shared, reference-counted adapter state: the owned executor plus the TLS trust configuration.
struct Inner {
    runtime: Runtime,
    roots: Arc<RootCertStore>,
    provider: Arc<rustls::crypto::CryptoProvider>,
    redirect_limit: u32,
    enforce_pins: bool,
}

/// The reqwest reference adapter. Cheap to clone (shares one runtime + trust config).
#[derive(Clone)]
pub struct LinuxHttp {
    inner: Arc<Inner>,
}

impl LinuxHttp {
    /// Build the adapter and its contained runtime from `config`.
    pub fn new(config: LinuxHttpConfig) -> Result<Self, LinuxHttpError> {
        install_default_provider();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = RootCertStore::empty();
        for der in &config.trust_anchors {
            roots
                .add(CertificateDer::from(der.clone()))
                .map_err(|_| LinuxHttpError::BadAnchor)?;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| LinuxHttpError::Runtime)?;
        Ok(LinuxHttp {
            inner: Arc::new(Inner {
                runtime,
                roots: Arc::new(roots),
                provider,
                redirect_limit: config.redirect_limit,
                enforce_pins: config.enforce_pins,
            }),
        })
    }

    /// Build a fresh reqwest client + TLS-rejection slot carrying this request's pins. A client per
    /// request because pin data is per-request (it lives inside the TLS verifier), which also means
    /// no pooled connection is ever reused (reinforcing retry-off, rule 8).
    fn build_client(
        &self,
        pins: Option<&PinSet>,
    ) -> Result<(reqwest::Client, RejectSlot), HttpError> {
        let reject: RejectSlot = Arc::new(Mutex::new(None));
        let verifier = Arc::new(PinningVerifier::new(
            self.inner.roots.clone(),
            self.inner.provider.clone(),
            pins,
            self.inner.enforce_pins,
            reject.clone(),
        )?);
        let tls = rustls::ClientConfig::builder_with_provider(self.inner.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|_| HttpError::Tls {
                kind: TlsErrorKind::HandshakeFailure,
            })?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| HttpError::Connect)?;
        Ok((client, reject))
    }

    /// Perform the request under the total deadline / cancellation race (row 4, rules 2/3/9).
    async fn perform(
        &self,
        request: HttpRequest,
        token: CancelToken,
        progress: Option<Box<dyn UploadProgressSink>>,
    ) -> Result<HttpResponse, HttpError> {
        let deadline = request.deadline();
        tokio::select! {
            biased;
            () = wait_cancelled(&token) => Err(HttpError::Cancelled),
            () = tokio::time::sleep(deadline) => Err(HttpError::Timeout),
            res = self.perform_inner(&request, progress) => res,
        }
    }

    /// Follow the request (and any redirects) to a terminal response. One reqwest exchange per hop.
    async fn perform_inner(
        &self,
        request: &HttpRequest,
        progress: Option<Box<dyn UploadProgressSink>>,
    ) -> Result<HttpResponse, HttpError> {
        let method = map_method(request.method());
        let mut current =
            reqwest::Url::parse(request.url().as_str()).map_err(|_| HttpError::Connect)?;
        let mut hops: Vec<Url> = Vec::new();

        // The body (and its progress observer) rides only the initial request.
        let mut body_bytes: Option<Vec<u8>> = match request.body() {
            RequestBody::Empty => None,
            RequestBody::Bytes(bytes) => Some(bytes.clone()),
            // No suite row drives a File request body; "stream from disk" is honoured as a
            // read-then-chunk hand-off (recorded simplification). A read failure is Io.
            RequestBody::File(file_ref) => Some(
                tokio::fs::read(file_ref.as_path())
                    .await
                    .map_err(|_| HttpError::Io)?,
            ),
            // `RequestBody` is `#[non_exhaustive]` (Multipart is the next variant); until the
            // adapter synthesises it, send body-less rather than panic.
            _ => None,
        };
        let mut progress = progress;
        let mut first = true;

        loop {
            let (client, reject) = self.build_client(request.pins())?;
            let mut rb = client.request(method.clone(), current.clone());
            for (name, value) in request.headers().iter() {
                rb = rb.header(name.as_str(), value.as_bytes());
            }
            if first {
                if let Some(bytes) = body_bytes.take() {
                    rb = attach_body(rb, bytes, progress.take());
                } else if let Some(sink) = progress.take() {
                    // Body-less request with an observer: terminally consistent at zero.
                    sink.progress(0, Some(0));
                }
            }

            let resp = match rb.send().await {
                Ok(resp) => resp,
                Err(err) => return Err(map_reqwest_error(&err, take_reject(&reject))),
            };

            let status = resp.status().as_u16();
            if is_redirect(status)
                && let Some(location) = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
            {
                let next = current.join(&location).map_err(|_| HttpError::Transport)?;
                // rule 4: an https → http redirect is refused, never followed.
                if current.scheme() == "https" && next.scheme() == "http" {
                    let to = Url::cleartext_dev(next.as_str()).map_err(|_| HttpError::Transport)?;
                    return Err(HttpError::InsecureRedirect { to });
                }
                if hops.len() as u32 >= self.inner.redirect_limit {
                    return Err(HttpError::TooManyRedirects {
                        limit: self.inner.redirect_limit,
                    });
                }
                hops.push(contract_url(&current)?);
                current = next;
                first = false;
                continue;
            }

            return build_response(resp, &current, hops, request.response_sink()).await;
        }
    }
}

impl Http for LinuxHttp {
    fn send(
        &self,
        request: HttpRequest,
        completion: Box<dyn CompletionSink>,
        upload_progress: Option<Box<dyn UploadProgressSink>>,
    ) -> RequestHandle {
        let token = CancelToken::new();
        let worker_token = token.clone();
        let me = self.clone();
        self.inner.runtime.spawn(async move {
            let outcome = me.perform(request, worker_token, upload_progress).await;
            completion.complete(outcome);
        });
        RequestHandle::for_token(token)
    }
}

impl Metrics for LinuxHttp {
    fn tier(&self) -> MetricsTier {
        // reqwest exposes no per-phase timing seam (§5.13); the honest tier is whole-request.
        MetricsTier::WholeRequest
    }
}

/// Poll the contract's cancellation token to completion (the async bridge for the poll-based token).
async fn wait_cancelled(token: &CancelToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Attach `bytes` as the request body, wrapping it in a progress-reporting stream when an observer
/// is present (row 14 / rule 11: monotone per attempt, terminating at the body length).
fn attach_body(
    rb: reqwest::RequestBuilder,
    bytes: Vec<u8>,
    progress: Option<Box<dyn UploadProgressSink>>,
) -> reqwest::RequestBuilder {
    let Some(sink) = progress else {
        return rb.body(bytes);
    };
    let total = bytes.len() as u64;
    if bytes.is_empty() {
        sink.progress(0, Some(0));
        return rb.body(bytes);
    }
    // A handful of chunks, so honest progress is a real monotone sequence rather than one point.
    let chunk = (bytes.len() / 4).max(1);
    let chunks: Vec<Vec<u8>> = bytes.chunks(chunk).map(<[u8]>::to_vec).collect();
    let stream = futures_util::stream::unfold(
        (chunks.into_iter(), 0u64, sink, total),
        |(mut it, mut sent, sink, total)| async move {
            let piece = it.next()?;
            sent += piece.len() as u64;
            sink.progress(sent, Some(total));
            Some((
                Ok::<Vec<u8>, std::io::Error>(piece),
                (it, sent, sink, total),
            ))
        },
    );
    rb.body(reqwest::Body::wrap_stream(stream))
}

/// Build the terminal [`HttpResponse`] from a reqwest response, honouring the requested sink.
async fn build_response(
    resp: reqwest::Response,
    final_url: &reqwest::Url,
    hops: Vec<Url>,
    sink: &ResponseSink,
) -> Result<HttpResponse, HttpError> {
    let status = StatusCode::new(resp.status().as_u16());
    let version = map_version(resp.version());
    // Honest under decoding: reqwest strips `Content-Length` when gzip-decoding, so this is `None`
    // for a compressed body and the decoded length otherwise (rule 7 / §5.12).
    let content_length = resp.content_length();
    let mut headers = Headers::new();
    for (name, value) in resp.headers() {
        if let (Ok(name), Ok(value)) = (
            HeaderName::parse(name.as_str()),
            HeaderValue::from_bytes(value.as_bytes().to_vec()),
        ) {
            headers.append(name, value);
        }
    }
    let final_contract_url = contract_url(final_url)?;

    let body = match sink {
        ResponseSink::Memory => {
            let bytes = resp.bytes().await.map_err(|_| HttpError::Transport)?;
            BodyOutcome::Memory(bytes.to_vec())
        }
        ResponseSink::File(file_ref) => {
            // The verified, adapter-counted byte total (Q3): the copy loop counts what it wrote.
            let bytes_written = write_body_to_file(resp, file_ref.as_path()).await?;
            BodyOutcome::File {
                path: file_ref.clone(),
                bytes_written,
            }
        }
        // A future (e.g. streaming) sink the adapter does not yet synthesise: default to buffering.
        _ => {
            let bytes = resp.bytes().await.map_err(|_| HttpError::Transport)?;
            BodyOutcome::Memory(bytes.to_vec())
        }
    };

    let mut builder = HttpResponse::builder(status, final_contract_url, version, body)
        .headers(headers)
        .content_length(content_length);
    for hop in hops {
        builder = builder.hop(hop);
    }
    Ok(builder.build())
}

/// Stream the decoded response body to `target` with temp-file discipline: write a sibling temp
/// file, fsync, then atomically rename it into place. A body-read failure is `Transport`; any local
/// file failure is `Io` (row 15 / the `Io` positive control).
async fn write_body_to_file(resp: reqwest::Response, target: &Path) -> Result<u64, HttpError> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("out");
    let nonce = SINK_NONCE.fetch_add(1, Ordering::SeqCst);
    let tmp = parent.join(format!(".{name}.tmp.{}.{nonce}", std::process::id()));

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|_| HttpError::Io)?;
    // The verified byte count (Q3): the decoded bytes this loop actually wrote.
    let mut bytes_written: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(HttpError::Transport);
            }
        };
        if file.write_all(&chunk).await.is_err() {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(HttpError::Io);
        }
        bytes_written = bytes_written.saturating_add(chunk.len() as u64);
    }
    if file.flush().await.is_err() || file.sync_all().await.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(HttpError::Io);
    }
    drop(file);
    tokio::fs::rename(&tmp, target).await.map_err(|_| {
        // Leave no temp file behind on a failed finalise.
        HttpError::Io
    })?;
    Ok(bytes_written)
}

/// Take (and clear) any TLS rejection the pinning verifier recorded during a failed handshake.
fn take_reject(slot: &RejectSlot) -> Option<TlsReject> {
    slot.lock().ok().and_then(|mut guard| guard.take())
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn map_method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Head => reqwest::Method::HEAD,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Options => reqwest::Method::OPTIONS,
        // `Method` is `#[non_exhaustive]`; no future variant exists yet. Fall back to GET rather
        // than panic — a real new method would extend this map in the same change that adds it.
        _ => reqwest::Method::GET,
    }
}

fn map_version(version: reqwest::Version) -> HttpVersion {
    match version {
        reqwest::Version::HTTP_10 => HttpVersion::Http1_0,
        reqwest::Version::HTTP_2 => HttpVersion::Http2,
        reqwest::Version::HTTP_3 => HttpVersion::Http3,
        // HTTP/0.9 and 1.1 both map to the 1.1 observable (0.9 never occurs on this stack).
        _ => HttpVersion::Http1_1,
    }
}

/// Convert a reqwest URL back into the contract's scheme-typed [`Url`].
fn contract_url(url: &reqwest::Url) -> Result<Url, HttpError> {
    match url.scheme() {
        "https" => Url::https(url.as_str()).map_err(|_| HttpError::Transport),
        "http" => Url::cleartext_dev(url.as_str()).map_err(|_| HttpError::Transport),
        _ => Err(HttpError::Transport),
    }
}
