//! C2 — the error-taxonomy matrix (feature-matrix §5.15). Every typed [`HttpErrorKey`] is either
//! reachable via a test-server endpoint with a **positive control** (an endpoint that provably
//! produces it against the socket mock), or explicitly recorded as adapter-only / contract-gap with
//! a justification — never silently skipped (a key with no reachable control is a green needle).
//!
//! Completeness is compiler-enforced: [`reachability`] is an exhaustive match over `HttpErrorKey`
//! (allowed within the defining crate despite `#[non_exhaustive]`), so a new key cannot be added
//! without classifying it here.

use std::time::Duration;

use super::{Cluster, ConformanceCtx, ConformanceRow, FailureReason, RowResult};
use super::{drive_cancel, drive_once};
use crate::error::HttpErrorKey;
use crate::request::{FileRef, HttpRequest, Method, PinSet, ResponseSink, Url};

/// Every taxonomy key, in a stable order. The [`reachability`] match is the real completeness
/// guard; this list drives the coverage test.
pub const ALL_KEYS: &[HttpErrorKey] = &[
    HttpErrorKey::Timeout,
    HttpErrorKey::Cancelled,
    HttpErrorKey::PermissionDenied,
    HttpErrorKey::PinMismatch,
    HttpErrorKey::Tls,
    HttpErrorKey::NameResolution,
    HttpErrorKey::Connect,
    HttpErrorKey::Transport,
    HttpErrorKey::InsecureRedirect,
    HttpErrorKey::TooManyRedirects,
    HttpErrorKey::Io,
    HttpErrorKey::StreamOverflow,
];

/// How a taxonomy key is covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reachability {
    /// A test-server endpoint provably produces the key (a positive control C2 row exists).
    Reachable,
    /// Only a real platform adapter can produce it (OS-level cause); recorded, not skipped.
    AdapterOnly(&'static str),
    /// The M0 contract has no surface to reach it yet; recorded, not skipped.
    ContractGap(&'static str),
}

/// Classify every key. Exhaustive (no wildcard) so a new `HttpErrorKey` variant forces a decision.
#[must_use]
pub fn reachability(key: HttpErrorKey) -> Reachability {
    match key {
        HttpErrorKey::Timeout => Reachability::Reachable,
        HttpErrorKey::Cancelled => Reachability::Reachable,
        HttpErrorKey::PinMismatch => Reachability::Reachable,
        HttpErrorKey::Tls => Reachability::Reachable,
        HttpErrorKey::NameResolution => Reachability::Reachable,
        HttpErrorKey::Connect => Reachability::Reachable,
        HttpErrorKey::Transport => Reachability::Reachable,
        HttpErrorKey::InsecureRedirect => Reachability::Reachable,
        HttpErrorKey::TooManyRedirects => Reachability::Reachable,
        // M1.5: the request-side response-sink selector ([`crate::ResponseSink::File`]) makes a
        // file-sink write failure reachable — drive a File sink at an unwritable path.
        HttpErrorKey::Io => Reachability::Reachable,
        HttpErrorKey::PermissionDenied => Reachability::AdapterOnly(
            "OS local-network permission (Android 16→17 EPERM / Apple Local Network prompt): a host \
             test server cannot make the OS deny permission. Positive control lands in the Apple / \
             Android adapter suites (steps 25/26).",
        ),
        // Step 27 M2: the adapter-driven control now exists — the slow-consumer completeness row
        // (feature-matrix §7 rule 12) drives StreamOverflow through a `StreamingHttp` adapter whose
        // producer ignores the pushed `Pause` under a slow consumer, overflowing the bounded ring
        // (crate::conformance::stream, `StreamFault::IgnorePause`). It stays classified here rather
        // than as a normal positive-control row because **no conformant adapter ever produces it**:
        // it is the typed failure a *broken* adapter earns, reachable only under fault injection (a
        // scoped red twin), so a `Reachable` C2 row asserting a correct adapter yields it would be a
        // lie. Recorded, with the control named, not skipped.
        HttpErrorKey::StreamOverflow => Reachability::ContractGap(
            "produced by the core-side streaming ring (crate::stream::BodyStream); the adapter-driven \
             control is step-27 M2's row 12 back-pressure twin (StreamFault::IgnorePause), which \
             overflows the ring under a slow consumer. No conformant adapter produces it.",
        ),
    }
}

/// The reachable C2 rows — one positive control per reachable key.
#[must_use]
pub fn rows() -> &'static [ConformanceRow] {
    &ROWS
}

