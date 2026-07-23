//! `bolted-http-linux` — the **reqwest reference adapter** for the [`bolted_http`] capability
//! contract (feature-matrix S-LX / step-24 M3). This is the adapter the conformance suite is
//! debugged against after the mock (the one-implementor lesson: mock first, then reqwest).
//!
//! ## The executor lives here
//!
//! `bolted-http` is sans-io and dependency-clean; an **adapter owns its executor**. [`LinuxHttp`]
//! contains a tokio multi-thread runtime. [`crate::LinuxHttp::send`] returns a [`RequestHandle`]
//! immediately and spawns the exchange onto that runtime; the single completion is delivered to the
//! [`CompletionSink`]. Cancellation is **pushed** (Q4 / streaming-seam §3b): `RequestHandle::cancel`
//! fires a [`FlowSignal::Cancel`] whose [`FlowObserver`] notifies a tokio [`Notify`] the request's
//! `select!` races — there is **no** 10 ms poll-watcher thread (the one every adapter paid before).
//!
//! ## Streaming (row 16 / rules 12–13)
//!
//! [`LinuxHttp`] also implements [`StreamingHttp`]: [`crate::LinuxHttp::send_streaming`] streams the
//! response body chunk-by-chunk (reqwest `bytes_stream` → `deliver_chunk`) into a driver-owned
//! ingest, honouring pushed pause by socket read-pacing (it stops polling the stream while paused,
//! so the core ring never overflows) and closing with the real terminal (`Complete { total }` from
//! counted bytes, or `Failed` on a mid-body error).
//!
//! [`FlowSignal::Cancel`]: bolted_http::signal::FlowSignal::Cancel
//! [`FlowObserver`]: bolted_http::signal::FlowObserver
//! [`Notify`]: tokio::sync::Notify
//! [`StreamingHttp`]: bolted_http::capability::StreamingHttp
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};

use futures_util::StreamExt;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use tokio::io::AsyncWriteExt;
use tokio::runtime::Runtime;
use tokio::sync::Notify;

use bolted_http::capability::CancelToken;
use bolted_http::capability::{
    ChunkSink, CompletionSink, Http, Metrics, MetricsTier, RequestHandle, StreamingHttp,
    UploadProgressSink,
};
use bolted_http::request::{HttpRequest, Method, PinSet, RequestBody, ResponseSink};
use bolted_http::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use bolted_http::signal::{FlowObserver, FlowSignal, FlowSignals};
use bolted_http::stream::{BodyChunk, BodyEnd};
use bolted_http::{
    HeaderName, HeaderValue, Headers, HttpError, RedirectCeiling, TlsErrorKind, Url,
};

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

/// A deliberately-injected streaming fault — the scoped per-adapter red twin for the streaming rows
/// (the `enforce_pins = false` precedent, one fault at a time). `None` is the conformant adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StreamFault {
    /// Conformant: deliver every chunk, honour the terminal.
    #[default]
    None,
    /// Skip delivering the first body chunk but still count its bytes toward the declared total —
    /// the truncation the completeness gate forbids (row 12's Linux red).
    DropChunk,
    /// Deliver every chunk but never send the terminal — the missing-terminal break (row 13's Linux
    /// red).
    SkipTerminal,
}

/// Composition-root configuration for the adapter (the contract's CFG rows — trust roots, redirect
/// ceiling). None of this is ever seen by the sans-io core.
#[derive(Clone, Debug)]
pub struct LinuxHttpConfig {
    /// DER-encoded trust anchors the adapter's **real** chain verification trusts (row 25 / §5.14).
    /// Production wires the system / Mozilla roots here; the conformance suite passes the test
    /// server's self-signed cert. Empty means "trust nothing" — every TLS request then fails.
    pub trust_anchors: Vec<Vec<u8>>,
    /// The redirect-follow ceiling; beyond it a request is [`HttpError::TooManyRedirects`]. Fed into
    /// the core-owned [`RedirectCeiling`] (Q2): the adapter no longer counts inline, the core does.
    pub redirect_limit: u32,
    /// Whether declarative SPKI pins are enforced (default `true`). `false` is the scoped red-twin
    /// that proves the pin check is load-bearing — a pin no longer bites.
    pub enforce_pins: bool,
    /// An injected streaming fault for the streaming rows' red twins (default [`StreamFault::None`]).
    pub stream_fault: StreamFault,
}

