//! The conformance harness (feature `conformance`) — the generic "adapter under test" seam and
//! the C1/C2/C3 clusters (feature-matrix §7, spike-plan §1).
//!
//! - **C1** ([`c1`]): the eleven §7 rules as parameterised [`ConformanceRow`]s, driven against the
//!   real [`server::TestServer`] via the socket mock ([`netmock`]).
//! - **C2** ([`c2`]): the error-taxonomy matrix — every reachable [`crate::HttpErrorKey`] with a
//!   positive control endpoint (unreachable keys are recorded as adapter-only, never skipped).
//! - **C3** ([`c3`]): the divergence matrix generated from the capability types.
//!
//! The suite must fail correctly before it can pass correctly (the one-implementor lesson): every
//! row has a red-twin test that flips exactly one thing in the mock and watches the row go red.
//!
//! Everything is behind the `conformance` feature so the default `bolted-http` surface stays
//! dependency-clean; real adapters (`bolted-http-linux`, the shell hosts) enable it to reuse the
//! suite.

pub mod c1;
pub mod c2;
pub mod c3;
pub mod mock;
pub mod netmock;
pub mod server;
pub mod wire;

use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use std::sync::{Arc, Mutex};

use crate::capability::{CompletionSink, Http, Metrics, PriorityHint, UploadProgressSink};
use crate::error::HttpError;
use crate::request::HttpRequest;
use crate::response::HttpResponse;

/// A source of fresh adapters-under-test, plus its optional-capability self-report. Each row gets a
/// new adapter, so rows never share mutable state.
///
/// The capability methods are how C3 is **generated from the types**: returning `Some(..)` only
/// type-checks if the concrete adapter actually implements the trait, so the divergence table can
/// never drift from the real impls (hand-written prose matrices are the prior-art failure mode).
/// The default is "absent".
pub trait AdapterFactory {
    /// Build a fresh adapter for one row.
    fn new_adapter(&self) -> Box<dyn Http>;

    /// The adapter as a [`PriorityHint`], if it honours priority (row 12, CAP). Default: absent.
    fn priority_hint(&self) -> Option<Box<dyn PriorityHint>> {
        None
    }

    /// The adapter as a [`Metrics`] source, if it reports metrics (row 18, CAP tiered). Default:
    /// absent.
    fn metrics(&self) -> Option<Box<dyn Metrics>> {
        None
    }
}

/// Which conformance cluster a row belongs to (feature-matrix §7 / spike-plan C1–C3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cluster {
    /// C1 — the eleven §7 rules.
    C1Rule,
    /// C2 — the error-taxonomy matrix.
    C2Taxonomy,
}

/// The outcome of running one row against one adapter.
#[derive(Debug, PartialEq, Eq)]
pub enum RowResult {
    /// The adapter honoured the row.
    Pass,
    /// The adapter violated the row — reported as data, never a message string.
    Fail(FailureReason),
    /// The row is not expressible against the current contract surface — recorded explicitly, not
    /// silently green (a needle that can never match is green forever).
    Skipped(SkipReason),
}

/// Why a row failed (typed — the suite reports data, mirroring the error rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureReason {
    /// The response status did not match what the row required.
    UnexpectedStatus {
        /// The status the row required.
        expected: u16,
        /// The status the adapter produced.
        got: u16,
    },
    /// The row expected success but the adapter produced an error.
    ExpectedSuccessGotError {
        /// The error key produced.
        got: crate::HttpErrorKey,
    },
    /// The row expected a typed error but the adapter succeeded.
    ExpectedErrorGotSuccess {
        /// The error key the row required.
        expected: crate::HttpErrorKey,
        /// The status the adapter produced instead.
        status: u16,
    },
    /// The adapter produced an error, but the wrong key.
    WrongErrorKey {
        /// The key the row required.
        expected: crate::HttpErrorKey,
        /// The key the adapter produced.
        got: crate::HttpErrorKey,
    },
    /// The adapter never delivered a completion within the row's budget.
    NoCompletion,
    /// A permitted request header was silently dropped (rule 6 runtime half).
    MissingHeader {
        /// The header the server never saw echoed back.
        name: &'static str,
    },
    /// The response body did not match what the row required (rules 1, 7).
    WrongBody,
    /// Two identical requests produced different outcomes (rule 1).
    NotDeterministic,
    /// Timeout and cancel collapsed to the same key (rule 2).
    KeysNotDistinct {
        /// The key both produced.
        key: crate::HttpErrorKey,
    },
    /// A mid-flight failure was silently retried (rule 8): the endpoint saw more than one hit.
    HiddenRetry {
        /// How many connections the endpoint saw.
        connections: usize,
    },
    /// The delivered body sink did not correspond to the requested [`crate::ResponseSink`]
    /// (row 15): a `File` request came back as `Memory`, or vice versa, or the file contents
    /// did not match the body.
    WrongSink,
    /// A decoded body reported a dishonest `content_length` (rule 7 / §5.12): under decoding the
    /// only honest answers are `None` or the decoded length; the adapter reported the compressed
    /// figure instead.
    DishonestContentLength {
        /// The length the adapter reported.
        got: u64,
        /// The decoded body length (the only honest non-`None` value).
        decoded: u64,
    },
    /// Upload progress went backwards within one attempt (rule 11 — not monotone).
    ProgressNotMonotone {
        /// The previously reported cumulative `sent`.
        prev: u64,
        /// The lower value reported after it.
        got: u64,
    },
    /// Upload progress did not terminate consistently with the completion (rule 11): the final
    /// `sent` did not equal the known body length.
    ProgressNotTerminal {
        /// The final `sent` reported (0 when no progress arrived at all).
        got: u64,
        /// The body length the final `sent` had to reach.
        expected: u64,
    },
}

