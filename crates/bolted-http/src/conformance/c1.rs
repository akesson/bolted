//! C1 — the eleven fixed §7 rules as parameterised rows (feature-matrix §7).
//!
//! Each row drives the adapter-under-test against the real [`super::server::TestServer`]. Rules
//! that target adapter *behaviour* the mock could only simulate (3 stalled-body timeout, 4
//! https→http refusal, 7 gzip normalization, 10 SPKI pinning) genuinely exercise the socket mock's
//! synthesis, so a broken mock is caught. Every rule has a red-twin test (below) that flips exactly
//! one [`super::netmock::MockBehavior`] flag and watches the row go red.
//!
//! Rule 6's compile-time half is proven by M0's `compile_fail` doctest on `RequestHeaderName`;
//! this module's rule-6 row covers the runtime half (a permitted header is never silently dropped).
//! Rule 11 (upload progress) drives a real POST upload and pins monotonicity + terminal consistency
//! against a recording progress sink (M1.5 closed the M1 contract gap — the progress surface now
//! exists on [`crate::Http::send`]). No C1 row skips.
//!
//! Beyond the eleven §7 rules, [`extra_rows`] carries the **row-15 response-sink correspondence**
//! row (a `Memory` request yields a `Memory` outcome, a `File` request yields a `File` outcome with
//! matching contents) — a C1-adjacent matrix row, watched red the same way.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Cluster, ConformanceCtx, ConformanceRow, FailureReason, RowResult};
use super::{drive_cancel, drive_once, drive_with_progress};
use crate::error::HttpErrorKey;
use crate::request::{FileRef, HttpRequest, Method, PinSet, RequestBody, ResponseSink, Url};
use crate::response::{BodyOutcome, HttpResponse};

/// The eleven C1 rows.
#[must_use]
pub fn rows() -> &'static [ConformanceRow] {
    &ROWS
}

static ROWS: [ConformanceRow; 11] = [
    row("C1/rule-01-same-request-same-outcome", rule_01),
    row("C1/rule-02-timeout-vs-cancel-distinct", rule_02),
    row("C1/rule-03-stalled-body-times-out", rule_03),
    row("C1/rule-04-https-to-http-refused", rule_04),
    row("C1/rule-05-manual-if-none-match-304", rule_05),
    row("C1/rule-06-permitted-header-not-dropped", rule_06),
    row("C1/rule-07-gzip-decoded-invariant", rule_07),
    row("C1/rule-08-no-hidden-retry", rule_08),
    row("C1/rule-09-cancel-completes-cancelled", rule_09),
    row("C1/rule-10-pin-mismatch-typed-error", rule_10),
    row("C1/rule-11-upload-progress-monotone", rule_11),
];

const fn row(id: &'static str, check: fn(&ConformanceCtx) -> RowResult) -> ConformanceRow {
    ConformanceRow {
        id,
        cluster: Cluster::C1Rule,
        check,
    }
}

// --- helpers --------------------------------------------------------------------------

fn http_url(ctx: &ConformanceCtx, path: &str) -> Result<Url, RowResult> {
    Url::cleartext_dev(&ctx.endpoints.http(path))
        .map_err(|_| RowResult::Fail(FailureReason::NoCompletion))
}

fn https_url(ctx: &ConformanceCtx, path: &str) -> Result<Url, RowResult> {
    Url::https(&ctx.endpoints.https(path)).map_err(|_| RowResult::Fail(FailureReason::NoCompletion))
}

fn memory(resp: &HttpResponse) -> Option<&[u8]> {
    match resp.body() {
        BodyOutcome::Memory(b) => Some(b),
        _ => None,
    }
}

const BUDGET: Duration = Duration::from_secs(5);

// --- the rules ------------------------------------------------------------------------

