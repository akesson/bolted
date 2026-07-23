//! The [`Http`] capability trait and its completion machinery, plus the optional capability
//! trait [`Metrics`].
//!
//! **Sans-io, callback/completion shaped.** There is no async runtime in this crate: an adapter
//! is handed the request effect and delivers the single completion to a [`CompletionSink`]. One
//! effect, one completion — the sink is consumed on delivery (`self: Box<Self>`), so it cannot
//! fire twice (feature-matrix §7 rule 8/9). Cancellation is a [`RequestHandle`]/[`CancelToken`]
//! pair rather than a dropped future.
//!
//! All trait bounds route through [`crate::MaybeSend`] (the Send seam), never `Send` directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::MaybeSend;
use crate::error::HttpError;
use crate::request::HttpRequest;
use crate::response::HttpResponse;
use crate::signal::FlowSignals;
use crate::stream::{BodyChunk, BodyEnd};

/// The sink an adapter delivers a request's single completion to.
///
/// `complete` takes `self: Box<Self>`: delivering the outcome consumes the sink, so the
/// "one effect, one completion" invariant is enforced by the type — a second delivery does not
/// type-check. A cancelled effect still completes, with `Err(HttpError::Cancelled)` (rule 9,
/// never silence).
pub trait CompletionSink: MaybeSend {
    /// Deliver the request's terminal outcome. Called exactly once.
    fn complete(self: Box<Self>, outcome: Result<HttpResponse, HttpError>);
}

/// An observer of upload progress for one request (feature-matrix row 14, §5.9, rule 11).
///
/// **Repeatable** (unlike [`CompletionSink`], which is one-shot): `progress` is called zero or more
/// times as the request body is handed off, then the request terminates through `completion`.
///
/// Contract semantics (rule 11 — the suite pins exactly this, no more):
/// - `sent` is **monotone non-decreasing** within one attempt.
/// - Terminally **consistent with the completion**: on success, the final `sent` equals the body
///   length when that length is known (`total = Some(len)`).
/// - **Indicative, not wire-truth.** Synthesized figures measure buffer hand-off, not bytes on the
///   wire (Apple's `didSendBodyData` is OS-fed; OkHttp/.NET/reqwest wrap the body sink). The
///   contract never promises wire bytes, so the suite never asserts them.
///
/// `total` is `Some(len)` when the body length is known up front, `None` otherwise.
pub trait UploadProgressSink: MaybeSend {
    /// Report cumulative bytes handed off (`sent`) against the optional known total.
    fn progress(&self, sent: u64, total: Option<u64>);
}

/// The HTTP capability: dispatch a request effect; the adapter performs it (out of this crate)
/// and delivers the completion to `completion`.
///
/// Object-safe: `dyn Http` is the "adapter under test" the conformance suite drives.
pub trait Http: MaybeSend {
    /// Dispatch `request`. Returns immediately with a [`RequestHandle`] for cancellation; the
    /// terminal outcome arrives later (or synchronously, for an in-memory adapter) via
    /// `completion`. When `upload_progress` is `Some`, the adapter reports body hand-off progress
    /// through it (row 14 / rule 11) — repeatable, terminating consistently with `completion`.
    fn send(
        &self,
        request: HttpRequest,
        completion: Box<dyn CompletionSink>,
        upload_progress: Option<Box<dyn UploadProgressSink>>,
    ) -> RequestHandle;
}

/// A cooperative cancellation flag shared between the caller (via [`RequestHandle`]) and the
/// adapter. Cheap to clone; the adapter polls [`CancelToken::is_cancelled`].
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, not-yet-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested (polled by the adapter).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A handle to an in-flight request. Dropping it does not cancel; call [`RequestHandle::cancel`]
/// (row 21). The completion still arrives — as `Err(HttpError::Cancelled)` when cancelled.
///
/// ## Poll vs. push cancellation
///
/// Cancellation reaches the adapter one of two ways, and a handle may carry both:
/// - the poll-based [`CancelToken`] (an atomic the adapter polls), and
/// - the **pushed** [`FlowSignals`] surface ([`RequestHandle::with_signals`]), which delivers
///   [`crate::signal::FlowSignal::Cancel`] synchronously so the adapter needs no poll-watcher thread
///   (streaming-seam §3b / Q4).
///
/// [`RequestHandle::cancel`] fires **both**: it sets the token (for any adapter still polling — the
/// FFI bridges do, until steps M3/M4) *and* pushes the signal (for an adapter that has migrated off
/// the poll-watcher, e.g. `bolted-http-linux`). An adapter uses whichever it registered; the other
/// is a cheap no-op.
#[derive(Clone, Debug)]
pub struct RequestHandle {
    token: CancelToken,
    signals: Option<FlowSignals>,
}

impl RequestHandle {
    /// Build a handle over the adapter's cancellation token (poll-based cancel only).
    #[must_use]
    pub fn for_token(token: CancelToken) -> Self {
        RequestHandle {
            token,
            signals: None,
        }
    }

