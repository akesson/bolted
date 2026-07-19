import XCTest
import BoltedHttpApple

/// Step 25 — the Apple HTTP adapter conformance gate.
///
/// - **M0** (`testC1Rule01…`): the harness bridge can carry a green (real adapter) and a red
///   (deliberately-broken adapter). Retained as the bridge's fail-ability proof.
/// - **M1 green** (`testM1RowsAreGreenExceptTheM2Syntheses`): the real `BoltedHttp` adapter drives
///   the full C1 + C2 suite. Every row M1 owns is green; the A2/A4 syntheses (file sink, pinning, hop
///   trace, https→http refusal, Io) stay red and are printed, not asserted — they land in M2.
/// - **M1 watched-red** (`testWatchedRedBaseline`): every M1-green row is first shown RED under a
///   broken adapter — the anti-vacuous-green baseline the notes record.
final class ConformanceTests: XCTestCase {
    /// The composition-root dance: adapter first, harness second, then the weak back-reference.
    private func makeHarness(_ adapter: HttpAdapter) -> HttpHarness {
        let harness = HttpHarness(adapter: adapter)
        if let real = adapter as? BoltedHttp { real.harness = harness }
        if let broken = adapter as? BrokenHttp { broken.harness = harness }
        if let ok = adapter as? AlwaysOkHttp { ok.harness = harness }
        return harness
    }

    private func row(_ reports: [RowReport], _ needle: String) -> RowReport? {
        reports.first { $0.id.contains(needle) }
    }

    // MARK: - M0 gate (bridge fail-ability)

    func testC1Rule01IsGreenOnTheRealAdapter() {
        let harness = makeHarness(BoltedHttp())
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        let reports = harness.runC1()
        guard let r = row(reports, "rule-01") else {
            return XCTFail("rule-01 not among reported rows: \(reports.map(\.id))")
        }
        XCTAssertTrue(r.passed, "C1 rule-01 must be green on the real adapter — message: \(r.message)")
        XCTAssertFalse(r.skipped, "rule-01 must run, not skip")
    }

    func testC1Rule01IsRedWithABrokenAdapter() {
        let harness = makeHarness(BrokenHttp())
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        let reports = harness.runC1()
        guard let r = row(reports, "rule-01") else {
            return XCTFail("rule-01 not among reported rows: \(reports.map(\.id))")
        }
        XCTAssertFalse(r.passed, "a broken adapter must drive rule-01 red — the bridge must be able to fail")
        XCTAssertFalse(r.message.isEmpty, "a red row must carry a legible, typed failure message")
        print("M0 RED-HALF rule-01 message: \(r.message)")
    }

    // MARK: - Row classification

    /// Rows M1 owns — every one must be green on the real adapter. (rule-05's real 304 comes free
    /// from the ephemeral session — the A3 note's prediction, so it is an M1 green, not M2.)
    private static let m1Green: [String] = [
        "rule-01", "rule-02", "rule-03", "rule-05", "rule-06", "rule-07", "rule-08", "rule-09",
        "rule-11",
        "key-timeout", "key-cancelled", "key-tls", "key-name-resolution", "key-connect",
        "key-transport", "key-too-many-redirects",
    ]

    /// Rows deferred to M2 (https→http refusal / pinning / file-sink Io). Asserted red so a premature
    /// green is caught; the watched-red record for the M2 syntheses.
    private static let m2Red: [String] = [
        "rule-04", "rule-10", "key-pin-mismatch", "key-insecure-redirect", "key-io",
    ]

    // MARK: - M1 gate (the real adapter across C1 + C2)

