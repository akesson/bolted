//! The full conformance suite (C1 + C2 + C3) run against the reqwest reference adapter, plus the
//! step-24 M3 L2/L3/L4 verdicts (feature-matrix S-LX). This is the reference adapter the suite is
//! debugged against *after* the mock (the one-implementor lesson).
//!
//! Wired into `mise run check` as `cargo test -p bolted-http-linux` (see mise.toml). Every C1 rule
//! (incl. the row-15 sink row), every reachable C2 key, and the C3 divergence table pass here; the
//! adapter-specific pin expectation is additionally watched **red** once (`pin_config_break_is_red`).

use std::time::Duration;

use bolted_http::HttpErrorKey;
use bolted_http::capability::{Http, Metrics, StreamingHttp};
use bolted_http::conformance::server::TestServer;
use bolted_http::conformance::{
    AdapterFactory, ConformanceCtx, Endpoints, RowResult, c1, c2, c3, run, stream,
};
use bolted_http::request::{HttpRequest, Method, PinSet, Url};

use bolted_http_linux::{LinuxHttp, LinuxHttpConfig, StreamFault};

/// A conformance factory over the reqwest adapter, trusting the test server's good cert as its real
/// chain-verification anchor (the CFG trust-roots seam; production wires system/Mozilla roots here).
struct LinuxFactory {
    http: LinuxHttp,
}

impl LinuxFactory {
    fn new(endpoints: &Endpoints) -> Self {
        let config = LinuxHttpConfig::with_trust_anchor(endpoints.good_cert_der().to_vec());
        LinuxFactory {
            http: LinuxHttp::new(config).expect("adapter builds"),
        }
    }

    /// A factory with SPKI pinning disabled — the scoped red-twin (the pin no longer bites).
    fn with_pinning_disabled(endpoints: &Endpoints) -> Self {
        let config = LinuxHttpConfig {
            enforce_pins: false,
            ..LinuxHttpConfig::with_trust_anchor(endpoints.good_cert_der().to_vec())
        };
        LinuxFactory {
            http: LinuxHttp::new(config).expect("adapter builds"),
        }
    }

    /// A factory with a streaming fault injected — the scoped red-twin for the streaming rows.
    fn with_stream_fault(endpoints: &Endpoints, fault: StreamFault) -> Self {
        let config = LinuxHttpConfig {
            stream_fault: fault,
            ..LinuxHttpConfig::with_trust_anchor(endpoints.good_cert_der().to_vec())
        };
        LinuxFactory {
            http: LinuxHttp::new(config).expect("adapter builds"),
        }
    }
}

impl AdapterFactory for LinuxFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(self.http.clone())
    }

    fn metrics(&self) -> Option<Box<dyn Metrics>> {
        // reqwest's honest tier is whole-request (§5.13) — present, tier B.
        Some(Box::new(self.http.clone()))
    }

    fn streaming(&self) -> Option<Box<dyn StreamingHttp>> {
        // The reqwest adapter streams response bodies (rows 12/13, streaming-seam §3b).
        Some(Box::new(self.http.clone()))
    }
    // The priority hint (row 12 field) is a uniform advisory field, not a capability trait (ruled
    // Q10) — reqwest carries the data and legally ignores it, so there is no C3 column for it.
}

fn harness() -> (TestServer, Endpoints) {
    let server = TestServer::start().expect("server starts");
    let endpoints = Endpoints::from_server(&server);
    (server, endpoints)
}

/// C1 — the eleven §7 rules plus the C1-adjacent row-15 response-sink row, all green (rules 3/4/7/10
/// exercise real adapter synthesis: deadline race, https→http refusal, gzip decode, SPKI pinning).
#[test]
fn c1_all_rules_pass_against_reqwest_adapter() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    for (id, result) in run(c1::rows(), &ctx)
        .into_iter()
        .chain(run(c1::extra_rows(), &ctx))
    {
        assert_eq!(result, RowResult::Pass, "C1 row {id} did not pass");
    }
}