    /// Build a handle carrying both the poll token and the pushed [`FlowSignals`] surface. An
    /// adapter that has deleted its poll-watcher registers a [`crate::signal::FlowObserver`], wraps
    /// it in `signals`, and reacts to the pushed [`crate::signal::FlowSignal::Cancel`].
    #[must_use]
    pub fn with_signals(token: CancelToken, signals: FlowSignals) -> Self {
        RequestHandle {
            token,
            signals: Some(signals),
        }
    }

    /// Request cancellation of the in-flight effect — sets the poll token **and** pushes
    /// [`crate::signal::FlowSignal::Cancel`] when a [`FlowSignals`] surface is registered.
    pub fn cancel(&self) {
        self.token.cancel();
        if let Some(signals) = &self.signals {
            signals.cancel();
        }
    }

    /// The underlying token (the adapter polls it).
    #[must_use]
    pub fn token(&self) -> CancelToken {
        self.token.clone()
    }
}

/// The adapter→core (driver) body-chunk delivery sink for a **streamed** response
/// (streaming-seam §3a/§3c). The streaming analogue of [`CompletionSink`]: an adapter delivers each
/// body chunk, then exactly one terminal.
///
/// **Repeatable for chunks, one-shot for the terminal.** [`ChunkSink::deliver_chunk`] is `&self`
/// (called once per chunk); [`ChunkSink::finish`] takes `self: Box<Self>`, so the terminal fires
/// **exactly once, enforced by the type** — a second terminal, or a chunk after the terminal, is a
/// use-after-move that does not compile (the step-24 one-shot discipline, extended to the stream;
/// the same property [`crate::stream::BodyStream::finish`] gives the core-owned ingest).
///
/// The driver owns the core-side [`crate::stream::BodyStream`] behind whatever synchronisation it
/// needs (a `Mutex` in the harness; a store lock in a real driver) — never in this crate. The sink
/// is the seam; the ring, the `seq` verifier and the completeness gate live behind it.
pub trait ChunkSink: MaybeSend {
    /// Deliver the next body chunk. Returns the typed failure the core ingest raised — a `seq`
    /// violation ([`HttpError::Transport`]) or ring overflow ([`HttpError::StreamOverflow`]) — so a
    /// conformant adapter can stop reading and close the stream with it.
    ///
    /// # Errors
    /// The core ingest's typed failure ([`HttpError::Transport`] on a `seq` violation;
    /// [`HttpError::StreamOverflow`] on ring overflow).
    fn deliver_chunk(&self, chunk: BodyChunk) -> Result<(), HttpError>;

    /// Close the stream with its terminal (design §3c). Consumes the sink — one terminal by
    /// construction. The driver's completeness gate ([`crate::stream::BodyStream::finish`]) turns a
    /// `Complete { total }` whose declared total disagrees with the ingested bytes into a failure.
    fn finish(self: Box<Self>, end: BodyEnd);
}

/// The **streaming** HTTP capability (streaming-seam §3b option C): dispatch a request whose response
/// body is streamed chunk-by-chunk into a driver-owned [`ChunkSink`], with the one core→adapter
/// [`FlowSignals`] surface (back-pressure + pushed cancel).
///
/// A capability trait an adapter **opts into** (like [`Metrics`]), never a widening of the base
/// [`Http`] contract: the buffered completion path is unchanged for adapters that do not stream. An
/// adapter that implements it receives pushed signals and must honour [`FlowSignal::Pause`] by
/// pausing its socket read (so the ring never overflows) and [`FlowSignal::Cancel`] by cancelling.
///
/// [`FlowSignal::Pause`]: crate::signal::FlowSignal::Pause
/// [`FlowSignal::Cancel`]: crate::signal::FlowSignal::Cancel
pub trait StreamingHttp: Http {
    /// Dispatch `request`, streaming its response body into `chunks`. Returns immediately with the
    /// [`FlowSignals`] emitter the driver uses to push back-pressure (pause/resume) and cancel; the
    /// chunks and the single terminal arrive on `chunks`.
    fn send_streaming(&self, request: HttpRequest, chunks: Box<dyn ChunkSink>) -> FlowSignals;
}

// --- Optional capability traits (feature-matrix §4) -------------------------------------
//
// These are how the C3 divergence matrix is *generated from the capability types*: an adapter's
// present/absent capabilities are read off which of these it implements, never hand-written.

/// **CAP, tiered** (row 18 / §5.13): request metrics. The capability exposes its *tier* rather
/// than pretending uniformity — reqwest has no phase seam to synthesise DNS/TLS timings from, so
/// Linux is honestly [`MetricsTier::WholeRequest`]. The richer per-request metrics payload is a
/// later milestone; the tier is what the divergence matrix needs now.
pub trait Metrics: Http {
    /// The metrics tier this adapter can honestly report.
    fn tier(&self) -> MetricsTier;
}

/// How much timing detail an adapter can honestly report (feature-matrix §5.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricsTier {
    /// Tier A: per-phase timings (DNS/connect/TLS/first-byte) and TLS detail (Apple, .NET, OkHttp).
    Phase,
    /// Tier B: whole-request timing only (Linux/reqwest — no phase seam to synthesise from).
    WholeRequest,
}