static ROWS: [ConformanceRow; 10] = [
    row("C2/key-timeout", HttpErrorKey::Timeout, c2_timeout),
    row("C2/key-cancelled", HttpErrorKey::Cancelled, c2_cancelled),
    row(
        "C2/key-pin-mismatch",
        HttpErrorKey::PinMismatch,
        c2_pin_mismatch,
    ),
    row("C2/key-tls", HttpErrorKey::Tls, c2_tls),
    row(
        "C2/key-name-resolution",
        HttpErrorKey::NameResolution,
        c2_name_resolution,
    ),
    row("C2/key-connect", HttpErrorKey::Connect, c2_connect),
    row("C2/key-transport", HttpErrorKey::Transport, c2_transport),
    row(
        "C2/key-insecure-redirect",
        HttpErrorKey::InsecureRedirect,
        c2_insecure_redirect,
    ),
    row(
        "C2/key-too-many-redirects",
        HttpErrorKey::TooManyRedirects,
        c2_too_many_redirects,
    ),
    row("C2/key-io", HttpErrorKey::Io, c2_io),
];

const fn row(
    id: &'static str,
    _key: HttpErrorKey,
    check: fn(&ConformanceCtx) -> RowResult,
) -> ConformanceRow {
    ConformanceRow {
        id,
        cluster: Cluster::C2Taxonomy,
        check,
    }
}

const BUDGET: Duration = Duration::from_secs(5);

/// Drive `request` and assert its outcome is the typed error `key` (the positive-control judgement,
/// shared by the row fns and their red twins).
pub(crate) fn drive_expect_key(
    ctx: &ConformanceCtx,
    request: HttpRequest,
    key: HttpErrorKey,
    cancel_after: Option<Duration>,
) -> RowResult {
    let adapter = ctx.factory.new_adapter();
    let outcome = match cancel_after {
        Some(d) => drive_cancel(adapter.as_ref(), request, d, BUDGET),
        None => drive_once(adapter.as_ref(), request, BUDGET),
    };
    match outcome {
        Some(Err(e)) if e.key() == key => RowResult::Pass,
        Some(Err(e)) => RowResult::Fail(FailureReason::WrongErrorKey {
            expected: key,
            got: e.key(),
        }),
        Some(Ok(r)) => RowResult::Fail(FailureReason::ExpectedErrorGotSuccess {
            expected: key,
            status: r.status().as_u16(),
        }),
        None => RowResult::Fail(FailureReason::NoCompletion),
    }
}

fn err_row(url: Result<Url, ()>) -> Result<Url, RowResult> {
    url.map_err(|()| RowResult::Fail(FailureReason::NoCompletion))
}

fn c2_timeout(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::cleartext_dev(&ctx.endpoints.http("/stall")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_millis(300)).build();
    drive_expect_key(ctx, req, HttpErrorKey::Timeout, None)
}

fn c2_cancelled(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::cleartext_dev(&ctx.endpoints.http("/stall")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(30)).build();
    drive_expect_key(
        ctx,
        req,
        HttpErrorKey::Cancelled,
        Some(Duration::from_millis(200)),
    )
}