impl Default for LinuxHttpConfig {
    fn default() -> Self {
        LinuxHttpConfig {
            trust_anchors: Vec::new(),
            redirect_limit: 10,
            enforce_pins: true,
            stream_fault: StreamFault::None,
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
    /// The core-owned redirect ceiling (Q2). reqwest's native follow is off (`Policy::none`), so it
    /// follows *zero* hops itself — the ceiling below it is the sole authority, counting from the
    /// hop trace the manual loop records.
    redirect_ceiling: RedirectCeiling,
    enforce_pins: bool,
    stream_fault: StreamFault,
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
                redirect_ceiling: RedirectCeiling::new(config.redirect_limit),
                enforce_pins: config.enforce_pins,
                stream_fault: config.stream_fault,
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
    /// Cancellation is **pushed**: the caller's `RequestHandle::cancel` fires the [`Notify`] this
    /// races — no 10 ms poll-watcher thread (streaming-seam §3b / Q4).
    async fn perform(
        &self,
        request: HttpRequest,
        cancel: Arc<Notify>,
        progress: Option<Box<dyn UploadProgressSink>>,
    ) -> Result<HttpResponse, HttpError> {
        let deadline = request.deadline();
        tokio::select! {
            biased;
            () = cancel.notified() => Err(HttpError::Cancelled),
            () = tokio::time::sleep(deadline) => Err(HttpError::Timeout),
            res = self.perform_inner(&request, progress) => res,
        }
    }

    /// Follow the request (and any redirects) to a terminal response, then materialise it into the
    /// requested sink.
    async fn perform_inner(
        &self,
        request: &HttpRequest,
        progress: Option<Box<dyn UploadProgressSink>>,
    ) -> Result<HttpResponse, HttpError> {
        let (resp, current, hops) = self.follow_redirects(request, progress).await?;
        build_response(resp, &current, hops, request.response_sink()).await
    }

    /// Follow the request and any redirects to the terminal reqwest response, returning it with the
    /// final URL and the recorded hop trace. One reqwest exchange per hop; redirect exhaustion is
    /// **core-counted** through [`RedirectCeiling`] (Q2), not an inline adapter count. The body /
    /// upload-progress observer rides only the initial request.
    async fn follow_redirects(
        &self,
        request: &HttpRequest,
        progress: Option<Box<dyn UploadProgressSink>>,
    ) -> Result<(reqwest::Response, reqwest::Url, Vec<Url>), HttpError> {
        let method = map_method(request.method());
        let mut current =
            reqwest::Url::parse(request.url().as_str()).map_err(|_| HttpError::Connect)?;
        let mut hops: Vec<Url> = Vec::new();

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
                hops.push(contract_url(&current)?);
                // Redirect exhaustion is the core's `RedirectCeiling` (Q2), not an inline count. The
                // ceiling counts the hop trace and emits `TooManyRedirects` itself — observably
                // identical to the old inline check for the conformance row (same key, same limit
                // param; the strict-`>` boundary differs from the old `>=` by exactly one hop, which
                // is invisible on `/redirect-loop`).
                self.inner.redirect_ceiling.enforce(&hops)?;
                current = next;
                first = false;
                continue;
            }

            return Ok((resp, current, hops));
        }
    }