/// C2 — every reachable taxonomy key has its positive control produced by the adapter.
/// `PermissionDenied` stays adapter-only-recorded (no host control) — its absence here is correct.
#[test]
fn c2_every_reachable_key_produced() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    for (id, result) in run(c2::rows(), &ctx) {
        assert_eq!(
            result,
            RowResult::Pass,
            "C2 row {id} did not produce its key"
        );
    }
}

/// C3 — the divergence table generated from the adapter's capability self-report: metrics present
/// at the WholeRequest tier (identical to the socket mock's honest shape). The priority hint is no
/// longer a divergent capability (ruled Q10, uniform advisory field), so it has no row here.
#[test]
fn c3_divergence_matrix_is_pinned() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);
    const EXPECTED: &str = "\
capability     | presence
---------------+-----------------------
metrics        | present (WholeRequest)";
    assert_eq!(c3::divergence(&factory).render(), EXPECTED);
}

/// Rows 12 & 13 (streaming, rules 12–13): the reqwest adapter streams the `/chunked` body into the
/// driver-owned ingest under a slow consumer. Row 12 = complete body + verified terminal total;
/// row 13 = exactly one terminal. Both green against the conformant adapter.
#[test]
fn streaming_rows_pass_against_reqwest_adapter() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    for (id, result) in run(stream::rows(), &ctx) {
        assert_eq!(result, RowResult::Pass, "streaming row {id} did not pass");
    }
}

/// Row 12 watched **red** on the reqwest adapter: with `DropChunk`, the adapter skips one body
/// chunk while still counting its bytes toward the declared total, so the completeness gate fires
/// and the terminal is a typed failure — the truncation row 12 forbids.
#[test]
fn streaming_row_12_red_on_dropped_chunk() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::with_stream_fault(&endpoints, StreamFault::DropChunk);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    let results = run(stream::rows(), &ctx);
    let row_12 = results
        .iter()
        .find(|(id, _)| id.contains("row-12"))
        .expect("row-12 present");
    assert!(
        matches!(row_12.1, RowResult::Fail(_)),
        "row 12 must be red with a dropped chunk, got {:?}",
        row_12.1
    );
}

/// Row 13 watched **red** on the reqwest adapter: with `SkipTerminal`, the adapter delivers every
/// chunk but never sends the terminal, so no `BodyEnd` arrives — the missing-terminal break.
#[test]
fn streaming_row_13_red_on_missing_terminal() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::with_stream_fault(&endpoints, StreamFault::SkipTerminal);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    let results = run(stream::rows(), &ctx);
    let row_13 = results
        .iter()
        .find(|(id, _)| id.contains("row-13"))
        .expect("row-13 present");
    assert!(
        matches!(row_13.1, RowResult::Fail(_)),
        "row 13 must be red with a missing terminal, got {:?}",
        row_13.1
    );
}

/// **L2** — the pinning verdict, exercised directly: a matching pin against the real (chain-verified)
/// server cert succeeds; a wrong pin is `PinMismatch`; and the untrusted endpoint is rejected as a
/// `Tls` failure by *real* chain verification (not a pin), proving the trust decision is genuine.
#[test]
fn l2_pinning_and_real_chain_verification() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);

    // Matching pin over a really-verified chain ⇒ success.
    let ok = drive(
        &factory,
        &endpoints.https("/ok"),
        true,
        Some(PinSet::new(vec![endpoints.good_pin()])),
    );
    assert!(
        matches!(ok, Some(Ok(ref r)) if r.status().is_success()),
        "good pin should succeed"
    );

    // Wrong pin over the same verified chain ⇒ PinMismatch (pins are ANDed on top of trust).
    let bad = drive(
        &factory,
        &endpoints.https("/ok"),
        true,
        Some(PinSet::new(vec![endpoints.wrong_pin()])),
    );
    assert!(
        matches!(bad, Some(Err(ref e)) if e.key() == HttpErrorKey::PinMismatch),
        "wrong pin should be PinMismatch, got {bad:?}"
    );

    // Untrusted cert, no pins ⇒ rejected by REAL chain verification as Tls (not a pin).
    let untrusted = drive(&factory, &endpoints.https_untrusted("/ok"), true, None);
    assert!(
        matches!(untrusted, Some(Err(ref e)) if e.key() == HttpErrorKey::Tls),
        "untrusted cert should be Tls, got {untrusted:?}"
    );
}

