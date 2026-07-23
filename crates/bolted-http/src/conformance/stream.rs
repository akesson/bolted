//! Streaming conformance (feature `conformance`): the response-body streaming seam driven end to
//! end through an [`StreamingHttp`] adapter, plus the two new §7 rows it enables.
//!
//! - **Row 12 — slow-consumer completeness** (rule 12): a consumer that drains *slowly* still
//!   receives the complete body, and the terminal total is verified. A conformant adapter honours
//!   the pushed [`crate::FlowSignal::Pause`] so the bounded ring never overflows; a broken adapter
//!   that drops a chunk under back-pressure fails, because the completeness gate turns the declared
//!   total that disagrees with the ingested bytes into a typed failure.
//! - **Row 13 — terminal-exactly-once** (rule 13): after the chunks, exactly one terminal arrives.
//!   *Double*-terminal is impossible **by construction** ([`crate::ChunkSink::finish`] consumes the
//!   sink — a compile error, proven in [`crate::stream`]); the reachable red is the *missing*
//!   terminal, which this row's `SkipTerminal` twin exercises.
//!
//! ## The driver lives here, sans-io stays over there
//!
//! The contract crate is lock-free (kill criterion 2): the core-owned [`BodyStream`] is a plain
//! `&mut`-driven value. The **driver** (here, the harness; in production, the store/shell pair) owns
//! the synchronisation. [`DriverStream`] is that driver: it wraps the `BodyStream` behind a `Mutex`,
//! runs a *slow consumer* that drains and pushes pause/resume through the [`FlowSignals`] surface,
//! and records the terminal. All of this is harness code — none of it is in `bolted-http`'s default
//! build.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use super::{Cluster, ConformanceCtx, ConformanceRow, FailureReason, RowResult, SkipReason};
use crate::capability::{
    CancelToken, ChunkSink, CompletionSink, Http, RequestHandle, StreamingHttp, UploadProgressSink,
};
use crate::error::HttpError;
use crate::request::{HttpRequest, Url};
use crate::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};
use crate::signal::{FlowObserver, FlowSignal, FlowSignals};
use crate::stream::{BodyChunk, BodyEnd, BodyStream};

use super::AdapterFactory;

/// Back-pressure watermarks for the slow consumer, expressed against the core-owned ring capacity
/// (never a copied literal). Pause when the ring reaches [`HIGH`]; resume once drained to [`LOW`].
/// The slack from [`HIGH`] to [`BodyStream::RING_CAPACITY`] is what keeps a *conformant* producer
/// from overflowing in the window between crossing the mark and observing the pushed signal.
const HIGH: usize = BodyStream::RING_CAPACITY * 3 / 4;
const LOW: usize = BodyStream::RING_CAPACITY / 4;

/// The chunk count the streaming rows drive — deliberately **above** [`BodyStream::RING_CAPACITY`],
/// so a slow consumer *must* exert back-pressure (the mock delivers this many discrete chunks; a
/// real adapter coalesces transport reads, so its ring may never fill — completeness is asserted on
/// both, back-pressure stress on the mock).
const ROW_CHUNKS: u64 = BodyStream::RING_CAPACITY as u64 + 128;

/// The consumer's per-tick sleep (what makes it *slow*) and the tick budget (a bounded loop rather
/// than a wall clock — `Instant::now` is disallowed workspace-wide). `TICKS * TICK` ≈ 3 s.
const TICK: Duration = Duration::from_millis(1);
const TICKS: u32 = 3000;

// --- The driver-side streaming ingest --------------------------------------------------------

/// The harness's driver-side ingest of one streamed response: the core [`BodyStream`] behind a
/// `Mutex` (the sync lives here, never in the contract crate), the bytes the slow consumer has
/// drained (in order), the recorded terminal, and the [`FlowSignals`] emitter for back-pressure.
struct DriverStream {
    /// The core ingest — `Some` until the terminal consumes it.
    ingest: Mutex<Option<BodyStream>>,
    /// Every decoded byte the consumer (or the terminal's final drain) has taken, in order.
    drained: Mutex<Vec<u8>>,
    /// The terminal outcome ([`BodyStream::finish`]'s result), once it has fired.
    terminal: Mutex<Option<Result<u64, HttpError>>>,
    /// The back-pressure emitter, set after `send_streaming` returns it (the adapter builds it).
    signals: Mutex<Option<FlowSignals>>,
    /// How many chunks were accepted into the ring (diagnostics).
    delivered_chunks: AtomicU64,
}

