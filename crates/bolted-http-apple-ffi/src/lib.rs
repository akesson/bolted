//! `bolted-http-apple-ffi` — the Apple **harness bridge** for the `bolted-http` capability
//! contract (step 25, milestone M0).
//!
//! This crate is the effect-side analog of the generated capability glue: the [`Http`] capability
//! crosses the FFI as a BoltFFI **callback trait** ([`HttpAdapter`]) that a hand-written Swift
//! URLSession adapter implements. On the Rust side it exposes three things to `swift test`:
//!
//! 1. a **conformance driver** ([`HttpHarness::run_c1`]) that runs the real `bolted-http`
//!    conformance rows against the registered Swift adapter and returns **structured** per-row
//!    results ([`RowReport`] — pass/fail plus a legible message, never a bare bool);
//! 2. **test-server lifecycle** control ([`HttpHarness::start_server`] / [`HttpHarness::stop_server`],
//!    which expose the three listeners' base URLs);
//! 3. the **completion re-entry** points the Swift adapter calls back through
//!    ([`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`]).
//!
//! It never reimplements the suite: the rows, the `TestServer`, and the `AdapterFactory` seam all
//! live behind `bolted-http`'s `conformance` feature and are adapted across the boundary here.
//!
//! ## The bridge, end to end
//!
//! A conformance row calls `factory.new_adapter()` and drives it with the suite's blocking
//! `drive_*` helpers. Our [`AdapterFactory`] yields a [`SwiftAdapter`] shim whose
//! [`Http::send`] (a) mints a single-flight token, (b) parks the row's [`CompletionSink`] in a
//! token-keyed registry, (c) converts the [`HttpRequest`] into the FFI-shaped [`FfiRequest`], and
//! (d) calls the Swift adapter's `execute`. `execute` returns immediately (URLSession is async);
//! the completion arrives later on a URLSession background thread and re-enters through
//! [`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`], which look the token up, convert
//! back to the contract types, and deliver to the parked sink — unblocking the row.
//!
//! M0 scope: the walking-skeleton Swift adapter honours exactly one C1 row (rule 1 — GET `/ok`).
//! The other rows are expected to report red; the point of M0 is that the bridge can carry a green
//! *and* be proven able to carry a red.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use boltffi::*;

use bolted_http::capability::{
    CancelToken, CompletionSink, Http, RequestHandle, UploadProgressSink,
};
use bolted_http::conformance::server::TestServer;
use bolted_http::conformance::{
    AdapterFactory, ConformanceCtx, ConformanceRow, Endpoints, FailureReason, RowResult, c1, c2,
    run,
};
use bolted_http::request::{HttpRequest, Method, RequestBody};
use bolted_http::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use bolted_http::{HeaderName, HeaderValue, Headers, HttpError, TlsErrorKind, Url};

// =====================================================================================
// The FFI data surface — plain `#[data]` mirrors of the contract types. BoltFFI's bindgen
// reads these as SOURCE TEXT and emits the Swift structs/enums; the rich `bolted-http` types
// (which are not `#[data]`) never cross the boundary — this crate is the homogenization seam.
// =====================================================================================

/// One request header crossing the FFI (name + value, both UTF-8 strings for M0).
#[data]
pub struct FfiHeader {
    pub name: String,
    pub value: String,
}

/// A request effect flattened for the boundary. `token` is the single-flight identity: the
/// completion must carry the same token to be matched to its parked sink.
#[data]
pub struct FfiRequest {
    pub token: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<FfiHeader>,
    pub body: Vec<u8>,
    /// One total deadline in milliseconds (the only timeout the portable contract carries).
    pub deadline_ms: u64,
}

/// The negotiated HTTP version, mirrored across the boundary (feature-matrix row 11). M1 drops the
/// M0 `Http1_1` placeholder: the Swift adapter reads the real protocol from `URLSessionTaskMetrics`
/// (`networkProtocolName`) and reports it here.
#[data]
#[derive(Clone, Copy)]
pub enum FfiHttpVersion {
    Http1_0,
    Http1_1,
    Http2,
    Http3,
}

