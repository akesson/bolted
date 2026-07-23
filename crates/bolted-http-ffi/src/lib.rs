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

use boltffi::*;

use bolted_http::capability::{
    CancelToken, ChunkSink, CompletionSink, Http, Metrics, MetricsTier, RequestHandle,
    StreamingHttp, UploadProgressSink,
};
use bolted_http::conformance::server::TestServer;
use bolted_http::conformance::{
    AdapterFactory, ConformanceCtx, ConformanceRow, Endpoints, FailureReason, RowResult, c1, c2,
    c3, run, stream,
};
use bolted_http::request::{FileRef, HttpRequest, Method, Priority, RequestBody, ResponseSink};
use bolted_http::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use bolted_http::signal::{FlowObserver, FlowSignal, FlowSignals};
use bolted_http::stream::{BodyChunk, BodyEnd};
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

/// One response-body chunk crossing the FFI on the **streaming seam** (streaming-seam §3a, ruled Q1;
/// step-27 M3 — the graduation of the step-25 A1 probe `Chunk` into shipped contract-path code). The
/// native adapter's response-stream delegate (`didReceive data` on Apple, OkHttp source on Android)
/// pushes one of these per transport read into [`HttpHarness::deliver_chunk`]; the core-owned
/// [`bolted_http::stream::BodyStream`] verifies `seq` (ascending, gapless) and rings/gates it.
/// Mirrors [`BodyChunk`].
#[data]
pub struct FfiBodyChunk {
    /// The chunk's sequence number — `0` for the first chunk of a response, `+1` each subsequent
    /// chunk. A hole or repeat is the core's typed `seq` failure (never tolerated).
    pub seq: u64,
    /// The decoded body bytes carried by this chunk.
    pub bytes: Vec<u8>,
}

/// The terminal that ends a streamed response body on the seam (streaming-seam §3c) — a **separate**
/// re-entry from chunk delivery ([`HttpHarness::finish_body`]), not a `last` flag. Mirrors
/// [`BodyEnd`]: `Complete { total }` closes the completeness gate (`total` must equal the ingested
/// bytes, else a truncation is surfaced as a typed failure), `Failed { error }` carries a mid-body
/// transport failure as data.
#[data]
#[derive(Clone)]
pub enum FfiBodyEnd {
    /// The body completed; `total` is the adapter's declared decoded byte count (gated against the
    /// bytes actually ingested).
    Complete { total: u64 },
    /// The body failed mid-stream, with a typed reason.
    Failed { error: FfiHttpError },
}

/// One core→adapter mid-flight signal crossing the FFI (streaming-seam §3b / Q4). The single shape,
/// three uses, mirroring [`FlowSignal`]: back-pressure (`Pause`/`Resume` — the adapter pauses/resumes
/// its socket read so the core ring never overflows) and the **pushed** cancel (`Cancel`, which
/// replaces the deleted 10 ms poll-watcher thread). Delivered to the native adapter through
/// [`HttpAdapter::signal`]; Apple maps them to `URLSessionTask.suspend/resume/cancel`.
#[data]
#[derive(Clone, Copy)]
pub enum FfiFlowSignal {
    /// Stop delivering body chunks (back-pressure): the core ring is near capacity.
    Pause,
    /// Resume delivering body chunks — back-pressure relieved.
    Resume,
    /// Cancel the in-flight request (the pushed cancel that replaces poll-watching).
    Cancel,
}

// =====================================================================================
// The callback trait the native side implements.
// =====================================================================================

/// The HTTP capability as it crosses the FFI: the native URLSession / OkHttp adapter implements
/// `execute`. It performs the request out-of-process (asynchronously) and delivers the completion
/// back through [`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`].
#[export]
pub trait HttpAdapter: Send + Sync {
    /// Dispatch a **buffered** request effect. Must return promptly (URLSession `resume()` / OkHttp
    /// `Call.enqueue` are non-blocking); the completion is delivered later, carrying the request's
    /// `token`, through [`HttpHarness::complete_ok`] / [`HttpHarness::complete_err`].
    fn execute(&self, request: FfiRequest);