impl DriverStream {
    fn new() -> Self {
        DriverStream {
            ingest: Mutex::new(Some(BodyStream::new())),
            drained: Mutex::new(Vec::new()),
            terminal: Mutex::new(None),
            signals: Mutex::new(None),
            delivered_chunks: AtomicU64::new(0),
        }
    }

    fn set_signals(&self, signals: FlowSignals) {
        if let Ok(mut g) = self.signals.lock() {
            *g = Some(signals);
        }
    }

    fn push(&self, signal: FlowSignal) {
        let signals = self.signals.lock().ok().and_then(|g| g.clone());
        if let Some(s) = signals {
            match signal {
                FlowSignal::Pause => s.pause(),
                FlowSignal::Resume => s.resume(),
                FlowSignal::Cancel => s.cancel(),
            }
        }
    }

    /// Adapter → driver: deliver one chunk into the ring. On success, push [`FlowSignal::Pause`] when
    /// the ring has reached the high-water mark (back-pressure). The typed failure (seq/overflow)
    /// propagates back so the adapter can close the stream with it.
    fn deliver(&self, chunk: BodyChunk) -> Result<(), HttpError> {
        let buffered = {
            let mut guard = self.ingest.lock().map_err(|_| HttpError::Transport)?;
            match guard.as_mut() {
                Some(stream) => {
                    stream.deliver_chunk(chunk)?;
                    stream.buffered()
                }
                // The stream was already finished — a chunk after the terminal is a driver error,
                // never a silent accept.
                None => return Err(HttpError::Transport),
            }
        };
        self.delivered_chunks.fetch_add(1, Ordering::SeqCst);
        if buffered >= HIGH {
            self.push(FlowSignal::Pause);
        }
        Ok(())
    }

    /// The slow consumer's drain step: take every buffered chunk into `drained` (holding the ingest
    /// lock across the append so ordering can never race the terminal's final drain), then push
    /// [`FlowSignal::Resume`] once drained to the low-water mark.
    fn drain_step(&self) {
        let buffered = {
            let Ok(mut ig) = self.ingest.lock() else {
                return;
            };
            let Some(stream) = ig.as_mut() else {
                return;
            };
            let chunks = stream.drain();
            if !chunks.is_empty()
                && let Ok(mut d) = self.drained.lock()
            {
                for c in chunks {
                    d.extend_from_slice(&c.bytes);
                }
            }
            stream.buffered()
        };
        if buffered <= LOW {
            self.push(FlowSignal::Resume);
        }
    }

    /// Adapter → driver: close the stream with its terminal. Drains any still-buffered chunks into
    /// `drained` **before** consuming the ingest (so the consumer never loses the tail), then runs
    /// the completeness gate and records the outcome.
    fn finish(&self, end: BodyEnd) {
        let outcome = {
            let Ok(mut ig) = self.ingest.lock() else {
                return;
            };
            let Some(mut stream) = ig.take() else {
                // A second terminal — impossible via `Box<dyn ChunkSink>` (consumes self), but the
                // driver stays defensive rather than double-recording.
                return;
            };
            let tail = stream.drain();
            if !tail.is_empty()
                && let Ok(mut d) = self.drained.lock()
            {
                for c in tail {
                    d.extend_from_slice(&c.bytes);
                }
            }
            stream.finish(end)
        };
        if let Ok(mut t) = self.terminal.lock() {
            *t = Some(outcome);
        }
    }

    fn has_terminal(&self) -> bool {
        self.terminal.lock().map(|t| t.is_some()).unwrap_or(false)
    }
}

/// The harness's [`ChunkSink`] over a [`DriverStream`] — the adapter-facing seam. Chunk delivery and
/// the one terminal both forward to the shared driver.
struct HarnessChunkSink {
    driver: Arc<DriverStream>,
}

