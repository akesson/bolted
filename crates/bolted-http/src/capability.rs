//! The [`Http`] capability trait and its completion machinery, plus the optional capability
//! traits ([`Metrics`], [`PriorityHint`]).
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

/// The HTTP capability: dispatch a request effect; the adapter performs it (out of this crate)
/// and delivers the completion to `completion`.
///
/// Object-safe: `dyn Http` is the "adapter under test" the conformance suite drives.
pub trait Http: MaybeSend {
    /// Dispatch `request`. Returns immediately with a [`RequestHandle`] for cancellation; the
    /// terminal outcome arrives later (or synchronously, for an in-memory adapter) via
    /// `completion`.
    fn send(&self, request: HttpRequest, completion: Box<dyn CompletionSink>) -> RequestHandle;
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
#[derive(Clone, Debug)]
pub struct RequestHandle {
    token: CancelToken,
}

impl RequestHandle {
    /// Build a handle over the adapter's cancellation token.
    #[must_use]
    pub fn for_token(token: CancelToken) -> Self {
        RequestHandle { token }
    }

    /// Request cancellation of the in-flight effect.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// The underlying token (the adapter polls it).
    #[must_use]
    pub fn token(&self) -> CancelToken {
        self.token.clone()
    }
}

// --- Optional capability traits (feature-matrix §4) -------------------------------------
//
// These are how the C3 divergence matrix is *generated from the capability types*: an adapter's
// present/absent capabilities are read off which of these it implements, never hand-written.

/// **CAP** (row 12): the adapter honours the request's [`crate::Priority`] hint. Marker only —
/// its presence is the honest signal; an adapter that legally ignores priority (OkHttp, .NET)
/// simply does not implement it. No method: the hint data rides the request already.
pub trait PriorityHint: Http {}

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
