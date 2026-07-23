//! The one core→adapter mid-flight signal surface (Q4 + streaming-seam §3b option C, ruled
//! 2026-07-21).
//!
//! A single shape — [`FlowSignal`] — carries **two uses**, both pushed from the core (driver) to a
//! conformant adapter:
//!
//! 1. **Back-pressure** ([`FlowSignal::Pause`] / [`FlowSignal::Resume`]): the driver tells the
//!    adapter to stop / resume delivering body chunks so the core's bounded ring never overflows.
//!    The adapter reacts by pausing its socket read (URLSession suspend, OkHttp source read-pacing,
//!    reqwest stream polling) — the capability-shaped extension that turns the fail-loud overflow
//!    into a *never-fires-in-practice* ceiling.
//! 2. **Cancellation** ([`FlowSignal::Cancel`]): a **pushed** cancel that replaces the 10 ms
//!    poll-watcher thread every adapter paid before (streaming-seam §3b). `RequestHandle::cancel`
//!    and the streaming handle both route here.
//!
//! ## Sans-io, lock-free (kill criterion 2)
//!
//! The contract mandates **no thread and no channel type**. The core emits a signal *value*; the
//! adapter *observes* it through the [`FlowObserver`] it registered, reacting however it can. The
//! observer is an `Arc` fixed at construction, so emitting a signal is a synchronous call with no
//! interior mutability and no lock in the contract type. Any thread/channel/notify machinery a
//! particular adapter needs (a tokio `Notify`, an `AtomicBool`, an `AbortHandle`) lives in that
//! adapter, behind its `FlowObserver` — never here.
//!
//! ## A future third instance (cookie per-hop re-entry, Q9)
//!
//! This is the core→adapter direction of the mid-flight seam (streaming-seam §4). The adapter→core
//! direction is [`crate::capability::ChunkSink`]; the cookie jar's per-hop consultation is a named
//! *future* third instance. The shape is defined once here so it can attach without re-opening the
//! contract.

use std::sync::Arc;

use crate::MaybeSend;

/// One core→adapter mid-flight signal. **One shape, two uses** — back-pressure (`Pause`/`Resume`)
/// and cancellation (`Cancel`). `#[non_exhaustive]`: the mid-flight vocabulary may grow (the cookie
/// per-hop re-entry is a named future instance).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowSignal {
    /// Stop delivering body chunks: the driver's ring is near capacity (back-pressure). A conformant
    /// adapter stops reading its socket until [`FlowSignal::Resume`], so the ring never overflows.
    Pause,
    /// Resume delivering body chunks — back-pressure relieved (the consumer has drained).
    Resume,
    /// Cancel the in-flight request. The **pushed** cancel that replaces poll-watching a
    /// [`crate::CancelToken`]; the request still completes, with `Err(HttpError::Cancelled)`.
    Cancel,
}

/// A conformant adapter's reaction to pushed [`FlowSignal`]s (streaming-seam §3b option C). The core
/// invokes [`FlowObserver::on_signal`] synchronously when it emits a signal; the reaction is the
/// adapter's own — suspend a task, set an atomic, notify a runtime primitive, abort. The contract
/// mandates no thread or channel here (the reaction supplies whatever it needs).
///
/// `Sync` (beyond [`MaybeSend`]) because a single observer is shared behind an `Arc` and a signal
/// may be emitted from any thread (a caller's `cancel`, the driver's back-pressure monitor).
pub trait FlowObserver: MaybeSend + Sync {
    /// React to one pushed signal.
    fn on_signal(&self, signal: FlowSignal);
}

/// The core→adapter signal **emitter** for one in-flight request. The driver holds it and pushes
/// signals through the named methods; the adapter's registered [`FlowObserver`] receives them.
///
/// Cheap to clone (an `Arc`), lock-free: emitting a signal is a synchronous call into the observer.
#[derive(Clone)]
pub struct FlowSignals {
    observer: Arc<dyn FlowObserver>,
}

impl FlowSignals {
    /// Build an emitter over the adapter's `observer`.
    #[must_use]
    pub fn new(observer: Arc<dyn FlowObserver>) -> Self {
        FlowSignals { observer }
    }

    /// Push [`FlowSignal::Pause`] — stop delivering body chunks (back-pressure).
    pub fn pause(&self) {
        self.emit(FlowSignal::Pause);
    }

    /// Push [`FlowSignal::Resume`] — resume delivering body chunks.
    pub fn resume(&self) {
        self.emit(FlowSignal::Resume);
    }

    /// Push [`FlowSignal::Cancel`] — cancel the in-flight request.
    pub fn cancel(&self) {
        self.emit(FlowSignal::Cancel);
    }

    fn emit(&self, signal: FlowSignal) {
        self.observer.on_signal(signal);
    }
}

impl std::fmt::Debug for FlowSignals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowSignals").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A test observer that records every signal it receives.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<FlowSignal>>);

    impl FlowObserver for Recorder {
        fn on_signal(&self, signal: FlowSignal) {
            if let Ok(mut g) = self.0.lock() {
                g.push(signal);
            }
        }
    }

    #[test]
    fn all_three_uses_reach_the_observer_in_order() {
        let recorder = Arc::new(Recorder::default());
        let signals = FlowSignals::new(recorder.clone());
        signals.pause();
        signals.resume();
        signals.cancel();
        let seen = recorder.0.lock().expect("lock").clone();
        assert_eq!(
            seen,
            vec![FlowSignal::Pause, FlowSignal::Resume, FlowSignal::Cancel]
        );
    }

    #[test]
    fn a_clone_pushes_to_the_same_observer() {
        let recorder = Arc::new(Recorder::default());
        let signals = FlowSignals::new(recorder.clone());
        let clone = signals.clone();
        clone.cancel();
        assert_eq!(
            recorder.0.lock().expect("lock").as_slice(),
            &[FlowSignal::Cancel]
        );
    }
}
