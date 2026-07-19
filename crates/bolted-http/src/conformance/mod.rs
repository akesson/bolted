//! The conformance harness (feature `conformance`) — the generic "adapter under test" seam and
//! the row-registration shape the C1/C2/C3 clusters will fill (feature-matrix §7).
//!
//! **Skeleton only in M0.** One placeholder row is wired end-to-end to prove the harness *bites*:
//! it passes against a correct mock and fails against a deliberately-broken one (the one-implementor
//! lesson — the suite must fail correctly before it can pass correctly). The eleven §7 rules, the
//! C2 taxonomy, and the C3 divergence matrix land in M1 on this frame.
//!
//! This module is behind the `conformance` feature so the default `bolted-http` surface stays
//! dependency-clean; the real adapters (`bolted-http-linux`, and the shell adapters via their host
//! test harnesses) enable it to reuse the suite.

pub mod mock;

use std::sync::mpsc;
use std::time::Duration;

use crate::capability::{CompletionSink, Http};
use crate::error::HttpError;
use crate::response::HttpResponse;

/// A source of fresh adapters-under-test. Each row gets a new adapter, so rows never share
/// mutable state. A real adapter's factory builds a configured client; the mock's builds a
/// scripted in-memory adapter (see [`mock::MockFactory`]).
pub trait AdapterFactory {
    /// Build a fresh adapter for one row.
    fn new_adapter(&self) -> Box<dyn Http>;
}

/// Which conformance cluster a row belongs to (feature-matrix §7 / spike-plan C1–C3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cluster {
    /// C1 — the eleven §7 rules as parameterised rows.
    C1Rule,
    /// C2 — the error-taxonomy matrix, one positive control per [`crate::HttpErrorKey`].
    C2Taxonomy,
    /// C3 — the divergence matrix, generated from the capability types.
    C3Divergence,
}

/// The outcome of running one row against one adapter.
#[derive(Debug, PartialEq, Eq)]
pub enum RowResult {
    /// The adapter honoured the row.
    Pass,
    /// The adapter violated the row — reported as data, never a message string.
    Fail(FailureReason),
}

/// Why a row failed (typed — the suite reports data, mirroring the error rule).
#[derive(Debug, PartialEq, Eq)]
pub enum FailureReason {
    /// The response status did not match what the row required.
    UnexpectedStatus {
        /// The status the row required.
        expected: u16,
        /// The status the adapter produced.
        got: u16,
    },
    /// The row expected success but the adapter produced an error.
    UnexpectedError,
    /// The adapter never delivered a completion within the row's budget.
    NoCompletion,
}

/// One registered conformance row: an id, its cluster, and the check that runs it against a
/// factory. `check` is a plain `fn` pointer so a row table is a `const`/static slice.
pub struct ConformanceRow {
    /// A stable row id (e.g. `"C1/rule-01-same-request-same-outcome"`).
    pub id: &'static str,
    /// The cluster the row belongs to.
    pub cluster: Cluster,
    /// The check: build an adapter from `factory`, drive it, judge the outcome.
    pub check: fn(&dyn AdapterFactory) -> RowResult,
}

/// Run `rows` against one `factory`, returning each row's result. The suite entry point real
/// adapters call once their factory is wired.
#[must_use]
pub fn run(
    rows: &[ConformanceRow],
    factory: &dyn AdapterFactory,
) -> Vec<(&'static str, RowResult)> {
    rows.iter().map(|r| (r.id, (r.check)(factory))).collect()
}

/// The M0 placeholder row set (one row). M1 replaces this with the eleven §7 rules, the C2
/// taxonomy, and the generated C3 matrix.
#[must_use]
pub fn placeholder_rows() -> &'static [ConformanceRow] {
    &[ConformanceRow {
        id: "C1/placeholder-echo-succeeds",
        cluster: Cluster::C1Rule,
        check: row_placeholder_echo,
    }]
}

/// A synchronous sink that forwards the completion down a channel, so a row can drive an adapter
/// and read the single outcome back without an async runtime.
struct ChannelSink(mpsc::Sender<Result<HttpResponse, HttpError>>);

impl CompletionSink for ChannelSink {
    fn complete(self: Box<Self>, outcome: Result<HttpResponse, HttpError>) {
        // The receiver may be gone if the row already timed out; that is not our concern here.
        let _ = self.0.send(outcome);
    }
}

/// Send one request through an adapter and collect its single completion. Returns `None` if the
/// adapter delivers nothing within `budget` (a broken adapter that never completes).
fn drive_once(
    adapter: &dyn Http,
    request: crate::request::HttpRequest,
    budget: Duration,
) -> Option<Result<HttpResponse, HttpError>> {
    let (tx, rx) = mpsc::channel();
    let _handle = adapter.send(request, Box::new(ChannelSink(tx)));
    rx.recv_timeout(budget).ok()
}

/// The placeholder row: a plain request must succeed with `200`. The correct mock returns `200`
/// (Pass); the broken mock returns `500` (Fail) — the harness's fail-correctly demonstration.
fn row_placeholder_echo(factory: &dyn AdapterFactory) -> RowResult {
    let adapter = factory.new_adapter();
    let url = match crate::request::Url::https("https://echo.test/") {
        Ok(u) => u,
        Err(_) => return RowResult::Fail(FailureReason::NoCompletion),
    };
    let request = crate::request::HttpRequest::builder(
        crate::request::Method::Get,
        url,
        Duration::from_secs(30),
    )
    .build();

    match drive_once(adapter.as_ref(), request, Duration::from_secs(2)) {
        Some(Ok(resp)) => {
            let got = resp.status().as_u16();
            if got == 200 {
                RowResult::Pass
            } else {
                RowResult::Fail(FailureReason::UnexpectedStatus { expected: 200, got })
            }
        }
        Some(Err(_)) => RowResult::Fail(FailureReason::UnexpectedError),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockFactory;

    #[test]
    fn placeholder_row_passes_against_correct_mock() {
        let results = run(placeholder_rows(), &MockFactory::correct());
        assert!(
            results.iter().all(|(_, r)| matches!(r, RowResult::Pass)),
            "the correct mock must pass every placeholder row: {results:?}"
        );
    }

    #[test]
    fn placeholder_row_fails_correctly_against_broken_mock() {
        // The suite must be able to *catch* a wrong adapter before it is allowed to bless a right
        // one. The broken mock returns 500; the row must report Fail. This green test is the
        // permanent record that the harness bites — not a permanently-red test.
        let results = run(placeholder_rows(), &MockFactory::broken());
        assert!(
            results.iter().any(|(_, r)| matches!(
                r,
                RowResult::Fail(FailureReason::UnexpectedStatus {
                    expected: 200,
                    got: 500
                })
            )),
            "the broken mock must fail the placeholder row with the wrong status: {results:?}"
        );
    }

    #[test]
    fn a_never_completing_adapter_is_caught_not_hung() {
        // A row must not hang on an adapter that never delivers a completion (rule 8/9 territory).
        let results = run(placeholder_rows(), &MockFactory::never_completes());
        assert!(
            results
                .iter()
                .any(|(_, r)| matches!(r, RowResult::Fail(FailureReason::NoCompletion))),
            "a silent adapter must be reported as NoCompletion: {results:?}"
        );
    }
}