impl ChunkSink for HarnessChunkSink {
    fn deliver_chunk(&self, chunk: BodyChunk) -> Result<(), HttpError> {
        self.driver.deliver(chunk)
    }

    fn finish(self: Box<Self>, end: BodyEnd) {
        self.driver.finish(end);
    }
}

/// What a streaming drive observed: the bytes the slow consumer received, the terminal outcome, and
/// the number of chunks accepted.
pub(crate) struct StreamObservation {
    pub drained: Vec<u8>,
    pub terminal: Option<Result<u64, HttpError>>,
    #[allow(dead_code)]
    pub delivered_chunks: u64,
}

/// Drive one streaming request against `adapter`, running the **slow consumer** on this thread until
/// the terminal arrives (or the tick budget is spent), pushing pause/resume by ring occupancy.
pub(crate) fn drive_streaming(
    adapter: &dyn StreamingHttp,
    request: HttpRequest,
) -> StreamObservation {
    let driver = Arc::new(DriverStream::new());
    let sink = Box::new(HarnessChunkSink {
        driver: driver.clone(),
    });
    let signals = adapter.send_streaming(request, sink);
    driver.set_signals(signals);

    for _ in 0..TICKS {
        driver.drain_step();
        if driver.has_terminal() {
            break;
        }
        thread::sleep(TICK);
    }
    // A final drain in case chunks arrived between the last tick and the terminal.
    driver.drain_step();

    let drained = driver.drained.lock().map(|d| d.clone()).unwrap_or_default();
    let terminal = driver.terminal.lock().ok().and_then(|t| t.clone());
    StreamObservation {
        drained,
        terminal,
        delivered_chunks: driver.delivered_chunks.load(Ordering::SeqCst),
    }
}

// --- The streaming mock implementor ----------------------------------------------------------

/// A deliberate streaming fault, for the per-implementor red twins (the socket-mock `MockBehavior`
/// pattern, one fault at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFault {
    /// Conformant: deliver every chunk, honour pause, finish with the honest total.
    None,
    /// Drop the final chunk but still declare the full total — the truncation the completeness gate
    /// forbids (row 12's red).
    Truncate,
    /// Deliver every chunk but never send the terminal — the missing-terminal break (row 13's red).
    SkipTerminal,
    /// Ignore [`FlowSignal::Pause`] under the slow consumer — the ring overflows loudly with
    /// [`HttpError::StreamOverflow`] (the back-pressure red; also the `StreamOverflow` control).
    IgnorePause,
}

/// The behaviour of one [`StreamMock`]. `chunk_count` chunks of body text identical to the test
/// server's `/chunked` endpoint (so a row's expected bytes match on the mock *and* on a real adapter
/// driven at the same endpoint).
#[derive(Clone, Copy, Debug)]
pub struct StreamMockBehavior {
    /// How many chunks to synthesise (each `chunk-NNNNNN\n`, 1-based, matching the server).
    pub chunk_count: u64,
    /// The injected fault (if any).
    pub fault: StreamFault,
}

impl StreamMockBehavior {
    /// The conformant behaviour for the streaming rows: `ROW_CHUNKS` chunks, no fault.
    #[must_use]
    pub fn correct() -> Self {
        StreamMockBehavior {
            chunk_count: ROW_CHUNKS,
            fault: StreamFault::None,
        }
    }

    /// The same behaviour with one fault injected.
    #[must_use]
    pub fn with_fault(mut self, fault: StreamFault) -> Self {
        self.fault = fault;
        self
    }
}

/// The `chunk-NNNNNN\n` line the server's `/chunked` endpoint emits for 1-based index `n` (the
/// row's expected body is the concatenation of these).
#[must_use]
pub fn chunk_line(n: u64) -> Vec<u8> {
    format!("chunk-{n:06}\n").into_bytes()
}

/// The full expected decoded body for a `/chunked?count=count` response (server + mock agree).
#[must_use]
pub fn expected_body(count: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for n in 1..=count {
        out.extend_from_slice(&chunk_line(n));
    }
    out
}

/// The adapter's [`FlowObserver`]: a paused flag with a condvar for resume, plus a cancelled flag.
struct StreamMockObserver {
    paused: Mutex<bool>,
    cv: Condvar,
    cancelled: AtomicBool,
}

