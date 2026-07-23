//! `bolted-http-ffi` — the **harness bridge** for the `bolted-http` capability contract, one crate
//! packing both the Apple and Android targets (step 27, milestone M0).
//!
//! This crate is the effect-side analog of the generated capability glue: the [`Http`] capability
//! crosses the FFI as a BoltFFI **callback trait** ([`HttpAdapter`]) that a hand-written native
//! adapter implements — a URLSession adapter on Apple, an OkHttp adapter on Android. The two
//! platforms share this identical FFI surface (it is the decided topology); before step 27 they
//! were two mirror crates whose sole divergence was the apple-only `PriorityHint` capability. Once
//! the priority hint went uniform (Q10 — a plain advisory request field, legally a no-op where the
//! engine can't honour it), the surfaces became identical and the crates merged into this one, the
//! way `gen-profile-ffi` packs apple + android + csharp from a single crate.
//!
//! On the Rust side it exposes three things to the test tiers (`swift test` on macOS; a
//! Gradle-managed-device instrumented suite on ART):
//!
//! 1. a **conformance driver** ([`HttpHarness::run_c1`]) that runs the real `bolted-http`
//!    conformance rows against the registered native adapter and returns **structured** per-row
//!    results ([`RowReport`] — pass/fail plus a legible message, never a bare bool);
//! 2. **test-server lifecycle** control ([`HttpHarness::start_server`] / [`HttpHarness::stop_server`],
//!    which expose the three listeners' base URLs);
//! 3. the **completion re-entry** points the native adapter calls back through
//!    ([`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`]).
//!
//! It never reimplements the suite: the rows, the `TestServer`, and the `AdapterFactory` seam all
//! live behind `bolted-http`'s `conformance` feature and are adapted across the boundary here.
//!
//! ## The bridge, end to end
//!
//! A conformance row calls `factory.new_adapter()` and drives it with the suite's blocking
//! `drive_*` helpers. Our [`AdapterFactory`] yields a [`NativeAdapter`] shim whose
//! [`Http::send`] (a) mints a single-flight token, (b) parks the row's [`CompletionSink`] in a
//! token-keyed registry, (c) converts the [`HttpRequest`] into the FFI-shaped [`FfiRequest`], and
//! (d) calls the native adapter's `execute`. `execute` returns immediately (URLSession `resume()`
//! and OkHttp `Call.enqueue` are both non-blocking); the completion arrives later on a native
//! background thread and re-enters through [`HttpHarness::complete_ok`] /
//! [`HttpHarness::complete_err`], which look the token up, convert back to the contract types, and
//! deliver to the parked sink — unblocking the row.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use boltffi::*;

use bolted_http::capability::{
    CancelToken, CompletionSink, Http, Metrics, MetricsTier, RequestHandle, UploadProgressSink,
};
use bolted_http::conformance::server::TestServer;
use bolted_http::conformance::{
    AdapterFactory, ConformanceCtx, ConformanceRow, Endpoints, FailureReason, RowResult, c1, c2,
    c3, run,
};
use bolted_http::request::{FileRef, HttpRequest, Method, Priority, RequestBody, ResponseSink};
use bolted_http::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use bolted_http::{HeaderName, HeaderValue, Headers, HttpError, TlsErrorKind, Url};

// =====================================================================================
// The FFI data surface — plain `#[data]` mirrors of the contract types. BoltFFI's bindgen
// reads these as SOURCE TEXT and emits the Swift/Kotlin structs/enums; the rich `bolted-http`
// types (which are not `#[data]`) never cross the boundary — this crate is the homogenization seam.
// =====================================================================================

/// One request header crossing the FFI (name + value, both UTF-8 strings for M0).
#[data]
pub struct FfiHeader {
    pub name: String,
    pub value: String,
}

/// One SHA-256-of-SPKI pin the request carries (32 bytes; feature-matrix §5.14, rule 10). The native
/// adapter computes the presented leaf certificate's SPKI SHA-256 and checks membership — a mismatch
/// is [`FfiHttpError::PinMismatch`]. Mirrors the request's `PinSet` across the boundary (M2).
#[data]
pub struct FfiPin {
    /// The 32-byte SHA-256 of a certificate's SubjectPublicKeyInfo.
    pub hash: Vec<u8>,
}