/// Rule 1: same request ⇒ same typed response on every drive (determinism).
fn rule_01(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/ok") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let mk = || HttpRequest::builder(Method::Get, url.clone(), Duration::from_secs(5)).build();
    let a = drive_once(ctx.factory.new_adapter().as_ref(), mk(), BUDGET);
    let b = drive_once(ctx.factory.new_adapter().as_ref(), mk(), BUDGET);
    match (a, b) {
        (Some(Ok(ra)), Some(Ok(rb))) => {
            if ra.status() == rb.status() && memory(&ra) == memory(&rb) {
                RowResult::Pass
            } else {
                RowResult::Fail(FailureReason::NotDeterministic)
            }
        }
        (Some(Err(e)), _) | (_, Some(Err(e))) => {
            RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() })
        }
        _ => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 2: timeout and cancel are distinct keys.
fn rule_02(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/stall") {
        Ok(u) => u,
        Err(e) => return e,
    };
    // Deadline path → Timeout.
    let timeout_req =
        HttpRequest::builder(Method::Get, url.clone(), Duration::from_millis(300)).build();
    let timeout_key = match drive_once(ctx.factory.new_adapter().as_ref(), timeout_req, BUDGET) {
        Some(Err(e)) => e.key(),
        Some(Ok(r)) => {
            return RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
                expected: HttpErrorKey::Timeout,
                status: r.status().as_u16(),
            });
        }
        None => return RowResult::Fail(FailureReason::NoCompletion),
    };
    // Cancel path → Cancelled.
    let cancel_req = HttpRequest::builder(Method::Get, url, Duration::from_secs(30)).build();
    let cancel_key = match drive_cancel(
        ctx.factory.new_adapter().as_ref(),
        cancel_req,
        Duration::from_millis(200),
        BUDGET,
    ) {
        Some(Err(e)) => e.key(),
        Some(Ok(r)) => {
            return RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
                expected: HttpErrorKey::Cancelled,
                status: r.status().as_u16(),
            });
        }
        None => return RowResult::Fail(FailureReason::NoCompletion),
    };
    if timeout_key == cancel_key {
        RowResult::Fail(FailureReason::KeysNotDistinct { key: timeout_key })
    } else {
        RowResult::Pass
    }
}

/// Rule 3: a stalled body yields `Timeout` ≤ deadline + ε (never a hang).
#[allow(clippy::disallowed_methods)] // measuring elapsed against the deadline; harness code
fn rule_03(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/stall") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let deadline = Duration::from_millis(300);
    let req = HttpRequest::builder(Method::Get, url, deadline).build();
    let start = std::time::Instant::now();
    let outcome = drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET);
    let elapsed = start.elapsed();
    match outcome {
        Some(Err(e)) if e.key() == HttpErrorKey::Timeout => {
            // ε is generous (watchdog polls at 10 ms; TLS/thread scheduling aside).
            if elapsed <= deadline + Duration::from_secs(2) {
                RowResult::Pass
            } else {
                RowResult::Fail(FailureReason::WrongErrorKey {
                    expected: HttpErrorKey::Timeout,
                    got: HttpErrorKey::Timeout,
                })
            }
        }
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: HttpErrorKey::Timeout,
            got: e.key(),
        }),
        Some(Ok(r)) => RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
            expected: HttpErrorKey::Timeout,
            status: r.status().as_u16(),
        }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 4: an https→http redirect is refused with the typed `InsecureRedirect` error.
fn rule_04(ctx: &ConformanceCtx) -> RowResult {
    let target = ctx.endpoints.http("/echo");
    let url = match https_url(ctx, &format!("/redirect-insecure?to={target}")) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .pins(PinSet::new(vec![ctx.endpoints.good_pin()]))
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Err(e)) if e.key() == HttpErrorKey::InsecureRedirect => RowResult::Pass,
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: HttpErrorKey::InsecureRedirect,
            got: e.key(),
        }),
        Some(Ok(r)) => RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
            expected: HttpErrorKey::InsecureRedirect,
            status: r.status().as_u16(),
        }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 5: a manual `If-None-Match` yields a real 304.