/// Why a row could not be expressed (recorded, for the report — never a silent pass). As of M1.5
/// no C1/C2 row skips — the M1 contract gaps (response-sink selector, upload-progress surface) are
/// closed — but the variant stays: a later adapter row (e.g. a platform-only behaviour) may still
/// have no host-expressible surface, and recording that beats a silently-green needle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The contract has no host-expressible surface for this behaviour; recorded, not skipped.
    NoContractSurface {
        /// A short identifier of the missing surface.
        capability: &'static str,
    },
}

/// The URLs and pin data the rows drive against — owned by the harness, shared by every adapter.
pub struct Endpoints {
    http_base: String,
    https_base: String,
    https_untrusted_base: String,
    good_spki: [u8; 32],
    untrusted_spki: [u8; 32],
    unresolvable: String,
    closed_port: String,
}

impl Endpoints {
    /// Build the endpoint set from a running server (+ a couple of synthetic failure targets a
    /// server cannot host: an unresolvable name and a refused port).
    #[must_use]
    pub fn from_server(server: &server::TestServer) -> Self {
        // A guaranteed-unused port: bind, read the number, drop the listener.
        let closed_port = TcpListener::bind("127.0.0.1:0")
            .ok()
            .and_then(|l| l.local_addr().ok())
            .map(|a| format!("http://127.0.0.1:{}/ok", a.port()))
            .unwrap_or_else(|| "http://127.0.0.1:9/ok".to_string());
        Endpoints {
            http_base: server.http_base(),
            https_base: server.https_base(),
            https_untrusted_base: server.https_untrusted_base(),
            good_spki: server.good_spki(),
            untrusted_spki: server.untrusted_spki(),
            // `.invalid` is reserved to never resolve (RFC 2606).
            unresolvable: "https://nonexistent.invalid/ok".to_string(),
            closed_port,
        }
    }

    /// A cleartext URL for `path` (e.g. `"/echo"`).
    #[must_use]
    pub fn http(&self, path: &str) -> String {
        format!("{}{}", self.http_base, path)
    }

    /// A TLS URL (good cert) for `path`.
    #[must_use]
    pub fn https(&self, path: &str) -> String {
        format!("{}{}", self.https_base, path)
    }

    /// A TLS URL presenting the untrusted cert.
    #[must_use]
    pub fn https_untrusted(&self, path: &str) -> String {
        format!("{}{}", self.https_untrusted_base, path)
    }

    /// The good cert's raw SPKI hash — what the socket mock trusts as its anchor.
    #[must_use]
    pub fn good_spki(&self) -> [u8; 32] {
        self.good_spki
    }

    /// The good cert's SPKI pin (pinning it against [`Endpoints::https`] succeeds).
    #[must_use]
    pub fn good_pin(&self) -> crate::SpkiPin {
        crate::SpkiPin::from_sha256(self.good_spki)
    }

    /// The untrusted cert's raw SPKI hash (used by a C2 red-twin that mis-trusts it).
    #[must_use]
    pub fn untrusted_spki(&self) -> [u8; 32] {
        self.untrusted_spki
    }

    /// A pin that does not match [`Endpoints::https`] (the rule-10 mismatch).
    #[must_use]
    pub fn wrong_pin(&self) -> crate::SpkiPin {
        crate::SpkiPin::from_sha256(self.untrusted_spki)
    }