/// Where the caller wants the response body delivered (feature-matrix row 15, the request-side
/// selector). Mirrors [`ResponseSink`] across the boundary (M2): `Memory` buffers, `File` sinks the
/// decoded body to `path` (`downloadTask` on Apple / a streamed file write on Android; atomic
/// finalize; a write failure is [`FfiHttpError::Io`]).
#[data]
pub enum FfiResponseSink {
    /// Buffer the decoded body in memory (the default).
    Memory,
    /// Sink the decoded body to this path.
    File { path: String },
}

/// The request's priority hint (feature-matrix row 12), mirrored across the boundary. **Uniform and
/// advisory** (ruled Q10): every request carries the *data* regardless (defaulting to
/// [`FfiPriority::Normal`] when unset), and each adapter honours it where its engine can express it
/// — Apple maps it to `URLSessionTask.priority` (A5, acceptance-only; the RFC 9218 wire behaviour is
/// FLAGGED lore, deliberately NOT conformance-tested) — and legally ignores it where it cannot
/// (OkHttp exposes no per-`Call` priority knob, so the Android adapter is a conformant no-op). It is
/// no longer a divergent capability, so it has no C3 column. Mirrors [`Priority`].
#[data]
#[derive(Clone, Copy)]
pub enum FfiPriority {
    Throttled,
    Low,
    Normal,
    High,
    Critical,
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
    /// The request's SPKI pins (empty ⇒ no pinning requested); rule 10 (M2). The adapter enforces
    /// these in its trust-evaluation delegate on top of the real chain/hostname check.
    pub pins: Vec<FfiPin>,
    /// Where the response body is delivered (row 15, M2). `Memory` (default) or `File { path }`.
    pub sink: FfiResponseSink,
    /// The request's priority hint (row 12; uniform advisory). Apple maps it to
    /// `URLSessionTask.priority`; OkHttp ignores it (no priority knob). The data rides every
    /// request regardless. Defaults to [`FfiPriority::Normal`] when the request set none.
    pub priority: FfiPriority,
}

/// The negotiated HTTP version, mirrored across the boundary (feature-matrix row 11). The adapter
/// reads the real protocol from the native transport (Apple `URLSessionTaskMetrics.networkProtocolName`;
/// OkHttp `Response.protocol`) and reports it here.
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
    /// The negotiated HTTP version, read from the native transport metrics (row 11).
    pub http_version: FfiHttpVersion,
    /// The redirect hop trace (row 7, M2): every intermediate URL the chain traversed, in order,
    /// excluding the final URL. Empty when no redirect occurred. Captured in the adapter's redirect
    /// interceptor.
    pub hops: Vec<String>,
    /// The file-sink destination when the response was sunk to a file (row 15, M2). Empty ⇒ a
    /// `Memory` outcome carrying `body`; non-empty ⇒ a `File` outcome at this path (the body was
    /// written there, not buffered).
    pub sink_path: String,
}