    /// Dispatch a **streaming** request effect (streaming-seam §3a, step-27 M3). Must return
    /// promptly; each response-body chunk is pushed later through [`HttpHarness::deliver_chunk`] and
    /// the single terminal through [`HttpHarness::finish_body`], both carrying `token`. Apple's
    /// URLSession `didReceive data` delegate is the per-chunk push; `didCompleteWithError` is the
    /// terminal.
    fn execute_streaming(&self, request: FfiRequest);

    /// Push a mid-flight [`FfiFlowSignal`] to the in-flight task identified by `token` (streaming-seam
    /// §3b / Q4 — the one signal shape, three uses). `Cancel` cancels the task (rule 9, replacing the
    /// deleted poll-watcher); `Pause`/`Resume` pace the socket read for back-pressure. A no-op if the
    /// token is unknown / already done.
    fn signal(&self, token: u64, flow: FfiFlowSignal);
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

/// State shared between the `Http`/`StreamingHttp` shim (which needs the native adapter + the token
/// registries) and the harness completion / chunk re-entry points (which need them).
struct Shared {
    adapter: Arc<dyn HttpAdapter>,
    pending: Mutex<HashMap<u64, Pending>>,
    /// Parked streaming sinks, keyed by token (streaming-seam §3d): one live driver-owned
    /// [`ChunkSink`] per in-flight streamed response. A chunk re-entry looks it up and delivers; the
    /// terminal (or a typed delivery failure) **removes and consumes** it — the driver-owned
    /// deterministic close. A stream whose terminal never arrives leaves its entry parked: that is
    /// the F-M3-1 leak, and row 14's live-count is exactly `pending_streams.len()`.
    pending_streams: Mutex<HashMap<u64, Box<dyn ChunkSink>>>,
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

        // Cancellation is **pushed** now (step-27 M3 — the 10 ms poll-watcher thread is deleted):
        // `RequestHandle::cancel` fires `FlowSignal::Cancel`, whose observer forwards
        // `adapter.signal(token, Cancel)` across the FFI so the URLSessionTask / OkHttp Call is
        // cancelled. No thread outlives the request; the completion still arrives (as `Cancelled`).
        let observer = Arc::new(NativeFlowObserver {
            adapter: Arc::clone(&self.shared.adapter),
            token,
        });
        RequestHandle::with_signals(CancelToken::new(), FlowSignals::new(observer))
    }
}

impl StreamingHttp for NativeAdapter {
    /// Dispatch a streaming request across the FFI (streaming-seam §3a/§3b). Parks the driver-owned
    /// [`ChunkSink`] under a fresh token, then asks the native adapter to `execute_streaming`; each
    /// pushed chunk re-enters through [`HttpHarness::deliver_chunk`], the terminal through
    /// [`HttpHarness::finish_body`]. Returns the [`FlowSignals`] the driver uses to push back-pressure
    /// (pause/resume) and cancel — routed to the native task through [`HttpAdapter::signal`].
    fn send_streaming(&self, request: HttpRequest, chunks: Box<dyn ChunkSink>) -> FlowSignals {
        let token = self.shared.next_token.fetch_add(1, Ordering::Relaxed);
        let ffi = to_ffi_request(token, &request);
        if let Ok(mut streams) = self.shared.pending_streams.lock() {
            streams.insert(token, chunks);
        }
        let observer = Arc::new(NativeFlowObserver {
            adapter: Arc::clone(&self.shared.adapter),
            token,
        });
        // Start the native stream after parking the sink, so a chunk that races back finds its sink.
        self.shared.adapter.execute_streaming(ffi);
        FlowSignals::new(observer)
    }
}

/// The adapter-side reaction to the core→adapter [`FlowSignals`] surface (streaming-seam §3b / Q4):
/// each pushed [`FlowSignal`] is forwarded across the FFI to the native task via
/// [`HttpAdapter::signal`]. This is what lets the Apple/Android adapters delete their poll-watcher —
/// cancel is pushed, not polled — and honour back-pressure (pause/resume) mid-stream.
struct NativeFlowObserver {
    adapter: Arc<dyn HttpAdapter>,
    token: u64,
}