/// A successful response re-entering the core as a typed completion input.
#[data]
pub struct FfiResponse {
    pub token: u64,
    pub status: u16,
    pub headers: Vec<FfiHeader>,
    pub body: Vec<u8>,
    /// The final URL after any redirects (rule 6). Empty is treated as a bridge error.
    pub final_url: String,
    /// The negotiated HTTP version, read from `URLSessionTaskMetrics` (row 11). No longer a
    /// placeholder.
    pub http_version: FfiHttpVersion,
}

/// The typed error keys the Swift adapter maps native failures to. M1 covers the full C2 taxonomy
/// the URLSession adapter can reach on the host tier; each maps to a [`HttpError`] variant so the
/// adapter reports keys, never strings. The pin/insecure-redirect/permission/io keys are the M2
/// syntheses and attach to this enum then (additive).
#[data]
#[derive(Clone)]
pub enum FfiHttpError {
    /// The deadline elapsed (synthesized total-deadline timer, or `URLError.timedOut`).
    Timeout,
    /// The caller cancelled the in-flight effect (`URLError.cancelled`, not deadline-caused).
    Cancelled,
    /// DNS / name resolution failed.
    NameResolution,
    /// A connection could not be established.
    Connect,
    /// A TLS failure (handshake / trust). The pin-vs-trust kind split is M2.
    Tls,
    /// The redirect chain exceeded the limit (`URLError.httpTooManyRedirects`). `limit` is the
    /// ceiling that fired; URLSession enforces its own internal cap in M1 (the request carries no
    /// redirect limit and the delegate-driven policy is M2), so `0` is the "adapter-internal cap"
    /// sentinel — no conformance row inspects it, only the key.
    TooManyRedirects { limit: u32 },
    /// Any other post-connection transport failure. `message` is informational only.
    Transport { message: String },
}

/// The three test-server base URLs handed to Swift on [`HttpHarness::start_server`], plus the TLS
/// material the HTTPS rows need: the good cert's DER (a trust anchor the adapter installs so its
/// server-trust evaluation accepts the self-signed test endpoint — anchor-only for M1) and the
/// good / untrusted SPKI hashes (32 bytes each; the pin **enforcement** that consumes them is M2,
/// but they cross now so M2 adds no data surface).
#[data]
pub struct ServerInfo {
    pub http_base: String,
    pub https_base: String,
    pub https_untrusted_base: String,
    /// The good (trusted) endpoint's certificate, DER-encoded — the adapter's trust anchor.
    pub good_cert_der: Vec<u8>,
    /// SHA-256 of the good cert's SubjectPublicKeyInfo (the pin that matches `https_base`).
    pub good_spki: Vec<u8>,
    /// SHA-256 of the untrusted cert's SPKI (a *wrong* pin for `https_base` — the rule-10 mismatch).
    pub untrusted_spki: Vec<u8>,
}

/// One conformance row's structured outcome. `message` is the `Debug` render of the typed
/// [`FailureReason`] (or skip reason) so a Swift test can print *why* a row went red.
#[data]
pub struct RowReport {
    pub id: String,
    pub passed: bool,
    pub skipped: bool,
    pub message: String,
}

// =====================================================================================
// The callback trait Swift implements.
// =====================================================================================

/// The HTTP capability as it crosses the FFI: the Swift URLSession adapter implements `execute`.
/// It performs the request out-of-process (asynchronously) and delivers the completion back
/// through [`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`].
#[export]
pub trait HttpAdapter: Send + Sync {
    /// Dispatch a request effect. Must return promptly (URLSession `resume()` is non-blocking);
    /// the completion is delivered later, carrying the request's `token`.
    fn execute(&self, request: FfiRequest);

    /// Forward a caller cancellation to the in-flight task identified by `token` (rule 9 — the
    /// adapter cancels the `URLSessionTask`, which completes with `URLError.cancelled`, which the
    /// adapter maps to [`FfiHttpError::Cancelled`]). A no-op if the token is unknown / already done.
    fn cancel(&self, token: u64);
}

// =====================================================================================
// Shared bridge state + the `Http` shim that fronts the Swift adapter for the suite.
// =====================================================================================

/// A parked row completion, keyed by token until the Swift adapter delivers.
struct Pending {
    completion: Box<dyn CompletionSink>,
    /// The row's upload-progress sink, if any (rule 11). The Swift adapter's `didSendBodyData`
    /// delegate re-enters [`HttpHarness::report_progress`], which forwards to this sink.
    progress: Option<Box<dyn UploadProgressSink>>,
}