/// The typed error keys the native adapter maps native failures to. Covers the full C2 taxonomy the
/// URLSession / OkHttp adapters can reach on the host tier; each maps to a [`HttpError`] variant so
/// the adapter reports keys, never strings.
#[data]
#[derive(Clone)]
pub enum FfiHttpError {
    /// The deadline elapsed (synthesized total-deadline timer, or a native timeout —
    /// `URLError.timedOut` / an OkHttp `SocketTimeoutException`).
    Timeout,
    /// The caller cancelled the in-flight effect (`URLError.cancelled` / an OkHttp
    /// `IOException("Canceled")`, not deadline-caused).
    Cancelled,
    /// DNS / name resolution failed.
    NameResolution,
    /// A connection could not be established.
    Connect,
    /// A TLS failure (handshake / trust). The pin-vs-trust split lands in M2: a real chain/hostname
    /// failure is `Tls`, a declarative SPKI pin mismatch is [`FfiHttpError::PinMismatch`] — mirroring
    /// the Linux verifier's split exactly, never conflated.
    Tls,
    /// A declarative SPKI pin did not match the presented leaf (rule 10 / row 19, M2). The chain +
    /// hostname verification *passed*; only the pin failed — distinct from [`FfiHttpError::Tls`].
    PinMismatch,
    /// An `https → http` redirect was refused (rule 4 / row 6, M2). `to` is the cleartext target
    /// that was refused (informational; the key is what the rows inspect).
    InsecureRedirect { to: String },
    /// A local I/O failure handling the response — e.g. a file-sink write failed (row 15 / the `Io`
    /// positive control, M2).
    Io,
    /// The OS refused permission for the request (Apple Local Network privacy / a sandbox network
    /// denial surfacing as POSIX `EPERM`; on Android a missing `INTERNET` permission surfacing as a
    /// `SecurityException`). Distinct from a network failure (§5.15). Platform-gated on the host/ART
    /// tiers — see the M2 notes; the adapter maps a genuine OS permission denial here, never invents
    /// the key.
    PermissionDenied,
    /// The redirect chain exceeded the limit (`URLError.httpTooManyRedirects` / OkHttp's follow-up
    /// cap). `limit` is the ceiling that fired; the native engine enforces its own internal cap in
    /// M1 (the request carries no redirect limit and the delegate-driven policy is M2), so `0` is
    /// the "adapter-internal cap" sentinel — no conformance row inspects it, only the key.
    TooManyRedirects { limit: u32 },
    /// Any other post-connection transport failure. `message` is informational only.
    Transport { message: String },
}

/// The three test-server base URLs handed to the native side on [`HttpHarness::start_server`], plus
/// the TLS material the HTTPS rows need: the good cert's DER (a trust anchor the adapter installs so
/// its server-trust evaluation accepts the self-signed test endpoint — anchor-only for M1) and the
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
/// [`FailureReason`] (or skip reason) so a native test can print *why* a row went red.
#[data]
pub struct RowReport {
    pub id: String,
    pub passed: bool,
    pub skipped: bool,
    pub message: String,
}

/// One response-body chunk crossing the FFI in the **A1 streaming probe** (step 25, probe-grade —
/// no contract surface; the streaming core-seam is deliberately unfrozen, freeze-agenda Q2).
/// Mirrors the step-24 S-FFI `Chunk`. `t_send_ns` is stamped by the native side
/// (`DispatchTime.now().uptimeNanoseconds` / `System.nanoTime()`) immediately before the deliver
/// call so the consumer can compute per-chunk delivery latency on one clock; `last` marks the final
/// chunk.
#[data]
#[derive(Clone)]
pub struct Chunk {
    pub seq: u64,
    pub bytes: Vec<u8>,
    pub t_send_ns: u64,
    pub last: bool,
}

// =====================================================================================
// The callback trait the native side implements.
// =====================================================================================

/// The HTTP capability as it crosses the FFI: the native URLSession / OkHttp adapter implements
/// `execute`. It performs the request out-of-process (asynchronously) and delivers the completion
/// back through [`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`].
#[export]
pub trait HttpAdapter: Send + Sync {
    /// Dispatch a request effect. Must return promptly (URLSession `resume()` / OkHttp
    /// `Call.enqueue` are non-blocking); the completion is delivered later, carrying the request's
    /// `token`.
    fn execute(&self, request: FfiRequest);

    /// Forward a caller cancellation to the in-flight task identified by `token` (rule 9 — the
    /// adapter cancels the `URLSessionTask` / OkHttp `Call`, which completes with a native
    /// cancellation the adapter maps to [`FfiHttpError::Cancelled`]). A no-op if the token is
    /// unknown / already done.
    fn cancel(&self, token: u64);
}

// =====================================================================================
// Shared bridge state + the `Http` shim that fronts the native adapter for the suite.
// =====================================================================================

/// A parked row completion, keyed by token until the native adapter delivers.
struct Pending {
    completion: Box<dyn CompletionSink>,
    /// The row's upload-progress sink, if any (rule 11). The native adapter's body-hand-off
    /// delegate re-enters [`HttpHarness::report_progress`], which forwards to this sink.
    progress: Option<Box<dyn UploadProgressSink>>,
}