    func testM1RowsAreGreenExceptTheM2Syntheses() {
        let adapter = BoltedHttp()
        let harness = makeHarness(adapter)
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        // Install the good cert as the adapter's trust anchor (anchor-only, M1): the HTTPS rows
        // evaluate the self-signed test endpoint against it. Handed over by startServer().
        XCTAssertFalse(info.goodCertDer.isEmpty, "startServer must export the good cert DER anchor")
        adapter.trustAnchorDER = info.goodCertDer

        let reports = harness.runC1() + harness.runC2()

        // Full status dump — the green/red evidence for the M1 notes.
        for r in reports.sorted(by: { $0.id < $1.id }) {
            let mark = r.passed ? "GREEN" : (r.skipped ? "SKIP " : "RED  ")
            print("M1 [\(mark)] \(r.id)\(r.message.isEmpty ? "" : " — \(r.message)")")
        }

        for needle in Self.m1Green {
            guard let r = row(reports, needle) else {
                XCTFail("expected M1 row \(needle) not among reports: \(reports.map(\.id))")
                continue
            }
            XCTAssertTrue(r.passed, "M1 row \(r.id) must be green — message: \(r.message)")
            XCTAssertFalse(r.skipped, "M1 row \(r.id) must run, not skip")
        }

        for needle in Self.m2Red {
            guard let r = row(reports, needle) else {
                XCTFail("expected M2 row \(needle) not among reports: \(reports.map(\.id))")
                continue
            }
            XCTAssertFalse(r.passed, "M2 synthesis \(r.id) is not expected green until M2")
        }
    }

    // MARK: - M1 watched-red baseline

    /// Every M1-green row is shown RED first (anti-vacuous-green). Two broken adapters cover the two
    /// row polarities: `BrokenHttp` (always errors `Transport`) reds every success- or specific-error-
    /// expecting row; `AlwaysOkHttp` (always `200`) reds the rows that require an error — rule-08 and
    /// key-transport — which `BrokenHttp`'s Transport happens to satisfy.
    func testWatchedRedBaseline() {
        // BrokenHttp reds all M1-green rows except the two that expect Transport.
        let brokenReds = Self.m1Green.filter { $0 != "rule-08" && $0 != "key-transport" }
        let brokenHarness = makeHarness(BrokenHttp())
        XCTAssertFalse(brokenHarness.startServer().httpBase.isEmpty)
        let brokenReports = brokenHarness.runC1() + brokenHarness.runC2()
        brokenHarness.stopServer()
        for r in brokenReports.sorted(by: { $0.id < $1.id }) where !r.passed {
            print("RED-BASELINE [broken] \(r.id) — \(r.message)")
        }
        for needle in brokenReds {
            guard let r = row(brokenReports, needle) else { continue }
            XCTAssertFalse(r.passed, "watched-red: \(r.id) must be red under BrokenHttp — got green")
        }

        // AlwaysOkHttp reds the error-expecting rows BrokenHttp cannot.
        let okHarness = makeHarness(AlwaysOkHttp())
        XCTAssertFalse(okHarness.startServer().httpBase.isEmpty)
        let okReports = okHarness.runC1() + okHarness.runC2()
        okHarness.stopServer()
        for needle in ["rule-08", "key-transport"] {
            guard let r = row(okReports, needle) else {
                XCTFail("row \(needle) missing under AlwaysOkHttp")
                continue
            }
            print("RED-BASELINE [always-ok] \(r.id) — \(r.message)")
            XCTAssertFalse(r.passed, "watched-red: \(r.id) must be red under AlwaysOkHttp — got green")
        }
    }
}

/// The M0 gate's break: an adapter that never performs a request, failing every effect immediately.
/// rule-01 expects a successful GET of `/ok`, so a blanket failure makes it red with the typed reason
/// `ExpectedSuccessGotError { got: Transport }`. Isolated to the test target; the shipped adapter is
/// untouched.
final class BrokenHttp: HttpAdapter {
    weak var harness: HttpHarness?

    func execute(request: FfiRequest) {
        harness?.completeErr(
            token: request.token,
            error: .transport(message: "deliberately broken adapter (gate red half)")
        )
    }

    func cancel(token: UInt64) {}
}

/// The complementary break for the watched-red baseline: an adapter that always succeeds with a bare
/// `200`, so every row that requires a typed error (rule-08 no-hidden-retry, key-transport) goes red.
final class AlwaysOkHttp: HttpAdapter {
    weak var harness: HttpHarness?

    func execute(request: FfiRequest) {
        harness?.completeOk(
            response: FfiResponse(
                token: request.token,
                status: 200,
                headers: [],
                body: Data("ok".utf8),
                finalUrl: request.url,
                httpVersion: .http11
            )
        )
    }

    func cancel(token: UInt64) {}
}