fn rule_05(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/etag") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let name = crate::header::RequestHeaderName::from_static("If-None-Match");
    let value = match crate::header::HeaderValue::from_text("\"v1\"") {
        Ok(v) => v,
        Err(_) => return RowResult::Fail(FailureReason::NoCompletion),
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .header(name, value)
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Ok(r)) if r.status().as_u16() == 304 => RowResult::Pass,
        Some(Ok(r)) => RowResult::Fail(FailureReason::UnexpectedStatus {
            expected: 304,
            got: r.status().as_u16(),
        }),
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 6 (runtime half): a permitted header is transmitted, never silently dropped.
fn rule_06(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/echo") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let name = crate::header::RequestHeaderName::from_static("X-Trace-Id");
    let value = match crate::header::HeaderValue::from_text("hello-42") {
        Ok(v) => v,
        Err(_) => return RowResult::Fail(FailureReason::NoCompletion),
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .header(name, value)
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Ok(r)) => match r.headers().get("x-echo-x-trace-id") {
            Some(v) if v.as_bytes() == b"hello-42" => RowResult::Pass,
            _ => RowResult::Fail(FailureReason::MissingHeader { name: "x-trace-id" }),
        },
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 7: a gzip body is decoded to the identical plaintext bytes, and `content_length` is honest
/// under decoding — `None` or the decoded length, never the compressed transport figure (§5.12).
fn rule_07(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/gzip") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    let decoded_len = super::server::GZIP_PLAINTEXT.len() as u64;
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Ok(r)) => match memory(&r) {
            Some(body) if body == super::server::GZIP_PLAINTEXT => match r.content_length() {
                // Honest: absent, or exactly the decoded length. A compressed figure is a lie.
                None => RowResult::Pass,
                Some(n) if n == decoded_len => RowResult::Pass,
                Some(n) => RowResult::Fail(FailureReason::DishonestContentLength {
                    got: n,
                    decoded: decoded_len,
                }),
            },
            _ => RowResult::Fail(FailureReason::WrongBody),
        },
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 8: a mid-flight failure surfaces the typed error — no hidden request-level retry. The
/// `/flaky` endpoint fails attempt 1 and succeeds attempt 2, so a retrying adapter surfaces a
/// success where the contract requires `Transport`.
fn rule_08(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/flaky") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Err(e)) if e.key() == HttpErrorKey::Transport => RowResult::Pass,
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: HttpErrorKey::Transport,
            got: e.key(),
        }),
        // A success here means the adapter retried the failed attempt (hidden retry).
        Some(Ok(_)) => RowResult::Fail(FailureReason::HiddenRetry { connections: 2 }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 9: a cancelled request always completes with `Cancelled` (never silence).
fn rule_09(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/stall") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(30)).build();
    match drive_cancel(
        ctx.factory.new_adapter().as_ref(),
        req,
        Duration::from_millis(200),
        BUDGET,
    ) {
        Some(Err(e)) if e.key() == HttpErrorKey::Cancelled => RowResult::Pass,
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: HttpErrorKey::Cancelled,
            got: e.key(),
        }),
        Some(Ok(r)) => RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
            expected: HttpErrorKey::Cancelled,
            status: r.status().as_u16(),
        }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 10: a pin mismatch yields the typed `PinMismatch` error — and a matching pin still
/// succeeds (so the check is not vacuously always-fail).
fn rule_10(ctx: &ConformanceCtx) -> RowResult {
    // Positive: the correct pin succeeds.
    let ok_url = match https_url(ctx, "/ok") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let ok_req = HttpRequest::builder(Method::Get, ok_url, Duration::from_secs(5))
        .pins(PinSet::new(vec![ctx.endpoints.good_pin()]))
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), ok_req, BUDGET) {
        Some(Ok(r)) if r.status().is_success() => {}
        Some(Ok(r)) => {
            return RowResult::Fail(FailureReason::UnexpectedStatus {
                expected: 200,
                got: r.status().as_u16(),
            });
        }
        Some(Err(e)) => {
            return RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() });
        }
        None => return RowResult::Fail(FailureReason::NoCompletion),
    }
    // Negative: the wrong pin is a typed PinMismatch.
    let bad_url = match https_url(ctx, "/ok") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let bad_req = HttpRequest::builder(Method::Get, bad_url, Duration::from_secs(5))
        .pins(PinSet::new(vec![ctx.endpoints.wrong_pin()]))
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), bad_req, BUDGET) {
        Some(Err(e)) if e.key() == HttpErrorKey::PinMismatch => RowResult::Pass,
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: HttpErrorKey::PinMismatch,
            got: e.key(),
        }),
        Some(Ok(r)) => RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
            expected: HttpErrorKey::PinMismatch,
            status: r.status().as_u16(),
        }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Rule 11: upload progress is monotone per attempt and terminally consistent with the completion.