/// State shared between the `Http` shim (which needs the native adapter + the token registry) and
/// the harness completion entry points (which need the registry).
struct Shared {
    adapter: Arc<dyn HttpAdapter>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_token: AtomicU64,
}

/// The per-row `Http` implementation the suite drives: a thin shim that forwards to the one
/// registered native adapter (the transport is stateless per request, so every row shares it).
struct NativeAdapter {
    shared: Arc<Shared>,
}

impl Http for NativeAdapter {
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
        // Rust → native: performs the request asynchronously and returns immediately.
        self.shared.adapter.execute(ffi);

        // Bridge the contract's poll-based cancellation to the native task: a detached watcher polls
        // the returned token (the Linux adapter's 10 ms poll, mirrored) and, on cancellation,
        // forwards `adapter.cancel(token)` across the FFI so the URLSessionTask / OkHttp Call is
        // cancelled. The watcher self-terminates when the request completes (its pending entry is
        // removed) so no thread outlives its request.
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

/// Row 18 (CAP, tiered): both native transports expose per-phase timings — Apple via
/// `URLSessionTaskMetrics`, OkHttp via an `EventListener` (DNS/connect/TLS/first-byte) — so the
/// honest tier is [`MetricsTier::Phase`], richer than reqwest's whole-request tier. The C3 column
/// reads this off the trait impl.
impl Metrics for NativeAdapter {
    fn tier(&self) -> MetricsTier {
        MetricsTier::Phase
    }
}

/// The factory the suite reads adapters from. Each `new_adapter()` shares the same `Shared`.
struct NativeFactory {
    shared: Arc<Shared>,
}

impl AdapterFactory for NativeFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(NativeAdapter {
            shared: Arc::clone(&self.shared),
        })
    }

    /// Present at the [`MetricsTier::Phase`] tier (row 18, CAP tiered) — `URLSessionTaskMetrics` /
    /// OkHttp `EventListener`.
    fn metrics(&self) -> Option<Box<dyn Metrics>> {
        Some(Box::new(NativeAdapter {
            shared: Arc::clone(&self.shared),
        }))
    }
}

// =====================================================================================
// The exported harness: construction, server lifecycle, completion re-entry, the driver.
// =====================================================================================

/// The A1 chunk-stream ring capacity — chosen well above any probe's chunk count so the SPSC ring
/// never drops even when the consumer lags the burst producer (drop would be a false loss).
const CHUNK_STREAM_CAPACITY: usize = 1024;

/// The Rust half of the bridge the native side drives. Constructed with the native adapter; owns the
/// shared registry and (once started) the in-process test server.
pub struct HttpHarness {
    shared: Arc<Shared>,
    server: Mutex<Option<(TestServer, Endpoints)>>,
    /// A1 streaming probe (step 25): the `ffi_stream` a live native consumer drains. Chunks pushed by
    /// [`HttpHarness::deliver_chunk`] are re-delivered here off the producer thread (F1 async push).
    /// Capacity generously exceeds any probe's chunk count so the SPSC ring never drops (a drop
    /// would be a *false* loss; the probe measures real completeness, not ring pressure).
    chunk_stream: Arc<EventSubscription<Chunk>>,
    /// How many chunks entered the core through [`HttpHarness::deliver_chunk`] (the completeness
    /// numerator source-of-truth, independent of what the consumer received).
    chunk_ingested: AtomicU64,
}