fn c2_pin_mismatch(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::https(&ctx.endpoints.https("/ok")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .pins(PinSet::new(vec![ctx.endpoints.wrong_pin()]))
        .build();
    drive_expect_key(ctx, req, HttpErrorKey::PinMismatch, None)
}

fn c2_tls(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::https(&ctx.endpoints.https_untrusted("/ok")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    drive_expect_key(ctx, req, HttpErrorKey::Tls, None)
}

fn c2_name_resolution(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::https(ctx.endpoints.unresolvable()).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    drive_expect_key(ctx, req, HttpErrorKey::NameResolution, None)
}

fn c2_connect(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::cleartext_dev(ctx.endpoints.closed_port()).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    drive_expect_key(ctx, req, HttpErrorKey::Connect, None)
}

fn c2_transport(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::cleartext_dev(&ctx.endpoints.http("/truncate")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    drive_expect_key(ctx, req, HttpErrorKey::Transport, None)
}

fn c2_insecure_redirect(ctx: &ConformanceCtx) -> RowResult {
    let target = ctx.endpoints.http("/echo");
    let url = match err_row(
        Url::https(
            &ctx.endpoints
                .https(&format!("/redirect-insecure?to={target}")),
        )
        .map_err(|_| ()),
    ) {
        Ok(u) => u,
        Err(e) => return e,
    };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .pins(PinSet::new(vec![ctx.endpoints.good_pin()]))
        .build();
    drive_expect_key(ctx, req, HttpErrorKey::InsecureRedirect, None)
}

fn c2_too_many_redirects(ctx: &ConformanceCtx) -> RowResult {
    let url =
        match err_row(Url::cleartext_dev(&ctx.endpoints.http("/redirect-loop")).map_err(|_| ())) {
            Ok(u) => u,
            Err(e) => return e,
        };
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
    drive_expect_key(ctx, req, HttpErrorKey::TooManyRedirects, None)
}

/// The `Io` positive control (M1.5): a successful response driven into a [`ResponseSink::File`] at
/// an **unwritable** path (a parent directory that does not exist) must surface `Io`, not success.
fn c2_io(ctx: &ConformanceCtx) -> RowResult {
    let url = match err_row(Url::cleartext_dev(&ctx.endpoints.http("/ok")).map_err(|_| ())) {
        Ok(u) => u,
        Err(e) => return e,
    };
    // A path whose parent directory does not exist ⇒ the file write fails ⇒ Io.
    let unwritable = std::env::temp_dir()
        .join(format!("bolted-http-nonexistent-{}", std::process::id()))
        .join("out.bin");
    let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5))
        .response_sink(ResponseSink::File(FileRef::new(unwritable)))
        .build();
    drive_expect_key(ctx, req, HttpErrorKey::Io, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::netmock::{MockBehavior, SocketMockFactory};
    use crate::conformance::server::TestServer;
    use crate::conformance::{ConformanceCtx, Endpoints, RowResult, run};

    fn harness() -> (TestServer, Endpoints) {
        let server = TestServer::start().expect("server starts");
        let endpoints = Endpoints::from_server(&server);
        (server, endpoints)
    }

    #[test]
    fn every_key_is_classified_and_reachable_rows_match() {
        let mut reachable = 0usize;
        for &key in ALL_KEYS {
            match reachability(key) {
                Reachability::Reachable => reachable += 1,
                Reachability::AdapterOnly(j) | Reachability::ContractGap(j) => {
                    assert!(!j.is_empty(), "{key:?} recorded with no justification");
                }
            }
        }
        assert_eq!(
            reachable,
            rows().len(),
            "every reachable key must have exactly one positive-control row"
        );
        // PermissionDenied stays adapter-only (no host control); Io is now reachable via the
        // request-side File sink (M1.5). Pinned so a silent regression is caught.
        assert!(matches!(
            reachability(HttpErrorKey::PermissionDenied),
            Reachability::AdapterOnly(_)
        ));
        assert!(matches!(
            reachability(HttpErrorKey::Io),
            Reachability::Reachable
        ));
    }

    #[test]
    fn correct_mock_produces_every_reachable_key() {
        let (_s, ep) = harness();
        let factory = SocketMockFactory::correct(ep.good_spki());
        let ctx = ConformanceCtx {
            factory: &factory,
            endpoints: &ep,
        };
        for (id, result) in run(rows(), &ctx) {
            assert_eq!(
                result,
                RowResult::Pass,
                "C2 row {id} did not produce its key"
            );
        }
    }

    // --- red twins: each positive control watched red -----------------------------------

    fn ctx_of<'a>(f: &'a SocketMockFactory, ep: &'a Endpoints) -> ConformanceCtx<'a> {
        ConformanceCtx {
            factory: f,
            endpoints: ep,
        }
    }

    fn twin(ep: &Endpoints, mutate: impl FnOnce(&mut MockBehavior)) -> SocketMockFactory {
        SocketMockFactory::correct(ep.good_spki()).with_behavior(mutate)
    }

    #[test]
    fn timeout_red_without_deadline() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.arm_deadline = false);
        assert!(matches!(c2_timeout(&ctx_of(&f, &ep)), RowResult::Fail(_)));
    }

    #[test]
    fn cancelled_red_when_silent() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honor_cancel = false);
        assert!(matches!(c2_cancelled(&ctx_of(&f, &ep)), RowResult::Fail(_)));
    }

    #[test]
    fn pin_mismatch_red_when_bypassed() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.check_pins = false);
        assert!(matches!(
            c2_pin_mismatch(&ctx_of(&f, &ep)),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn tls_red_when_untrusted_is_trusted() {
        // A mock that trusts the untrusted cert's SPKI succeeds where Tls is required.
        let (_s, ep) = harness();
        let f = SocketMockFactory::correct(ep.untrusted_spki());
        assert!(matches!(c2_tls(&ctx_of(&f, &ep)), RowResult::Fail(_)));
    }

    #[test]
    fn name_resolution_red_against_resolvable_host() {
        // The positive control's judgement must reject a resolvable target.
        let (_s, ep) = harness();
        let f = SocketMockFactory::correct(ep.good_spki());
        let ctx = ctx_of(&f, &ep);
        let url = Url::cleartext_dev(&ep.http("/ok")).expect("url");
        let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
        assert!(matches!(
            drive_expect_key(&ctx, req, HttpErrorKey::NameResolution, None),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn connect_red_against_open_port() {
        let (_s, ep) = harness();
        let f = SocketMockFactory::correct(ep.good_spki());
        let ctx = ctx_of(&f, &ep);
        let url = Url::cleartext_dev(&ep.http("/ok")).expect("url");
        let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
        assert!(matches!(
            drive_expect_key(&ctx, req, HttpErrorKey::Connect, None),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn transport_red_against_ok() {
        let (_s, ep) = harness();
        let f = SocketMockFactory::correct(ep.good_spki());
        let ctx = ctx_of(&f, &ep);
        let url = Url::cleartext_dev(&ep.http("/ok")).expect("url");
        let req = HttpRequest::builder(Method::Get, url, Duration::from_secs(5)).build();
        assert!(matches!(
            drive_expect_key(&ctx, req, HttpErrorKey::Transport, None),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn insecure_redirect_red_when_followed() {
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.refuse_insecure_redirect = false);
        assert!(matches!(
            c2_insecure_redirect(&ctx_of(&f, &ep)),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn too_many_redirects_red_without_a_limit() {
        // A huge limit ⇒ the mock chases the infinite loop until the budget expires ⇒ no completion.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.redirect_limit = u32::MAX);
        assert!(matches!(
            c2_too_many_redirects(&ctx_of(&f, &ep)),
            RowResult::Fail(_)
        ));
    }

    #[test]
    fn io_red_when_file_sink_ignored() {
        // The ignore-file-sink break skips the write ⇒ a Memory success where Io was required.
        let (_s, ep) = harness();
        let f = twin(&ep, |b| b.honor_file_sink = false);
        assert!(matches!(c2_io(&ctx_of(&f, &ep)), RowResult::Fail(_)));
    }
}