    /// A URL whose host never resolves (NameResolution positive control).
    #[must_use]
    pub fn unresolvable(&self) -> &str {
        &self.unresolvable
    }

    /// A cleartext URL to a refused port (Connect positive control).
    #[must_use]
    pub fn closed_port(&self) -> &str {
        &self.closed_port
    }
}

/// What a row is handed: the factory (adapter under test) plus the shared endpoints.
pub struct ConformanceCtx<'a> {
    /// The adapter factory under test.
    pub factory: &'a dyn AdapterFactory,
    /// The shared server endpoints + pin data.
    pub endpoints: &'a Endpoints,
}

/// One registered conformance row: an id, its cluster, and the check that runs it against a ctx.
/// `check` is a plain `fn` pointer so a row table is a `const` slice; the runtime data (server
/// addresses) reaches it through [`ConformanceCtx`], not through a capture.
pub struct ConformanceRow {
    /// A stable row id (e.g. `"C1/rule-03-stalled-body-times-out"`).
    pub id: &'static str,
    /// The cluster the row belongs to.
    pub cluster: Cluster,
    /// The check: build an adapter from the ctx's factory, drive it against an endpoint, judge.
    pub check: fn(&ConformanceCtx) -> RowResult,
}

/// Run `rows` against one ctx, returning each row's result.
#[must_use]
pub fn run(rows: &[ConformanceRow], ctx: &ConformanceCtx) -> Vec<(&'static str, RowResult)> {
    rows.iter().map(|r| (r.id, (r.check)(ctx))).collect()
}

// --- Driving helpers shared by the row modules -----------------------------------------

/// A synchronous sink that forwards the completion down a channel.
struct ChannelSink(mpsc::Sender<Result<HttpResponse, HttpError>>);

impl CompletionSink for ChannelSink {
    fn complete(self: Box<Self>, outcome: Result<HttpResponse, HttpError>) {
        let _ = self.0.send(outcome);
    }
}

/// Send one request and collect its single completion. `None` if nothing arrives within `budget`
/// (a silent / hung adapter).
pub(crate) fn drive_once(
    adapter: &dyn Http,
    request: HttpRequest,
    budget: Duration,
) -> Option<Result<HttpResponse, HttpError>> {
    let (tx, rx) = mpsc::channel();
    let _handle = adapter.send(request, Box::new(ChannelSink(tx)), None);
    rx.recv_timeout(budget).ok()
}

/// A recorded upload-progress sequence: `(sent, total)` samples in call order (rule 11).
pub(crate) type ProgressSamples = Vec<(u64, Option<u64>)>;

/// A [`UploadProgressSink`] that records every `(sent, total)` reported, for rule 11's judgement.
#[derive(Clone, Default)]
pub(crate) struct RecordingProgress(Arc<Mutex<ProgressSamples>>);

impl RecordingProgress {
    pub(crate) fn new() -> Self {
        RecordingProgress(Arc::new(Mutex::new(Vec::new())))
    }

    /// The recorded `(sent, total)` samples, in call order.
    pub(crate) fn samples(&self) -> ProgressSamples {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl UploadProgressSink for RecordingProgress {
    fn progress(&self, sent: u64, total: Option<u64>) {
        if let Ok(mut g) = self.0.lock() {
            g.push((sent, total));
        }
    }
}

/// Send one request with an attached [`RecordingProgress`] sink; return the completion and the
/// recorded progress samples. `None` completion if nothing arrives within `budget`.
pub(crate) fn drive_with_progress(
    adapter: &dyn Http,
    request: HttpRequest,
    budget: Duration,
) -> (Option<Result<HttpResponse, HttpError>>, ProgressSamples) {
    let (tx, rx) = mpsc::channel();
    let recorder = RecordingProgress::new();
    let _handle = adapter.send(
        request,
        Box::new(ChannelSink(tx)),
        Some(Box::new(recorder.clone())),
    );
    let outcome = rx.recv_timeout(budget).ok();
    (outcome, recorder.samples())
}

/// Send one request, cancel it after `cancel_after`, and collect the completion (or `None` if the
/// adapter goes silent — the rule-9 break).
pub(crate) fn drive_cancel(
    adapter: &dyn Http,
    request: HttpRequest,
    cancel_after: Duration,
    budget: Duration,
) -> Option<Result<HttpResponse, HttpError>> {
    let (tx, rx) = mpsc::channel();
    let handle = adapter.send(request, Box::new(ChannelSink(tx)), None);
    thread::sleep(cancel_after);
    handle.cancel();
    rx.recv_timeout(budget).ok()
}
