import XCTest
import BoltedHttpApple

/// Step 25 — the Apple HTTP adapter conformance gate.
///
/// - **M0** (`testC1Rule01…`): the harness bridge can carry a green (real adapter) and a red
///   (deliberately-broken adapter). Retained as the bridge's fail-ability proof.
/// - **M2 full green** (`testFullSuiteIsGreenOnTheRealAdapter`): the real `BoltedHttp` adapter drives
///   the ENTIRE suite — all C1 rows, the C1-adjacent extra rows (row-15 sink correspondence, the
///   redirect hop trace), and every C2 key — green, plus the pinned C3 Apple column. The A2/A4
///   syntheses (file sink, SPKI pinning, hop trace, https→http refusal, Io) that were red in M1 are
///   now green.
/// - **Watched-red** (`testWatchedRedBaseline`): every green row is first shown RED under a broken
///   adapter — the anti-vacuous-green baseline the notes record.
/// - **PermissionDenied mapping** (`testPermissionDeniedMapping`): the load-bearing EPERM→key mapping
///   proven at the unit level (a live host control is platform-gated; see the M2 notes).
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

    // MARK: - Rows that expect a `Transport` error (BrokenHttp cannot red them)

    /// The two rows whose contract outcome IS `Transport` — a blanket-Transport broken adapter makes
    /// them GREEN, so `AlwaysOkHttp` (always 200) is the adapter that reds them instead.
    private static let transportExpectingRows: Set<String> = ["rule-08", "key-transport"]

    // MARK: - M2 gate (the whole suite, real adapter)

    /// Run every driver row (C1 + extra rows + C2) against `adapter` over a fresh server. The trust
    /// anchor is installed for the HTTPS rows.
    private func runFullSuite(_ adapter: HttpAdapter) -> (reports: [RowReport], c3: String) {
        let harness = makeHarness(adapter)
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        XCTAssertFalse(info.goodCertDer.isEmpty, "startServer must export the good cert DER anchor")
        if let real = adapter as? BoltedHttp { real.trustAnchorDER = info.goodCertDer }
        defer { harness.stopServer() }
        let reports = harness.runC1() + harness.runExtraRows() + harness.runC2()
        return (reports, harness.runC3())
    }

    func testFullSuiteIsGreenOnTheRealAdapter() {
        let (reports, c3) = runFullSuite(BoltedHttp())

        // Full status dump — the green/red evidence for the M2 notes.
        for r in reports.sorted(by: { $0.id < $1.id }) {
            let mark = r.passed ? "GREEN" : (r.skipped ? "SKIP " : "RED  ")
            print("M2 [\(mark)] \(r.id)\(r.message.isEmpty ? "" : " — \(r.message)")")
        }

        XCTAssertFalse(reports.isEmpty, "no rows ran")
        for r in reports {
            XCTAssertTrue(r.passed, "row \(r.id) must be green on the real adapter — message: \(r.message)")
            XCTAssertFalse(r.skipped, "row \(r.id) must run, not skip")
        }

        // The pinned C3 Apple column, generated from the capability traits (row 12 present, row 18
        // Phase). A drift here means a capability impl changed without updating this expectation.
        let expectedC3 = """
        capability     | presence
        ---------------+-----------------------
        priority-hint  | present
        metrics        | present (Phase)
        """
        XCTAssertEqual(c3, expectedC3, "C3 Apple column drifted:\n\(c3)")
        print("M2 C3 Apple column:\n\(c3)")
    }

    // MARK: - Watched-red baseline (anti-vacuous-green)

    /// Every green row is shown RED first. Two broken adapters cover the two row polarities:
    /// `BrokenHttp` (always errors `Transport`) reds every success- or specific-error-expecting row;
    /// `AlwaysOkHttp` (always `200`) reds the `Transport`-expecting rows `BrokenHttp` cannot.
    func testWatchedRedBaseline() {
        // BrokenHttp reds all rows except the two that expect Transport.
        let brokenHarness = makeHarness(BrokenHttp())
        XCTAssertFalse(brokenHarness.startServer().httpBase.isEmpty)
        let brokenReports = brokenHarness.runC1() + brokenHarness.runExtraRows() + brokenHarness.runC2()
        brokenHarness.stopServer()
        for r in brokenReports.sorted(by: { $0.id < $1.id }) where !r.passed {
            print("RED-BASELINE [broken] \(r.id) — \(r.message)")
        }
        for r in brokenReports where !Self.transportExpectingRows.contains(where: { r.id.contains($0) }) {
            XCTAssertFalse(r.passed, "watched-red: \(r.id) must be red under BrokenHttp — got green")
        }

        // AlwaysOkHttp reds the Transport-expecting rows BrokenHttp cannot.
        let okHarness = makeHarness(AlwaysOkHttp())
        XCTAssertFalse(okHarness.startServer().httpBase.isEmpty)
        let okReports = okHarness.runC1() + okHarness.runExtraRows() + okHarness.runC2()
        okHarness.stopServer()
        for needle in Self.transportExpectingRows {
            guard let r = row(okReports, needle) else {
                XCTFail("row \(needle) missing under AlwaysOkHttp")
                continue
            }
            print("RED-BASELINE [always-ok] \(r.id) — \(r.message)")
            XCTAssertFalse(r.passed, "watched-red: \(r.id) must be red under AlwaysOkHttp — got green")
        }
    }

    // MARK: - PermissionDenied (the mapping control; live host control platform-gated)

    /// `PermissionDenied` has no hermetic host control on the macOS SwiftPM test tier (the genuine
    /// causes — Apple Local Network privacy, an App-Sandbox network denial — need a GUI / an
    /// entitlement-signed sandboxed bundle, both non-gating for this step). So its positive control is
    /// the load-bearing MAPPING itself: a genuine POSIX `EPERM` maps to `PermissionDenied`, and a
    /// non-permission errno does not (the negative control — the mapping is not vacuous).
    func testPermissionDeniedMapping() {
        XCTAssertEqual(BoltedHttp.permissionKeyForPOSIX(EPERM), .permissionDenied,
                       "a genuine EPERM must map to PermissionDenied")
        XCTAssertNil(BoltedHttp.permissionKeyForPOSIX(ECONNREFUSED),
                     "a connection-refused errno is not permission-shaped")
        XCTAssertNil(BoltedHttp.permissionKeyForPOSIX(ETIMEDOUT),
                     "a timeout errno is not permission-shaped")
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
                httpVersion: .http11,
                hops: [],
                sinkPath: ""
            )
        )
    }

    func cancel(token: UInt64) {}
}