/// State shared between the `Http` shim (which needs the Swift adapter + the token registry) and
/// the harness completion entry points (which need the registry).
struct Shared {
    adapter: Arc<dyn HttpAdapter>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_token: AtomicU64,
}

/// The per-row `Http` implementation the suite drives: a thin shim that forwards to the one
/// registered Swift adapter (URLSession is stateless per request, so every row shares it).
struct SwiftAdapter {
    shared: Arc<Shared>,
}

impl Http for SwiftAdapter {
    fn send(
        &self,
        request: HttpRequest,
        completion: Box<dyn CompletionSink>,
        upload_progress: Option<Box<dyn UploadProgressSink>>,
    ) -> RequestHandle {
        let token = self.shared.next_token.fetch_add(1, Ordering::Relaxed);
        let ffi = to_ffi_request(token, &request);
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.insert(
                token,
                Pending {
                    completion,
                    progress: upload_progress,
                },
            );
        }
        // Rust → Swift: performs the request asynchronously and returns immediately.
        self.shared.adapter.execute(ffi);

        // Bridge the contract's poll-based cancellation to the Swift task: a detached watcher polls
        // the returned token (the Linux adapter's 10 ms poll, mirrored) and, on cancellation,
        // forwards `adapter.cancel(token)` across the FFI so the URLSessionTask is cancelled. The
        // watcher self-terminates when the request completes (its pending entry is removed) so no
        // thread outlives its request.
        let cancel_token = CancelToken::new();
        let watcher = cancel_token.clone();
        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            loop {
                if watcher.is_cancelled() {
                    shared.adapter.cancel(token);
                    break;
                }
                let still_pending = shared
                    .pending
                    .lock()
                    .map(|p| p.contains_key(&token))
                    .unwrap_or(false);
                if !still_pending {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        RequestHandle::for_token(cancel_token)
    }
}

/// The factory the suite reads adapters from. Each `new_adapter()` shares the same `Shared`.
struct SwiftFactory {
    shared: Arc<Shared>,
}

impl AdapterFactory for SwiftFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(SwiftAdapter {
            shared: Arc::clone(&self.shared),
        })
    }
    // priority_hint() / metrics() default to absent for M0 (the C3 Apple column is M2).
}

// =====================================================================================
// The exported harness: construction, server lifecycle, completion re-entry, the driver.
// =====================================================================================

/// The Rust half of the bridge Swift drives. Constructed with the Swift adapter; owns the shared
/// registry and (once started) the in-process test server.
pub struct HttpHarness {
    shared: Arc<Shared>,
    server: Mutex<Option<(TestServer, Endpoints)>>,
}

#[export]
impl HttpHarness {
    /// Build the harness over the registered Swift adapter (the composition-root dance: adapter
    /// first, harness second, then the Swift side sets its weak back-reference to this harness).
    pub fn new(adapter: Arc<dyn HttpAdapter>) -> Self {
        HttpHarness {
            shared: Arc::new(Shared {
                adapter,
                pending: Mutex::new(HashMap::new()),
                next_token: AtomicU64::new(1),
            }),
            server: Mutex::new(None),
        }
    }

    /// Start the in-process conformance test server (three listeners) and return its base URLs.
    /// Idempotent-ish: a second call replaces the previous server. Returns empty bases on failure
    /// (the server crate only fails on cert/bind errors — surfaced as empty rather than a panic).
    pub fn start_server(&self) -> ServerInfo {
        match TestServer::start() {
            Ok(server) => {
                let endpoints = Endpoints::from_server(&server);
                let info = ServerInfo {
                    http_base: server.http_base(),
                    https_base: server.https_base(),
                    https_untrusted_base: server.https_untrusted_base(),
                    good_cert_der: endpoints.good_cert_der().to_vec(),
                    good_spki: endpoints.good_spki().to_vec(),
                    untrusted_spki: endpoints.untrusted_spki().to_vec(),
                };
                if let Ok(mut slot) = self.server.lock() {
                    *slot = Some((server, endpoints));
                }
                info
            }
            Err(_) => ServerInfo {
                http_base: String::new(),
                https_base: String::new(),
                https_untrusted_base: String::new(),
                good_cert_der: Vec::new(),
                good_spki: Vec::new(),
                untrusted_spki: Vec::new(),
            },
        }
    }