impl FlowObserver for NativeFlowObserver {
    fn on_signal(&self, signal: FlowSignal) {
        let flow = match signal {
            FlowSignal::Pause => FfiFlowSignal::Pause,
            FlowSignal::Resume => FfiFlowSignal::Resume,
            FlowSignal::Cancel => FfiFlowSignal::Cancel,
            // `FlowSignal` is `#[non_exhaustive]`; a future signal this bridge does not model is a
            // no-op rather than a spurious native call.
            _ => return,
        };
        self.adapter.signal(self.token, flow);
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

    /// Streaming is present (streaming-seam §3b, step-27 M3): the native adapter pushes response-body
    /// chunks through [`HttpHarness::deliver_chunk`] / [`HttpHarness::finish_body`]. Returning `Some`
    /// only type-checks because [`NativeAdapter`] really implements [`StreamingHttp`] — the streaming
    /// rows (12/13) run against the real adapter rather than skipping.
    fn streaming(&self) -> Option<Box<dyn StreamingHttp>> {
        Some(Box::new(NativeAdapter {
            shared: Arc::clone(&self.shared),
        }))
    }
}

// =====================================================================================
// The exported harness: construction, server lifecycle, completion re-entry, the driver.
// =====================================================================================

/// The Rust half of the bridge the native side drives. Constructed with the native adapter; owns the
/// shared registries and (once started) the in-process test server.
pub struct HttpHarness {
    shared: Arc<Shared>,
    server: Mutex<Option<(TestServer, Endpoints)>>,
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
                pending_streams: Mutex::new(HashMap::new()),
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

