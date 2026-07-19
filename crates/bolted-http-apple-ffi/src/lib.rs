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

use boltffi::*;

use bolted_http::capability::{
    CancelToken, CompletionSink, Http, RequestHandle, UploadProgressSink,
};
use bolted_http::conformance::server::TestServer;
use bolted_http::conformance::{
    AdapterFactory, ConformanceCtx, Endpoints, FailureReason, RowResult, c1, run,
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

/// A successful response re-entering the core as a typed completion input.
#[data]
pub struct FfiResponse {
    pub token: u64,
    pub status: u16,
    pub headers: Vec<FfiHeader>,
    pub body: Vec<u8>,
    /// The final URL after any redirects (rule 6). Empty is treated as a bridge error.
    pub final_url: String,
}

/// The typed error keys the Swift adapter maps native failures to. A deliberately small,
/// M0-sized set (the full C2 taxonomy mapping is M1); each maps to a [`HttpError`] variant so the
/// adapter reports keys, never strings.
#[data]
#[derive(Clone)]
pub enum FfiHttpError {
    /// The deadline elapsed.
    Timeout,
    /// The caller cancelled the in-flight effect.
    Cancelled,
    /// DNS / name resolution failed.
    NameResolution,
    /// A connection could not be established.
    Connect,
    /// A TLS failure (handshake / trust). The kind split is M1.
    Tls,
    /// Any other post-connection transport failure. `message` is informational only.
    Transport { message: String },
}

/// The three test-server base URLs handed to Swift on [`HttpHarness::start_server`].
#[data]
pub struct ServerInfo {
    pub http_base: String,
    pub https_base: String,
    pub https_untrusted_base: String,
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
}

// =====================================================================================
// Shared bridge state + the `Http` shim that fronts the Swift adapter for the suite.
// =====================================================================================

/// A parked row completion, keyed by token until the Swift adapter delivers.
struct Pending {
    completion: Box<dyn CompletionSink>,
    /// The row's upload-progress sink, if any. Parked for M1 (the M0 skeleton never reports
    /// progress, so a progress-driven row terminates inconsistently — a legitimate red).
    _progress: Option<Box<dyn UploadProgressSink>>,
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
                    _progress: upload_progress,
                },
            );
        }
        // Rust → Swift: performs the request asynchronously and returns immediately.
        self.shared.adapter.execute(ffi);
        // Cancellation is not wired to Swift in M0 (rule 9 is an M1 row); a fresh token means a
        // cancel is a no-op, and the cancel rows report red — the honest M0 state.
        RequestHandle::for_token(CancelToken::new())
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

    /// Run the eleven C1 conformance rows against the registered Swift adapter and return each
    /// row's structured result. Requires a started server; without one, every row reports the
    /// missing-server state rather than panicking.
    pub fn run_c1(&self) -> Vec<RowReport> {
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
        run(c1::rows(), &ctx)
            .into_iter()
            .map(|(id, result)| to_row_report(id, &result))
            .collect()
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
    // M0 reports the version as HTTP/1.1 unconditionally (the real observable is an M1 concern).
    let built = HttpResponse::builder(
        StatusCode::new(response.status),
        url,
        HttpVersion::Http1_1,
        BodyOutcome::Memory(response.body.clone()),
    )
    .headers(headers)
    .build();
    Ok(built)
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