/// Drives a real POST upload with a recording progress sink; the suite never asserts wire-truth,
/// only monotonicity + the terminal equality with the known body length (§5.9, row 14).
fn rule_11(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/echo") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let body = vec![b'u'; 256];
    let body_len = body.len() as u64;
    let req = HttpRequest::builder(Method::Post, url, Duration::from_secs(5))
        .body(RequestBody::Bytes(body))
        .build();
    let (outcome, samples) = drive_with_progress(ctx.factory.new_adapter().as_ref(), req, BUDGET);
    match outcome {
        Some(Ok(_)) => judge_progress(&samples, body_len),
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Judge a recorded progress sequence against rule 11: monotone non-decreasing `sent`, and a final
/// `sent` equal to the known body length (terminal consistency). Never compares to wire bytes.
fn judge_progress(samples: &[(u64, Option<u64>)], body_len: u64) -> RowResult {
    let mut prev = 0u64;
    for &(sent, _total) in samples {
        if sent < prev {
            return RowResult::Fail(FailureReason::ProgressNotMonotone { prev, got: sent });
        }
        prev = sent;
    }
    let final_sent = samples.last().map(|&(s, _)| s).unwrap_or(0);
    if final_sent == body_len {
        RowResult::Pass
    } else {
        RowResult::Fail(FailureReason::ProgressNotTerminal {
            got: final_sent,
            expected: body_len,
        })
    }
}

/// The C1-adjacent matrix rows (the eleven §7 rules stay in [`rows`]): the row-15 response-sink
/// correspondence row and the M4 redirect-trace row (the `final_url` + `hops` observables).
#[must_use]
pub fn extra_rows() -> &'static [ConformanceRow] {
    &SINK_ROWS
}

static SINK_ROWS: [ConformanceRow; 2] = [
    row(
        "C1/row-15-response-sink-correspondence",
        row_15_sink_correspondence,
    ),
    row(
        "C1/row-redirect-trace-final-url-and-hops",
        redirect_trace_correspondence,
    ),
];

/// The redirect-trace row (M4 — the redirect-trace blind spot fix): a followed `302` chain reports
/// the chain's tail as `final_url` and records every intermediate hop. Until M4 **no** rule
/// referenced either observable, so an adapter that followed redirects yet misreported the final URL
/// or dropped the hop trace passed the entire suite. Drives `/redirect-chain?n=2` — two `302` hops,
/// then a terminal `200`.
fn redirect_trace_correspondence(ctx: &ConformanceCtx) -> RowResult {
    let url = match http_url(ctx, "/redirect-chain?n=2") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    match drive_once(ctx.factory.new_adapter().as_ref(), req, BUDGET) {
        Some(Ok(r)) => {
            if r.status().as_u16() != 200 {
                return RowResult::Fail(FailureReason::UnexpectedStatus {
                    expected: 200,
                    got: r.status().as_u16(),
                });
            }
            // Two 302 hops precede the terminal 200.
            if r.hops().len() != 2 {
                return RowResult::Fail(FailureReason::WrongHopTrace {
                    got: r.hops().len(),
                    expected: 2,
                });
            }
            // The terminal URL is the chain's tail (`n=0`), never the original request (`n=2`) or a
            // pre-terminal hop.
            if !r.final_url().as_str().contains("n=0") {
                return RowResult::Fail(FailureReason::WrongFinalUrl);
            }
            RowResult::Pass
        }
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

/// Unique-per-drive suffix for the file-sink temp path (avoids collisions across parallel rows).
static SINK_NONCE: AtomicU64 = AtomicU64::new(0);

/// Row 15: the delivered [`BodyOutcome`] corresponds to the requested [`ResponseSink`] — a `Memory`
/// request yields `Memory`, a `File(path)` request yields `File(path)` with the body written there.
fn row_15_sink_correspondence(ctx: &ConformanceCtx) -> RowResult {
    // Memory sink ⇒ Memory outcome.
    let mem_url = match http_url(ctx, "/ok") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let mem_req = HttpRequest::builder(Method::Get, mem_url, Duration::from_secs(5))
        .response_sink(ResponseSink::Memory)
        .build();
    match drive_once(ctx.factory.new_adapter().as_ref(), mem_req, BUDGET) {
        Some(Ok(r)) if matches!(r.body(), BodyOutcome::Memory(_)) => {}
        Some(Ok(_)) => return RowResult::Fail(FailureReason::WrongSink),
        Some(Err(e)) => {
            return RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() });
        }
        None => return RowResult::Fail(FailureReason::NoCompletion),
    }

    // File sink ⇒ File outcome at that path, with the response body written there.
    let file_url = match http_url(ctx, "/ok") {
        Ok(u) => u,
        Err(e) => return e,
    };
    let n = SINK_NONCE.fetch_add(1, Ordering::SeqCst);
    let path =
        std::env::temp_dir().join(format!("bolted-http-sink-{}-{n}.bin", std::process::id()));
    let file_req = HttpRequest::builder(Method::Get, file_url, Duration::from_secs(5))
        .response_sink(ResponseSink::File(FileRef::new(path.clone())))
        .build();
    let result = match drive_once(ctx.factory.new_adapter().as_ref(), file_req, BUDGET) {
        Some(Ok(r)) => match r.body() {
            BodyOutcome::File(got) if got.as_path() == path.as_path() => match std::fs::read(&path)
            {
                Ok(bytes) if bytes == b"ok" => RowResult::Pass,
                _ => RowResult::Fail(FailureReason::WrongSink),
            },
            _ => RowResult::Fail(FailureReason::WrongSink),
        },
        Some(Err(e)) => RowResult::Fail(FailureReason::ExpectedSuccessGotError { got: e.key() }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    };
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::netmock::{MockBehavior, SocketMockFactory};
    use crate::conformance::server::TestServer;
    use crate::conformance::{ConformanceCtx, Endpoints, run};

    fn harness() -> (TestServer, Endpoints) {
        let server = TestServer::start().expect("server starts");
        let endpoints = Endpoints::from_server(&server);
        (server, endpoints)
    }

    fn correct_factory(ep: &Endpoints) -> SocketMockFactory {
        SocketMockFactory::correct(ep.good_spki())
    }

    #[test]
    fn eleven_rows_registered() {
        assert_eq!(rows().len(), 11);
    }

    #[test]
    fn correct_mock_passes_all_c1() {
        let (_server, ep) = harness();
        let factory = correct_factory(&ep);
        let ctx = ConformanceCtx {
            factory: &factory,
            endpoints: &ep,
        };
        // The eleven §7 rules plus the C1-adjacent row-15 sink row all pass; none skips.
        for (id, result) in run(rows(), &ctx)
            .iter()
            .chain(run(extra_rows(), &ctx).iter())
        {
            assert_eq!(*result, RowResult::Pass, "row {id} did not pass");
        }
    }

    // --- the red twins: one break per rule, watched red ---------------------------------

    fn twin(ep: &Endpoints, mutate: impl FnOnce(&mut MockBehavior)) -> SocketMockFactory {
        SocketMockFactory::correct(ep.good_spki()).with_behavior(mutate)
    }

    fn ctx_of<'a>(f: &'a SocketMockFactory, ep: &'a Endpoints) -> ConformanceCtx<'a> {
        ConformanceCtx {
            factory: f,
            endpoints: ep,
        }
    }

    #[test]
    fn rule_01_red_when_nondeterministic() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.deterministic = false);
        assert!(matches!(
            rule_01(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::NotDeterministic)
        ));
    }

    #[test]
    fn rule_02_red_when_cancel_conflated_with_timeout() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.classify_cancel = false);
        assert!(matches!(
            rule_02(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::KeysNotDistinct { .. })
        ));
    }

    #[test]
    fn rule_03_red_without_deadline() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.arm_deadline = false);
        // No deadline ⇒ the stalled body hangs ⇒ no completion in budget.
        assert!(matches!(
            rule_03(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::NoCompletion)
        ));
    }

    #[test]
    fn rule_04_red_when_following_insecure_redirect() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.refuse_insecure_redirect = false);
        assert!(matches!(
            rule_04(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::ExpectedErrorGotSuccess { .. })
        ));
    }

    #[test]
    fn rule_05_red_when_conditional_header_dropped() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.send_headers = false);
        assert!(matches!(
            rule_05(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::UnexpectedStatus { expected: 304, .. })
        ));
    }

    #[test]
    fn rule_06_red_when_header_dropped() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.send_headers = false);
        assert!(matches!(
            rule_06(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::MissingHeader { .. })
        ));
    }

    #[test]
    fn rule_07_red_without_gzip_decode() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.decode_gzip = false);
        assert!(matches!(
            rule_07(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::WrongBody)
        ));
    }

    #[test]
    fn rule_08_red_when_retrying() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.retry_on_transport = true);
        assert!(matches!(
            rule_08(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::HiddenRetry { .. })
        ));
    }

    #[test]
    fn rule_09_red_when_silent_on_cancel() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honor_cancel = false);
        assert!(matches!(
            rule_09(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::NoCompletion)
        ));
    }

    #[test]
    fn rule_10_red_when_pinning_bypassed() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.check_pins = false);
        assert!(matches!(
            rule_10(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::ExpectedErrorGotSuccess { .. })
        ));
    }

    #[test]
    fn rule_07_red_when_content_length_lies_under_decoding() {
        // The body is still decoded correctly, but content_length reports the compressed figure.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honest_content_length = false);
        assert!(matches!(
            rule_07(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::DishonestContentLength { .. })
        ));
    }

    #[test]
    fn rule_11_red_when_progress_not_monotone() {
        // The naïve-wrapper break: a 100%-jump-then-drop sequence violates monotonicity.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honest_upload_progress = false);
        assert!(matches!(
            rule_11(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::ProgressNotMonotone { .. })
        ));
    }

    #[test]
    fn row_15_sink_red_when_file_sink_ignored() {
        // The sink-drop break: a File request comes back as Memory.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honor_file_sink = false);
        assert!(matches!(
            row_15_sink_correspondence(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::WrongSink)
        ));
    }

    #[test]
    fn redirect_trace_red_when_trace_dropped() {
        // M4 blind-spot fix: the trace-drop break reports the original request URL as final and
        // drops the hops. Before this row it survived the whole suite; now the hop count is wrong.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honest_redirect_trace = false);
        assert!(matches!(
            redirect_trace_correspondence(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::WrongHopTrace {
                got: 0,
                expected: 2
            })
        ));
    }

    #[test]
    fn rule_11_red_when_progress_stops_short() {
        // The forgot-the-last-chunk break: monotone but terminally inconsistent progress. This is
        // the positive control the monotonicity twin never provided for `ProgressNotTerminal`.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.terminal_upload_progress = false);
        assert!(matches!(
            rule_11(&ctx_of(&f, &ep)),
            RowResult::Fail(FailureReason::ProgressNotTerminal { .. })
        ));
    }
}