impl StreamMockObserver {
    fn new() -> Self {
        StreamMockObserver {
            paused: Mutex::new(false),
            cv: Condvar::new(),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Block the producer while paused (woken by a resume/cancel signal).
    fn wait_while_paused(&self) {
        if let Ok(mut p) = self.paused.lock() {
            while *p && !self.cancelled.load(Ordering::SeqCst) {
                p = match self.cv.wait(p) {
                    Ok(p) => p,
                    Err(_) => return,
                };
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl FlowObserver for StreamMockObserver {
    fn on_signal(&self, signal: FlowSignal) {
        match signal {
            FlowSignal::Pause => {
                if let Ok(mut p) = self.paused.lock() {
                    *p = true;
                }
            }
            FlowSignal::Resume => {
                if let Ok(mut p) = self.paused.lock() {
                    *p = false;
                }
                self.cv.notify_all();
            }
            FlowSignal::Cancel => {
                self.cancelled.store(true, Ordering::SeqCst);
                self.cv.notify_all();
            }
        }
    }
}

/// A synthesising streaming adapter — the mock implementor of [`StreamingHttp`]. It produces the
/// same body the test server's `/chunked` endpoint does, one [`BodyChunk`] per line, on a producer
/// thread; the injected [`StreamFault`] is the per-row red twin.
#[derive(Clone)]
pub struct StreamMock {
    behavior: StreamMockBehavior,
}

impl StreamMock {
    /// A streaming mock with `behavior`.
    #[must_use]
    pub fn new(behavior: StreamMockBehavior) -> Self {
        StreamMock { behavior }
    }
}

impl Http for StreamMock {
    fn send(
        &self,
        request: HttpRequest,
        completion: Box<dyn CompletionSink>,
        _upload_progress: Option<Box<dyn UploadProgressSink>>,
    ) -> RequestHandle {
        // The streaming mock has no buffered path the rows use; a trivial 200 keeps it a valid `Http`.
        completion.complete(Ok(HttpResponse::builder(
            StatusCode::OK,
            request.url().clone(),
            HttpVersion::Http1_1,
            BodyOutcome::Memory(Vec::new()),
        )
        .build()));
        RequestHandle::for_token(CancelToken::new())
    }
}

impl StreamingHttp for StreamMock {
    fn send_streaming(&self, _request: HttpRequest, chunks: Box<dyn ChunkSink>) -> FlowSignals {
        let behavior = self.behavior;
        let observer = Arc::new(StreamMockObserver::new());
        let signals = FlowSignals::new(observer.clone());
        thread::spawn(move || run_producer(behavior, chunks, &observer));
        signals
    }
}

/// The mock producer: deliver `chunk_count` chunks (honouring pause unless the fault says otherwise),
/// then the terminal — with the injected fault applied.
fn run_producer(
    behavior: StreamMockBehavior,
    chunks: Box<dyn ChunkSink>,
    observer: &StreamMockObserver,
) {
    let count = behavior.chunk_count;
    // `declared` accumulates the honest byte total the server announced — for `Truncate` it counts
    // the dropped chunk's bytes too, so the declared total exceeds the ingested bytes (the gate then
    // fires). For every other behaviour it equals exactly what was delivered.
    let mut declared: u64 = 0;
    let mut seq: u64 = 0;
    for i in 0..count {
        if behavior.fault != StreamFault::IgnorePause {
            observer.wait_while_paused();
        }
        if observer.is_cancelled() {
            chunks.finish(BodyEnd::Failed(HttpError::Cancelled));
            return;
        }
        let line = chunk_line(i + 1);
        let is_last = i + 1 == count;
        if behavior.fault == StreamFault::Truncate && is_last {
            // Drop the last chunk from delivery but still count it toward the declared total.
            declared += line.len() as u64;
            break;
        }
        match chunks.deliver_chunk(BodyChunk::new(seq, line.clone())) {
            Ok(()) => {
                declared += line.len() as u64;
                seq += 1;
            }
            Err(e) => {
                // Ring overflow (IgnorePause) or a seq fault — report it as the terminal.
                chunks.finish(BodyEnd::Failed(e));
                return;
            }
        }
    }
    if behavior.fault == StreamFault::SkipTerminal {
        return; // never finish — the missing-terminal break.
    }
    chunks.finish(BodyEnd::Complete { total: declared });
}

/// A factory over the streaming mock (a fresh adapter per row).
#[derive(Clone)]
pub struct StreamMockFactory {
    behavior: StreamMockBehavior,
}

impl StreamMockFactory {
    /// A conformant streaming-mock factory.
    #[must_use]
    pub fn correct() -> Self {
        StreamMockFactory {
            behavior: StreamMockBehavior::correct(),
        }
    }

    /// The same factory with one streaming fault (a red twin).
    #[must_use]
    pub fn with_fault(mut self, fault: StreamFault) -> Self {
        self.behavior = self.behavior.with_fault(fault);
        self
    }
}

impl AdapterFactory for StreamMockFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(StreamMock::new(self.behavior))
    }

    fn streaming(&self) -> Option<Box<dyn StreamingHttp>> {
        Some(Box::new(StreamMock::new(self.behavior)))
    }
}

// --- The rows --------------------------------------------------------------------------------

/// The two streaming rows (rules 12 and 13). Run on any factory whose [`AdapterFactory::streaming`]
/// is present; on one without streaming they record a skip (never a vacuous pass).
#[must_use]
pub fn rows() -> &'static [ConformanceRow] {
    &ROWS
}

static ROWS: [ConformanceRow; 2] = [
    row("C1/row-12-slow-consumer-completeness", row_12),
    row("C1/row-13-terminal-exactly-once", row_13),
];

const fn row(id: &'static str, check: fn(&ConformanceCtx) -> RowResult) -> ConformanceRow {
    ConformanceRow {
        id,
        cluster: Cluster::C1Rule,
        check,
    }
}

/// The `/chunked?count=ROW_CHUNKS` URL the streaming rows drive (server + mock agree on the body).
fn chunked_url(ctx: &ConformanceCtx) -> Result<Url, RowResult> {
    Url::cleartext_dev(&ctx.endpoints.http(&format!("/chunked?count={ROW_CHUNKS}")))
        .map_err(|_| RowResult::Fail(FailureReason::NoTerminal))
}

/// Row 12 (rule 12): a slow consumer receives the **complete** body and the terminal total is
/// verified. The completeness gate makes truncation observable: a dropped chunk under back-pressure
/// leaves the declared total > ingested, so the terminal is a typed failure and the row goes red.
fn row_12(ctx: &ConformanceCtx) -> RowResult {
    let Some(adapter) = ctx.factory.streaming() else {
        return RowResult::Skipped(SkipReason::NoContractSurface {
            capability: "streaming",
        });
    };
    let url = match chunked_url(ctx) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req =
        HttpRequest::builder(crate::request::Method::Get, url, Duration::from_secs(5)).build();
    let obs = drive_streaming(adapter.as_ref(), req);
    let expected = expected_body(ROW_CHUNKS);
    judge_completeness(&obs, &expected)
}

/// Judge row 12: the terminal succeeded, the declared total equals the body length, and the slow
/// consumer drained exactly the body (no dropped chunk).
fn judge_completeness(obs: &StreamObservation, expected: &[u8]) -> RowResult {
    let expected_len = expected.len() as u64;
    match &obs.terminal {
        Some(Ok(total)) => {
            if *total != expected_len {
                return RowResult::Fail(FailureReason::WrongTerminalTotal {
                    got: *total,
                    expected: expected_len,
                });
            }
            if obs.drained.as_slice() != expected {
                return RowResult::Fail(FailureReason::IncompleteStream {
                    got: obs.drained.len(),
                    expected: expected.len(),
                });
            }
            RowResult::Pass
        }
        Some(Err(e)) => RowResult::Fail(FailureReason::StreamFailed { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoTerminal),
    }
}

/// Row 13 (rule 13): after the chunks, exactly one terminal arrives. Double-terminal is impossible
/// by construction (the sink consumes on `finish`); the reachable red is the missing terminal.
fn row_13(ctx: &ConformanceCtx) -> RowResult {
    let Some(adapter) = ctx.factory.streaming() else {
        return RowResult::Skipped(SkipReason::NoContractSurface {
            capability: "streaming",
        });
    };
    let url = match chunked_url(ctx) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req =
        HttpRequest::builder(crate::request::Method::Get, url, Duration::from_secs(5)).build();
    let obs = drive_streaming(adapter.as_ref(), req);
    match obs.terminal {
        Some(_) => RowResult::Pass,
        None => RowResult::Fail(FailureReason::NoTerminal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::server::TestServer;
    use crate::conformance::{ConformanceCtx, Endpoints, RowResult};

    fn harness() -> (TestServer, Endpoints) {
        let server = TestServer::start().expect("server starts");
        let endpoints = Endpoints::from_server(&server);
        (server, endpoints)
    }

    fn ctx_of<'a>(f: &'a StreamMockFactory, ep: &'a Endpoints) -> ConformanceCtx<'a> {
        ConformanceCtx {
            factory: f,
            endpoints: ep,
        }
    }

    #[test]
    fn two_rows_registered() {
        assert_eq!(rows().len(), 2);
    }

    #[test]
    fn correct_stream_mock_passes_both_rows() {
        let (_s, ep) = harness();
        let f = StreamMockFactory::correct();
        let ctx = ctx_of(&f, &ep);
        assert_eq!(row_12(&ctx), RowResult::Pass);
        assert_eq!(row_13(&ctx), RowResult::Pass);
    }

    #[test]
    fn back_pressure_is_load_bearing_no_overflow_under_slow_consumer() {
        // The conformant mock delivers ROW_CHUNKS (> RING_CAPACITY) chunks; only honouring the pushed
        // Pause keeps the ring from overflowing. A pass here IS the proof the signal surface works.
        let (_s, ep) = harness();
        let f = StreamMockFactory::correct();
        let ctx = ctx_of(&f, &ep);
        let obs = {
            let adapter = f.streaming().expect("streaming present");
            let url = super::chunked_url(&ctx).expect("url");
            let req =
                HttpRequest::builder(crate::request::Method::Get, url, Duration::from_secs(5))
                    .build();
            drive_streaming(adapter.as_ref(), req)
        };
        assert_eq!(obs.delivered_chunks, ROW_CHUNKS);
        assert!(matches!(obs.terminal, Some(Ok(_))));
    }

    // --- the red twins, watched red per fault --------------------------------------------

    #[test]
    fn row_12_red_on_truncation() {
        // A dropped chunk under back-pressure: declared total > ingested ⇒ the completeness gate
        // fires ⇒ the terminal is a typed failure ⇒ row red.
        let (_s, ep) = harness();
        let f = StreamMockFactory::correct().with_fault(StreamFault::Truncate);
        assert!(matches!(
            row_12(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::StreamFailed { .. })
        ));
    }

    #[test]
    fn row_12_red_on_ignored_back_pressure_is_overflow() {
        // Ignoring Pause under the slow consumer overflows the bounded ring: StreamOverflow is the
        // terminal (the adapter-driven positive control the M1 notes promised for that key).
        let (_s, ep) = harness();
        let f = StreamMockFactory::correct().with_fault(StreamFault::IgnorePause);
        match row_12(&ctx_of(&f, &ep)) {
            RowResult::Fail(FailureReason::StreamFailed { got }) => {
                assert_eq!(got, crate::HttpErrorKey::StreamOverflow);
            }
            other => panic!("expected StreamOverflow terminal, got {other:?}"),
        }
    }

    #[test]
    fn row_13_red_on_missing_terminal() {
        // Deliver every chunk, never finish: no terminal arrives ⇒ row red.
        let (_s, ep) = harness();
        let f = StreamMockFactory::correct().with_fault(StreamFault::SkipTerminal);
        assert!(matches!(
            row_13(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::NoTerminal)
        ));
    }

    #[test]
    fn row_12_skips_without_streaming() {
        // A factory with no streaming capability records a skip, never a vacuous pass.
        let (_s, ep) = harness();
        let f = crate::conformance::netmock::SocketMockFactory::correct(ep.good_spki());
        let ctx = ConformanceCtx {
            factory: &f,
            endpoints: &ep,
        };
        assert!(matches!(row_12(&ctx), RowResult::Skipped(_)));
    }
}