/// **L3** — retry-off proven by the `/flaky` control: attempt 1 truncates, and the adapter surfaces
/// the typed `Transport` error rather than silently retrying. The server saw exactly one connection.
#[test]
fn l3_no_hidden_retry_on_flaky() {
    let (server, endpoints) = harness();
    let factory = LinuxFactory::new(&endpoints);
    let outcome = drive(&factory, &endpoints.http("/flaky"), false, None);
    assert!(
        matches!(outcome, Some(Err(ref e)) if e.key() == HttpErrorKey::Transport),
        "flaky attempt 1 must surface Transport, got {outcome:?}"
    );
    assert_eq!(
        server.hits("/flaky"),
        1,
        "the adapter must not re-send a request that reached the wire (retry-off)"
    );
}

/// The scoped adapter-specific red: with SPKI pin enforcement disabled, the rule-10 pin-mismatch row
/// goes **red** (a wrong pin now succeeds), while a pin-independent row (rule 1) stays green — the
/// break is real and targeted (full mutation coverage is M4).
#[test]
fn pin_config_break_is_red() {
    let (_server, endpoints) = harness();
    let factory = LinuxFactory::with_pinning_disabled(&endpoints);
    let ctx = ConformanceCtx {
        factory: &factory,
        endpoints: &endpoints,
    };
    let results = run(c1::rows(), &ctx);
    let pin_row = results
        .iter()
        .find(|(id, _)| id.contains("rule-10"))
        .expect("rule-10 present");
    let base_row = results
        .iter()
        .find(|(id, _)| id.contains("rule-01"))
        .expect("rule-01 present");
    assert!(
        matches!(pin_row.1, RowResult::Fail(_)),
        "rule-10 must be red with pinning disabled, got {:?}",
        pin_row.1
    );
    assert_eq!(
        base_row.1,
        RowResult::Pass,
        "the break must be targeted (rule-01 stays green)"
    );
}

// --- a small driver shared by the L2/L3 tests (mirrors the harness's private `drive_once`) --------

use std::sync::mpsc;

use bolted_http::HttpError;
use bolted_http::capability::{CompletionSink, RequestHandle, UploadProgressSink};
use bolted_http::response::HttpResponse;

struct ChannelSink(mpsc::Sender<Result<HttpResponse, HttpError>>);
impl CompletionSink for ChannelSink {
    fn complete(self: Box<Self>, outcome: Result<HttpResponse, HttpError>) {
        let _ = self.0.send(outcome);
    }
}

/// Drive one request (https or cleartext) with optional pins and collect its single completion.
fn drive(
    factory: &LinuxFactory,
    url: &str,
    https: bool,
    pins: Option<PinSet>,
) -> Option<Result<HttpResponse, HttpError>> {
    let url = if https {
        Url::https(url).expect("https url")
    } else {
        Url::cleartext_dev(url).expect("cleartext url")
    };
    let mut builder = HttpRequest::builder(Method::Get, url, Duration::from_secs(5));
    if let Some(pins) = pins {
        builder = builder.pins(pins);
    }
    let adapter = factory.new_adapter();
    let (tx, rx) = mpsc::channel();
    let _handle: RequestHandle = adapter.send(
        builder.build(),
        Box::new(ChannelSink(tx)),
        None::<Box<dyn UploadProgressSink>>,
    );
    rx.recv_timeout(Duration::from_secs(6)).ok()
}