    /// Stream the response body chunk-by-chunk into `chunks` (streaming-seam §3a–3c). Follows
    /// redirects to the terminal response, then feeds `bytes_stream` into the driver-owned ingest —
    /// honouring pushed pause (socket read-pacing: it stops polling the stream while paused) and
    /// pushed cancel, under one **total** deadline. Closes with the real terminal: `Complete { total }`
    /// from the bytes it counted, or `Failed(..)` on a mid-body error / cancel / timeout. The
    /// completeness gate and `seq` verification live in the core — this feeds them honestly (or, under
    /// an injected [`StreamFault`], dishonestly, for the row's red twin).
    async fn stream_perform(
        &self,
        request: HttpRequest,
        chunks: Box<dyn ChunkSink>,
        observer: Arc<LinuxFlowObserver>,
    ) {
        let sleep = tokio::time::sleep(request.deadline());
        tokio::pin!(sleep);

        // Phase 1 — connect + follow redirects to the terminal response, under cancel + deadline.
        let resp = {
            let follow = self.follow_redirects(&request, None);
            tokio::select! {
                biased;
                () = observer.cancel.notified() => {
                    chunks.finish(BodyEnd::Failed(HttpError::Cancelled));
                    return;
                }
                () = &mut sleep => {
                    chunks.finish(BodyEnd::Failed(HttpError::Timeout));
                    return;
                }
                result = follow => match result {
                    Ok((resp, _final_url, _hops)) => resp,
                    Err(e) => {
                        chunks.finish(BodyEnd::Failed(e));
                        return;
                    }
                }
            }
        };

        // Phase 2 — stream the body. One `BodyChunk` per transport read (reqwest coalesces, so this
        // is a handful of large chunks on a small body — the ring rarely fills here; the mock is the
        // back-pressure stress).
        let mut stream = resp.bytes_stream();
        let mut seq: u64 = 0;
        let mut counted: u64 = 0;
        let mut dropped_one = false;
        loop {
            // Read-pacing: while paused, stop polling the stream (the socket back-pressures the
            // server). Register the resume waiter before re-checking `paused` (no lost wake-up).
            while observer.paused.load(Ordering::SeqCst) {
                let resumed = observer.resume.notified();
                if !observer.paused.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    biased;
                    () = observer.cancel.notified() => {
                        chunks.finish(BodyEnd::Failed(HttpError::Cancelled));
                        return;
                    }
                    () = &mut sleep => {
                        chunks.finish(BodyEnd::Failed(HttpError::Timeout));
                        return;
                    }
                    () = resumed => {}
                }
            }

            let item = tokio::select! {
                biased;
                () = observer.cancel.notified() => {
                    chunks.finish(BodyEnd::Failed(HttpError::Cancelled));
                    return;
                }
                () = &mut sleep => {
                    chunks.finish(BodyEnd::Failed(HttpError::Timeout));
                    return;
                }
                item = stream.next() => item,
            };

            match item {
                None => {
                    if self.inner.stream_fault == StreamFault::SkipTerminal {
                        // The missing-terminal red twin: drop the sink without finishing.
                        return;
                    }
                    chunks.finish(BodyEnd::Complete { total: counted });
                    return;
                }
                Some(Ok(bytes)) => {
                    // The drop-chunk red twin: skip delivering the first chunk but still count its
                    // bytes toward the declared total, so the completeness gate fires (Transport).
                    if self.inner.stream_fault == StreamFault::DropChunk && !dropped_one {
                        dropped_one = true;
                        counted = counted.saturating_add(bytes.len() as u64);
                        continue;
                    }
                    counted = counted.saturating_add(bytes.len() as u64);
                    if let Err(e) = chunks.deliver_chunk(BodyChunk::new(seq, bytes.to_vec())) {
                        // A seq violation or ring overflow — report it as the terminal.
                        chunks.finish(BodyEnd::Failed(e));
                        return;
                    }
                    seq += 1;
                }
                Some(Err(_)) => {
                    chunks.finish(BodyEnd::Failed(HttpError::Transport));
                    return;
                }
            }
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
        // Pushed cancellation (Q4): `RequestHandle::cancel` fires this `Notify`; no poll-watcher.
        let observer = Arc::new(LinuxFlowObserver::new());
        let cancel = observer.cancel.clone();
        let me = self.clone();
        self.inner.runtime.spawn(async move {
            let outcome = me.perform(request, cancel, upload_progress).await;
            completion.complete(outcome);
        });
        // The token is carried for API symmetry but the adapter no longer polls it — it reacts to
        // the pushed `FlowSignal::Cancel` instead (`RequestHandle::cancel` fires both).
        RequestHandle::with_signals(CancelToken::new(), FlowSignals::new(observer))
    }
}

impl StreamingHttp for LinuxHttp {
    fn send_streaming(&self, request: HttpRequest, chunks: Box<dyn ChunkSink>) -> FlowSignals {
        let observer = Arc::new(LinuxFlowObserver::new());
        let signals = FlowSignals::new(observer.clone());
        let me = self.clone();
        self.inner.runtime.spawn(async move {
            me.stream_perform(request, chunks, observer).await;
        });
        signals
    }
}

impl Metrics for LinuxHttp {
    fn tier(&self) -> MetricsTier {
        // reqwest exposes no per-phase timing seam (§5.13); the honest tier is whole-request.
        MetricsTier::WholeRequest
    }
}

/// The adapter's reaction to the one core→adapter [`FlowSignals`] surface (Q4 + streaming-seam §3b):
/// pushed cancel fires `cancel`; pushed pause/resume drive `paused` + `resume` for socket
/// read-pacing. All tokio primitives — the contract mandates none of them (they live here).
struct LinuxFlowObserver {
    /// Fired by [`FlowSignal::Cancel`]; raced by every deadline/cancel `select!`.
    cancel: Arc<Notify>,
    /// Set by [`FlowSignal::Pause`] / cleared by [`FlowSignal::Resume`]; the stream loop pauses its
    /// read while it is set (back-pressure — the ring never overflows).
    paused: Arc<AtomicBool>,
    /// Fired by [`FlowSignal::Resume`] to wake a paused stream loop.
    resume: Arc<Notify>,
}

impl LinuxFlowObserver {
    fn new() -> Self {
        LinuxFlowObserver {
            cancel: Arc::new(Notify::new()),
            paused: Arc::new(AtomicBool::new(false)),
            resume: Arc::new(Notify::new()),
        }
    }
}

impl FlowObserver for LinuxFlowObserver {
    fn on_signal(&self, signal: FlowSignal) {
        match signal {
            FlowSignal::Pause => self.paused.store(true, Ordering::SeqCst),
            FlowSignal::Resume => {
                self.paused.store(false, Ordering::SeqCst);
                self.resume.notify_waiters();
            }
            // `notify_one` (not `notify_waiters`) so a cancel that arrives before the racing
            // `select!` registers its waiter is not lost — the stored permit wakes the next
            // `notified()`.
            FlowSignal::Cancel => self.cancel.notify_one(),
            // `FlowSignal` is `#[non_exhaustive]`; a future signal this adapter does not model is a
            // no-op rather than a build break.
            _ => {}
        }
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