    /// Shut the test server down (drops the listeners).
    pub fn stop_server(&self) {
        if let Ok(mut slot) = self.server.lock() {
            *slot = None;
        }
    }

    /// Success completion re-entry: match the token to its parked sink and deliver the response.
    /// Unknown / stale tokens are dropped (single-flight — the first completion wins).
    pub fn complete_ok(&self, response: FfiResponse) {
        let Some(pending) = self.take_pending(response.token) else {
            return;
        };
        pending.completion.complete(to_http_response(&response));
    }

    /// Failure completion re-entry: match the token and deliver the typed error.
    pub fn complete_err(&self, token: u64, error: FfiHttpError) {
        let Some(pending) = self.take_pending(token) else {
            return;
        };
        pending.completion.complete(Err(to_http_error(&error)));
    }

    /// Upload-progress re-entry (rule 11): forward the Swift adapter's `didSendBodyData` figures to
    /// the parked [`UploadProgressSink`] **without** removing the pending entry (progress is
    /// repeatable; only a completion consumes the entry). `total` is `None` when the body length is
    /// not known up front (`NSURLSessionTransferSizeUnknown`).
    pub fn report_progress(&self, token: u64, sent: u64, total: Option<u64>) {
        if let Ok(pending) = self.shared.pending.lock()
            && let Some(entry) = pending.get(&token)
            && let Some(sink) = entry.progress.as_ref()
        {
            sink.progress(sent, total);
        }
    }

    /// Run the eleven C1 conformance rows against the registered Swift adapter (structured results).
    /// Requires a started server; without one, reports the missing-server state rather than panicking.
    pub fn run_c1(&self) -> Vec<RowReport> {
        self.run_rows(c1::rows())
    }

    /// Run the C2 error-taxonomy rows (one positive control per reachable key) against the adapter.
    pub fn run_c2(&self) -> Vec<RowReport> {
        self.run_rows(c2::rows())
    }
}

impl HttpHarness {
    /// Remove and return the parked completion for `token`, if present.
    fn take_pending(&self, token: u64) -> Option<Pending> {
        self.shared
            .pending
            .lock()
            .ok()
            .and_then(|mut p| p.remove(&token))
    }

    /// Drive `rows` against the registered Swift adapter over the started server, projecting each
    /// [`RowResult`] onto a structured [`RowReport`]. Shared by [`HttpHarness::run_c1`] / `run_c2`.
    fn run_rows(&self, rows: &[ConformanceRow]) -> Vec<RowReport> {
        let guard = match self.server.lock() {
            Ok(g) => g,
            Err(_) => return vec![no_server_report()],
        };
        let Some((_server, endpoints)) = guard.as_ref() else {
            return vec![no_server_report()];
        };
        let factory = SwiftFactory {
            shared: Arc::clone(&self.shared),
        };
        let ctx = ConformanceCtx {
            factory: &factory,
            endpoints,
        };
        run(rows, &ctx)
            .into_iter()
            .map(|(id, result)| to_row_report(id, &result))
            .collect()
    }
}

// =====================================================================================
// Conversions between the FFI surface and the `bolted-http` contract types.
// =====================================================================================