#[export]
impl HttpHarness {
    /// Build the harness over the registered native adapter (the composition-root dance: adapter
    /// first, harness second, then the native side sets its weak back-reference to this harness).
    pub fn new(adapter: Arc<dyn HttpAdapter>) -> Self {
        HttpHarness {
            shared: Arc::new(Shared {
                adapter,
                pending: Mutex::new(HashMap::new()),
                next_token: AtomicU64::new(1),
            }),
            server: Mutex::new(None),
            chunk_stream: Arc::new(EventSubscription::new(CHUNK_STREAM_CAPACITY)),
            chunk_ingested: AtomicU64::new(0),
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

    /// Upload-progress re-entry (rule 11): forward the native adapter's body-hand-off figures to
    /// the parked [`UploadProgressSink`] **without** removing the pending entry (progress is
    /// repeatable; only a completion consumes the entry). `total` is `None` when the body length is
    /// not known up front (Apple `NSURLSessionTransferSizeUnknown` / an OkHttp -1 content length).
    pub fn report_progress(&self, token: u64, sent: u64, total: Option<u64>) {
        if let Ok(pending) = self.shared.pending.lock()
            && let Some(entry) = pending.get(&token)
            && let Some(sink) = entry.progress.as_ref()
        {
            sink.progress(sent, total);
        }
    }

    /// Run the eleven C1 conformance rows against the registered native adapter (structured results).
    /// Requires a started server; without one, reports the missing-server state rather than panicking.
    pub fn run_c1(&self) -> Vec<RowReport> {
        self.run_rows(c1::rows())
    }

    /// Run the C2 error-taxonomy rows (one positive control per reachable key) against the adapter.
    pub fn run_c2(&self) -> Vec<RowReport> {
        self.run_rows(c2::rows())
    }

    /// Run the C1-adjacent extra rows (row-15 response-sink correspondence, the redirect hop trace)
    /// against the adapter — the M2 syntheses (file sink + hop trace) they exercise (structured
    /// results). Requires a started server.
    pub fn run_extra_rows(&self) -> Vec<RowReport> {
        self.run_rows(c1::extra_rows())
    }

    /// The C3 divergence table for this adapter, rendered from the capability traits (row 18
    /// `metrics present (Phase)`). No server needed — it reads the factory's type-checked capability
    /// self-report.
    pub fn run_c3(&self) -> String {
        let factory = NativeFactory {
            shared: Arc::clone(&self.shared),
        };
        c3::divergence(&factory).render()
    }

    // -- A1 streaming probe (step 25, probe-grade). ---------------------------------------------
    // A streamed response through the step-24 S-FFI-chosen mechanism (F1 `ffi_stream` async push)
    // inside a real http round-trip: the native streaming consumer reads the test server's
    // `/chunked` endpoint and pushes each chunk here via `deliver_chunk`; a live native consumer
    // drains `chunk_stream` and proves ordered/lossless/complete delivery. NO contract surface is
    // added — the streaming core seam is deliberately unfrozen (freeze Q2).

    /// A1 deliver: a response-body chunk crossing the FFI from the native streaming consumer re-enters
    /// here and is pushed out to the live consumer through the `ffi_stream` (F1). Increments the
    /// ingest counter (the completeness numerator). The push cannot drop: the ring capacity exceeds
    /// any probe's chunk count.
    pub fn deliver_chunk(&self, chunk: Chunk) {
        self.chunk_ingested.fetch_add(1, Ordering::Relaxed);
        self.chunk_stream.push_event(chunk);
    }

    /// A1: how many chunks entered the core through [`HttpHarness::deliver_chunk`] — the ingest
    /// source-of-truth. Equal to the chunk count when the http round-trip + cross-FFI ingest are
    /// whole; the consumer's received count is the *separate* re-delivery-completeness measure.
    pub fn chunk_ingested(&self) -> u64 {
        self.chunk_ingested.load(Ordering::Relaxed)
    }

    /// A1: the live response-stream the native consumer drains (an `AsyncStream<Chunk>` on Apple, a
    /// `Flow<Chunk>` on Android; F1 — `ffi_stream` async push). Its built-in async hop means the
    /// consumer resumes OFF the producer (adapter) thread — the F1 re-entrancy rationale the step-24
    /// verdict rests on.
    #[ffi_stream(item = Chunk)]
    pub fn chunk_stream(&self) -> Arc<EventSubscription<Chunk>> {
        Arc::clone(&self.chunk_stream)
    }

    /// A1: close the chunk stream so its live consumer terminates promptly (the `AsyncStream` / `Flow`
    /// ends, the consumer task finishes). The probe calls this after each run so a completed — or,
    /// under load, a still-draining — consumer does not linger as a dead subscription in the shared
    /// `ffi_stream` runtime and starve the next run's consumer. Idempotent.
    pub fn close_chunk_stream(&self) {
        self.chunk_stream.unsubscribe();
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

    /// Drive `rows` against the registered native adapter over the started server, projecting each
    /// [`RowResult`] onto a structured [`RowReport`]. Shared by [`HttpHarness::run_c1`] / `run_c2`.
    fn run_rows(&self, rows: &[ConformanceRow]) -> Vec<RowReport> {
        let guard = match self.server.lock() {
            Ok(g) => g,
            Err(_) => return vec![no_server_report()],
        };
        let Some((_server, endpoints)) = guard.as_ref() else {
            return vec![no_server_report()];
        };
        let factory = NativeFactory {
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
    // The request's SPKI pins (empty when unpinned) — the adapter enforces them in its trust
    // delegate (rule 10, M2).
    let pins = request
        .pins()
        .map(|set| {
            set.pins()
                .iter()
                .map(|p| FfiPin {
                    hash: p.as_bytes().to_vec(),
                })
                .collect()
        })
        .unwrap_or_default();
    // The response-sink selector (row 15, M2). `ResponseSink` is `#[non_exhaustive]`; anything that
    // is not `File` (Memory today, a future streaming sink) mirrors as `Memory`.
    let sink = match request.response_sink() {
        ResponseSink::File(file) => FfiResponseSink::File {
            path: file.as_path().to_string_lossy().into_owned(),
        },
        _ => FfiResponseSink::Memory,
    };
    // The priority hint (row 12; uniform advisory). Absent ⇒ `Normal` — the hint data rides every
    // request.
    let priority = match request.priority() {
        Some(Priority::Throttled) => FfiPriority::Throttled,
        Some(Priority::Low) => FfiPriority::Low,
        Some(Priority::Normal) | None => FfiPriority::Normal,
        Some(Priority::High) => FfiPriority::High,
        Some(Priority::Critical) => FfiPriority::Critical,
    };
    FfiRequest {
        token,
        method: method_str(request.method()).to_owned(),
        url: request.url().as_str().to_owned(),
        headers,
        body,
        deadline_ms: u64::try_from(request.deadline().as_millis()).unwrap_or(u64::MAX),
        pins,
        sink,
        priority,
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
    // Row 15 sink correspondence (M2): a non-empty `sink_path` is a `File` outcome at that path (the
    // body was written there, not buffered) — `content_length` is not meaningful for a file sink, so
    // it is `None`. A `Memory` sink reports the decoded in-memory length, honest for a buffered body
    // (`Some(n)` promises `n` decoded bytes) — satisfying rule 7 without the compressed figure
    // (§5.12). The version is the adapter's real native transport observable (row 11).
    let (body, content_length) = if response.sink_path.is_empty() {
        (
            BodyOutcome::Memory(response.body.clone()),
            Some(response.body.len() as u64),
        )
    } else {
        (
            BodyOutcome::File(FileRef::new(response.sink_path.clone())),
            None,
        )
    };
    let mut built = HttpResponse::builder(
        StatusCode::new(response.status),
        url,
        to_http_version(response.http_version),
        body,
    )
    .headers(headers)
    .content_length(content_length);
    // Redirect hop trace (row 7, M2): each intermediate URL, in traversal order. A hop that fails to
    // re-type is dropped rather than faulting the whole completion (every real hop parses).
    for hop in &response.hops {
        if let Some(hop_url) = parse_final_url(hop) {
            built = built.hop(hop_url);
        }
    }
    Ok(built.build())
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
        FfiHttpError::PinMismatch => HttpError::PinMismatch,
        // The cleartext target always re-types; a malformed one falls back to `Transport` rather
        // than an `unwrap` (no row inspects `to`, only the key).
        FfiHttpError::InsecureRedirect { to } => Url::cleartext_dev(to)
            .map(|to| HttpError::InsecureRedirect { to })
            .unwrap_or(HttpError::Transport),
        FfiHttpError::Io => HttpError::Io,
        FfiHttpError::PermissionDenied => HttpError::PermissionDenied,
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