    /// Run the streaming rows (12 — slow-consumer completeness; 13 — terminal-exactly-once) against
    /// the registered native adapter over the started server (streaming-seam §3b/§3c, step-27 M3).
    /// Drives `/chunked` through the real streaming path (`send_streaming` → native
    /// `execute_streaming` → pushed chunks → the driver-owned completeness gate). Requires a started
    /// server.
    pub fn run_stream_rows(&self) -> Vec<RowReport> {
        self.run_rows(stream::rows())
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

    // -- Streaming seam re-entry (streaming-seam §3a/§3c, step-27 M3). --------------------------
    // The step-25 A1 probe (`ffi_stream` async push, probe-grade) graduates here into the shipped
    // contract path: the native adapter pushes each response-body chunk synchronously through
    // `deliver_chunk` into the driver-owned `ChunkSink` (the core `BodyStream` behind the harness
    // driver), and the single terminal through `finish_body`. There is no `ffi_stream` and no live
    // native consumer to abandon — the driver owns the ingest and closes it deterministically at the
    // terminal (streaming-seam §3d), so the F-M3-1 subscription leak reduces to "a stream whose
    // terminal never arrives leaves a parked entry", which row 14 detects via `live_streams`.

    /// Streaming deliver (streaming-seam §3a): a response-body chunk crossing the FFI from the native
    /// adapter re-enters here and is delivered into the parked driver-owned [`ChunkSink`] for `token`.
    /// Returns `true` to keep reading; `false` when the core raised a typed failure (a `seq` violation
    /// or ring overflow) — in which case the harness has already **closed** the stream with that
    /// failure (removing and consuming the sink), and the adapter must stop reading and cancel. A
    /// `false` is also returned for an unknown / already-closed `token`.
    pub fn deliver_chunk(&self, token: u64, chunk: FfiBodyChunk) -> bool {
        let bc = BodyChunk::new(chunk.seq, chunk.bytes);
        let delivered = match self.shared.pending_streams.lock() {
            Ok(streams) => streams.get(&token).map(|sink| sink.deliver_chunk(bc)),
            Err(_) => None,
        };
        match delivered {
            Some(Ok(())) => true,
            Some(Err(err)) => {
                // The core ingest raised a typed failure — close the stream with it (driver-owned
                // deterministic close) and tell the adapter to stop.
                if let Ok(mut streams) = self.shared.pending_streams.lock()
                    && let Some(sink) = streams.remove(&token)
                {
                    sink.finish(BodyEnd::Failed(err));
                }
                false
            }
            None => false,
        }
    }

    /// Streaming terminal (streaming-seam §3c): close the streamed response for `token` with its
    /// terminal. **Removes and consumes** the parked [`ChunkSink`] (one terminal by construction; the
    /// deterministic close that keeps `live_streams` honest). A `Complete { total }` runs the
    /// completeness gate (`total == ingested`, else a typed truncation failure); a `Failed { error }`
    /// carries a mid-body failure. A no-op for an unknown / already-closed `token`.
    pub fn finish_body(&self, token: u64, end: FfiBodyEnd) {
        let sink = self
            .shared
            .pending_streams
            .lock()
            .ok()
            .and_then(|mut streams| streams.remove(&token));
        if let Some(sink) = sink {
            sink.finish(to_body_end(&end));
        }
    }

    /// The number of **live** streamed-response subscriptions (streaming-seam §3d): parked
    /// [`ChunkSink`]s whose terminal has not yet arrived. The subscription-hygiene observable (row
    /// 14): after N conformant streamed responses it returns to baseline (0), because each terminal
    /// removes its entry; a stream whose terminal never arrives (the F-M3-1 leak) leaves its entry
    /// parked, so this stays above baseline — the row's real red case.
    pub fn live_streams(&self) -> u64 {
        self.shared
            .pending_streams
            .lock()
            .map(|s| s.len() as u64)
            .unwrap_or(0)
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
        // The verified, adapter-counted byte total (Q3): the native side wrote the file, so the
        // counted truth is the file's size on disk — distinct from the advisory (`None` here)
        // header value.
        let bytes_written = std::fs::metadata(&response.sink_path)
            .map(|m| m.len())
            .unwrap_or(0);
        (
            BodyOutcome::File {
                path: FileRef::new(response.sink_path.clone()),
                bytes_written,
            },
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

/// Map the FFI streaming terminal to the contract [`BodyEnd`] (streaming-seam §3c).
fn to_body_end(end: &FfiBodyEnd) -> BodyEnd {
    match end {
        FfiBodyEnd::Complete { total } => BodyEnd::Complete { total: *total },
        FfiBodyEnd::Failed { error } => BodyEnd::Failed(to_http_error(error)),
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

// =====================================================================================
// Host-side bridge tests (step-27 M5). The streaming-seam bridge logic — `finish_body`'s
// remove-then-consume, `deliver_chunk`'s close-on-error, and `NativeFlowObserver`'s
// signal forwarding — is otherwise watched ONLY by the Apple/Android platform tiers. These
// drive the bridge on the host with a fake `HttpAdapter` (which echoes the streaming token and
// records forwarded signals) and a recording `ChunkSink`, so a defect in that logic goes red on
// the host suite (`mise run test`) instead of surviving until a device tier runs.
#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::*;
    use bolted_http::request::{HttpRequest, Method};

    /// A fake native adapter: it never performs a request, but records the streaming token handed to
    /// `execute_streaming` (so a test can drive the harness re-entry points for that token) and every
    /// mid-flight signal forwarded through `signal` (so signal-routing is observable).
    #[derive(Default)]
    struct FakeAdapter {
        last_stream_token: AtomicU64,
        signals: Mutex<Vec<(u64, FfiFlowSignal)>>,
    }

    impl HttpAdapter for FakeAdapter {
        fn execute(&self, _request: FfiRequest) {}
        fn execute_streaming(&self, request: FfiRequest) {
            self.last_stream_token
                .store(request.token, Ordering::SeqCst);
        }
        fn signal(&self, token: u64, flow: FfiFlowSignal) {
            self.signals
                .lock()
                .expect("signals lock")
                .push((token, flow));
        }
    }

    /// A driver-owned `ChunkSink` that records what the bridge forwards to it: the delivered `seq`s
    /// and the single terminal. `err_on_seq` lets a test drive `deliver_chunk`'s error path (the core
    /// ingest would raise a typed failure on a bad `seq`/overflow; here the sentinel stands in for it).
    struct RecordingSink {
        delivered: Arc<Mutex<Vec<u64>>>,
        terminal: Arc<Mutex<Option<BodyEnd>>>,
        err_on_seq: Option<u64>,
    }

    impl ChunkSink for RecordingSink {
        fn deliver_chunk(&self, chunk: BodyChunk) -> Result<(), HttpError> {
            if self.err_on_seq == Some(chunk.seq) {
                return Err(HttpError::Transport);
            }
            self.delivered
                .lock()
                .expect("delivered lock")
                .push(chunk.seq);
            Ok(())
        }
        fn finish(self: Box<Self>, end: BodyEnd) {
            *self.terminal.lock().expect("terminal lock") = Some(end);
        }
    }

    /// Park a streaming sink through the real bridge (`NativeAdapter::send_streaming`) and return the
    /// token the fake adapter observed, the recording handles, and the `FlowSignals` emitter.
    #[allow(clippy::type_complexity)]
    fn park_stream(
        harness: &HttpHarness,
        fake: &FakeAdapter,
        err_on_seq: Option<u64>,
    ) -> (
        u64,
        Arc<Mutex<Vec<u64>>>,
        Arc<Mutex<Option<BodyEnd>>>,
        FlowSignals,
    ) {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let terminal = Arc::new(Mutex::new(None));
        let sink = Box::new(RecordingSink {
            delivered: Arc::clone(&delivered),
            terminal: Arc::clone(&terminal),
            err_on_seq,
        });
        let factory = NativeFactory {
            shared: Arc::clone(&harness.shared),
        };
        let streaming = factory.streaming().expect("streaming present");
        let url = Url::cleartext_dev("http://127.0.0.1/chunked").expect("url");
        let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
        let signals = streaming.send_streaming(req, sink);
        let token = fake.last_stream_token.load(Ordering::SeqCst);
        (token, delivered, terminal, signals)
    }

    #[test]
    fn finish_body_removes_and_consumes_the_parked_sink() {
        let fake = Arc::new(FakeAdapter::default());
        let harness = HttpHarness::new(fake.clone());
        let (token, delivered, terminal, _signals) = park_stream(&harness, &fake, None);

        // Parked: one live subscription.
        assert_eq!(harness.live_streams(), 1);
        assert!(harness.deliver_chunk(
            token,
            FfiBodyChunk {
                seq: 0,
                bytes: b"ab".to_vec(),
            }
        ));
        assert_eq!(delivered.lock().expect("lock").as_slice(), &[0]);

        // The terminal MUST both fire on the sink (consume) and clear the registry (remove).
        harness.finish_body(token, FfiBodyEnd::Complete { total: 2 });
        assert!(
            matches!(&*terminal.lock().expect("lock"), Some(BodyEnd::Complete { total }) if *total == 2),
            "finish_body must consume the sink with its terminal"
        );
        assert_eq!(
            harness.live_streams(),
            0,
            "finish_body must remove the parked sink (subscription hygiene)"
        );
    }

    #[test]
    fn deliver_chunk_closes_the_stream_on_a_typed_failure() {
        let fake = Arc::new(FakeAdapter::default());
        let harness = HttpHarness::new(fake.clone());
        // The sink raises a typed failure on seq 0 (standing in for a seq violation / ring overflow).
        let (token, _delivered, terminal, _signals) = park_stream(&harness, &fake, Some(0));

        assert_eq!(harness.live_streams(), 1);
        // A typed failure must make the bridge stop reading (`false`) AND close the stream.
        let keep_reading = harness.deliver_chunk(
            token,
            FfiBodyChunk {
                seq: 0,
                bytes: Vec::new(),
            },
        );
        assert!(
            !keep_reading,
            "a typed failure must tell the adapter to stop"
        );
        assert!(
            matches!(&*terminal.lock().expect("lock"), Some(BodyEnd::Failed(_))),
            "deliver_chunk must close the stream with the failure terminal"
        );
        assert_eq!(
            harness.live_streams(),
            0,
            "deliver_chunk must remove the parked sink after closing on error"
        );
    }

    #[test]
    fn flow_signals_are_forwarded_to_the_native_task() {
        let fake = Arc::new(FakeAdapter::default());
        let harness = HttpHarness::new(fake.clone());
        let (token, _delivered, _terminal, signals) = park_stream(&harness, &fake, None);

        signals.pause();
        signals.resume();
        signals.cancel();

        let seen = fake.signals.lock().expect("lock");
        let for_token: Vec<&FfiFlowSignal> = seen
            .iter()
            .filter(|(t, _)| *t == token)
            .map(|(_, f)| f)
            .collect();
        assert!(
            for_token.iter().any(|f| matches!(f, FfiFlowSignal::Pause)),
            "Pause must be forwarded across the FFI"
        );
        assert!(
            for_token.iter().any(|f| matches!(f, FfiFlowSignal::Resume)),
            "Resume must be forwarded across the FFI"
        );
        assert!(
            for_token.iter().any(|f| matches!(f, FfiFlowSignal::Cancel)),
            "Cancel must be forwarded across the FFI (the pushed cancel that replaced poll-watching)"
        );
    }
}