/// Flatten a contract [`HttpRequest`] into the FFI [`FfiRequest`].
fn to_ffi_request(token: u64, request: &HttpRequest) -> FfiRequest {
    let headers = request
        .headers()
        .iter()
        .map(|(name, value)| FfiHeader {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();
    let body = match request.body() {
        RequestBody::Bytes(bytes) => bytes.clone(),
        // A file body is read into memory for the boundary (no suite row drives one in M0).
        RequestBody::File(file) => std::fs::read(file.as_path()).unwrap_or_default(),
        _ => Vec::new(),
    };
    FfiRequest {
        token,
        method: method_str(request.method()).to_owned(),
        url: request.url().as_str().to_owned(),
        headers,
        body,
        deadline_ms: u64::try_from(request.deadline().as_millis()).unwrap_or(u64::MAX),
    }
}

/// Build a contract [`HttpResponse`] from an [`FfiResponse`]. A missing / malformed `final_url` is
/// a bridge fault and surfaces as `Transport` (the completion must still fire).
fn to_http_response(response: &FfiResponse) -> Result<HttpResponse, HttpError> {
    let Some(url) = parse_final_url(&response.final_url) else {
        return Err(HttpError::Transport);
    };
    let mut headers = Headers::new();
    for header in &response.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::parse(&header.name),
            HeaderValue::from_bytes(header.value.clone().into_bytes()),
        ) {
            headers.append(name, value);
        }
    }
    // M1: the version is the adapter's real `URLSessionTaskMetrics` observable, not a placeholder.
    // `content_length` is the decoded in-memory body length — always honest for a `Memory` sink
    // (`Some(n)` promises `n` decoded bytes), so rule 7's decoded-gzip length check is satisfied
    // without ever reporting the compressed transport figure (§5.12).
    let built = HttpResponse::builder(
        StatusCode::new(response.status),
        url,
        to_http_version(response.http_version),
        BodyOutcome::Memory(response.body.clone()),
    )
    .headers(headers)
    .content_length(Some(response.body.len() as u64))
    .build();
    Ok(built)
}

/// Map the FFI version mirror to the contract [`HttpVersion`].
fn to_http_version(version: FfiHttpVersion) -> HttpVersion {
    match version {
        FfiHttpVersion::Http1_0 => HttpVersion::Http1_0,
        FfiHttpVersion::Http1_1 => HttpVersion::Http1_1,
        FfiHttpVersion::Http2 => HttpVersion::Http2,
        FfiHttpVersion::Http3 => HttpVersion::Http3,
    }
}

/// Map the FFI error key to the contract [`HttpError`].
fn to_http_error(error: &FfiHttpError) -> HttpError {
    match error {
        FfiHttpError::Timeout => HttpError::Timeout,
        FfiHttpError::Cancelled => HttpError::Cancelled,
        FfiHttpError::NameResolution => HttpError::NameResolution,
        FfiHttpError::Connect => HttpError::Connect,
        FfiHttpError::Tls => HttpError::Tls {
            kind: TlsErrorKind::HandshakeFailure,
        },
        FfiHttpError::TooManyRedirects { limit } => HttpError::TooManyRedirects { limit: *limit },
        FfiHttpError::Transport { .. } => HttpError::Transport,
    }
}

/// Re-type a final-URL string as the contract's scheme-typed [`Url`] (or `None` if unusable).
fn parse_final_url(url: &str) -> Option<Url> {
    if url.len() >= 8 && url[..8].eq_ignore_ascii_case("https://") {
        Url::https(url).ok()
    } else if url.len() >= 7 && url[..7].eq_ignore_ascii_case("http://") {
        Url::cleartext_dev(url).ok()
    } else {
        None
    }
}

/// The wire method name for a contract [`Method`].
fn method_str(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Options => "OPTIONS",
        // `Method` is `#[non_exhaustive]`; a future variant sends as GET rather than panicking.
        _ => "GET",
    }
}

/// Project a suite [`RowResult`] onto the structured [`RowReport`], rendering the typed reason.
fn to_row_report(id: &str, result: &RowResult) -> RowReport {
    match result {
        RowResult::Pass => RowReport {
            id: id.to_owned(),
            passed: true,
            skipped: false,
            message: String::new(),
        },
        RowResult::Fail(reason) => RowReport {
            id: id.to_owned(),
            passed: false,
            skipped: false,
            message: describe_failure(reason),
        },
        RowResult::Skipped(reason) => RowReport {
            id: id.to_owned(),
            passed: false,
            skipped: true,
            message: format!("skipped: {reason:?}"),
        },
    }
}

/// A legible message for a typed failure reason (the `Debug` render is the data-shaped truth).
fn describe_failure(reason: &FailureReason) -> String {
    format!("{reason:?}")
}

/// The placeholder report when `run_c1` is called before `start_server`.
fn no_server_report() -> RowReport {
    RowReport {
        id: "C1/harness".to_owned(),
        passed: false,
        skipped: false,
        message: "test server not started (call start_server first)".to_owned(),
    }
}
