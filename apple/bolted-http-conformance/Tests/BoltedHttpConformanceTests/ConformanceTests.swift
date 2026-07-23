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
        // Streaming rows (12/13) run against the real adapter's `execute_streaming` path (step-27 M3).
        let reports = harness.runC1() + harness.runExtraRows() + harness.runC2() + harness.runStreamRows()
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

        // The pinned C3 Apple column, generated from the capability traits (row 18 Phase). The
        // priority hint (row 12) is now a uniform advisory field, not a divergent capability
        // (ruled Q10), so it has no column. A drift here means a capability impl changed without
        // updating this expectation.
        let expectedC3 = """
        capability     | presence
        ---------------+-----------------------
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

    // MARK: - A5 priority acceptance (row 12 CAP — acceptance-only, NOT the wire)

    /// The request's priority hint is mapped to `URLSessionTask.priority` and the TASK carries the
    /// mapped value (acceptance-only; the RFC 9218 wire behaviour is FLAGGED lore, not tested).
    ///
    /// Watched-red / non-vacuous: the mapping DISTINGUISHES levels (High ≠ Low), and the applied
    /// task priority for a High request is `highPriority` — NOT the URLSession `defaultPriority`
    /// (0.5) an adapter that never touched `task.priority` would leave. That default-vs-High gap is
    /// what makes the assertion able to fail.
    func testA5PriorityAcceptanceOnTheTask() {
        XCTAssertEqual(BoltedHttp.taskPriority(for: .high), URLSessionTask.highPriority)
        XCTAssertEqual(BoltedHttp.taskPriority(for: .critical), URLSessionTask.highPriority)
        XCTAssertEqual(BoltedHttp.taskPriority(for: .normal), URLSessionTask.defaultPriority)
        XCTAssertEqual(BoltedHttp.taskPriority(for: .low), URLSessionTask.lowPriority)
        XCTAssertEqual(BoltedHttp.taskPriority(for: .throttled), URLSessionTask.lowPriority)
        XCTAssertNotEqual(BoltedHttp.taskPriority(for: .high), BoltedHttp.taskPriority(for: .low),
                          "the mapping must distinguish priority levels — else acceptance is vacuous")

        let adapter = BoltedHttp()
        let harness = HttpHarness(adapter: adapter)
        adapter.harness = harness
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        // Drive a real request carrying an explicit High priority; the applied task priority is
        // recorded synchronously off the live task (before resume).
        let req = FfiRequest(token: 90001, method: "GET", url: info.httpBase + "/ok",
                             headers: [], body: Data(), deadlineMs: 5_000,
                             pins: [], sink: .memory, priority: .high)
        adapter.execute(request: req)
        XCTAssertEqual(adapter.lastTaskPriority, URLSessionTask.highPriority,
                       "the URLSessionTask must carry the High→highPriority mapping")
        XCTAssertNotEqual(adapter.lastTaskPriority, URLSessionTask.defaultPriority,
                          "acceptance would be vacuous if the task carried the untouched default")
        print("A5 accepted: High → task.priority = \(adapter.lastTaskPriority ?? -1) "
            + "(highPriority=\(URLSessionTask.highPriority), default=\(URLSessionTask.defaultPriority))")
        // Let the in-flight request settle before teardown (its completion into an unmatched token
        // is a harmless no-op; this only avoids racing stopServer).
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
    }

    // MARK: - A6 classic-loading-mode sweep

    /// A6: run the ENTIRE suite (C1 + extra + C2 + streaming rows + C3) with
    /// `usesClassicLoadingMode = false`, comparing row-by-row to the OS-default baseline. Expected:
    /// no divergence — but a finding either way. One flag on the adapter/session; the adapter is not
    /// forked. (The streaming rows now ride `runFullSuite`, so the sweep covers them; the step-25
    /// probe-grade A1 stream — its own `ffi_stream` machinery — graduated into rows 12/13/14.)
    func testA6ClassicLoadingModeSweep() {
        let baseline = runFullSuite(BoltedHttp())                       // OS-default loading mode
        let sweep = runFullSuite(BoltedHttp(classicLoading: false))     // forced non-classic

        var baseByID: [String: RowReport] = [:]
        for r in baseline.reports { baseByID[r.id] = r }
        var divergences = 0
        for s in sweep.reports.sorted(by: { $0.id < $1.id }) {
            guard let b = baseByID[s.id] else {
                print("A6 [NEW ROW] \(s.id) present only under classic-off"); divergences += 1; continue
            }
            if b.passed != s.passed || b.skipped != s.skipped {
                print("A6 [DIVERGE] \(s.id): default(passed=\(b.passed),skip=\(b.skipped)) "
                    + "vs classic-off(passed=\(s.passed),skip=\(s.skipped)) — \(s.message)")
                divergences += 1
            }
        }
        // Every row must still be green under classic-off (the suite is not merely unchanged — it is
        // unchanged AND passing).
        for r in sweep.reports {
            XCTAssertTrue(r.passed, "A6: row \(r.id) must stay green under classic-off — \(r.message)")
        }
        XCTAssertEqual(sweep.c3, baseline.c3, "A6: C3 Apple column must not diverge under classic-off")
        XCTAssertEqual(divergences, 0, "A6: \(divergences) suite row(s) diverged under classic-off")
        print("A6 suite sweep: \(sweep.reports.count) rows, \(divergences) divergence(s) vs default; "
            + "C3 stable=\(sweep.c3 == baseline.c3)")
    }

    // MARK: - Streaming rows 12/13 (streaming-seam §3b/§3c — the real adapter, step-27 M3)

    private func streamHarness(_ adapter: HttpAdapter) -> HttpHarness {
        let harness = makeHarness(adapter)
        XCTAssertFalse(harness.startServer().httpBase.isEmpty, "the in-process test server failed to start")
        return harness
    }

    /// Rows 12 (slow-consumer completeness) and 13 (terminal-exactly-once) GREEN on the real adapter:
    /// URLSession `didReceive data` pushes each read across the FFI into the driver-owned completeness
    /// gate, and `didCompleteWithError` delivers the single terminal.
    func testStreamingRowsGreenOnTheRealAdapter() {
        let harness = streamHarness(BoltedHttp())
        defer { harness.stopServer() }
        let reports = harness.runStreamRows()
        XCTAssertFalse(reports.isEmpty, "no streaming rows ran")
        for r in reports.sorted(by: { $0.id < $1.id }) {
            print("M3 STREAM [\(r.passed ? "GREEN" : "RED  ")] \(r.id)\(r.message.isEmpty ? "" : " — \(r.message)")")
            XCTAssertTrue(r.passed, "streaming row \(r.id) must be green on the real adapter — \(r.message)")
            XCTAssertFalse(r.skipped, "streaming row \(r.id) must run, not skip")
        }
        XCTAssertEqual(harness.liveStreams(), 0, "conformant streams must leave no live subscription")
    }

    /// Watched-red (row 12): `.dropChunk` drops the first transport read but counts its bytes toward
    /// the declared total, so the core completeness gate fires and row 12 goes red — the truncation
    /// the gate forbids, on the REAL Apple adapter.
    func testStreamingRow12RedOnDroppedChunk() {
        let harness = streamHarness(BoltedHttp(streamFault: .dropChunk))
        defer { harness.stopServer() }
        let reports = harness.runStreamRows()
        guard let r = reports.first(where: { $0.id.contains("row-12") }) else {
            return XCTFail("row-12 not among streaming rows: \(reports.map(\.id))")
        }
        print("M3 RED row-12 (dropChunk): \(r.message)")
        XCTAssertFalse(r.passed, "a dropped chunk must red row 12 (completeness gate) — \(r.message)")
        XCTAssertFalse(r.message.isEmpty, "a red row must carry a legible, typed failure message")
    }

    /// Watched-red (row 13): `.skipTerminal` delivers every chunk but never calls `finishBody`, so no
    /// terminal arrives and row 13 goes red — the missing-terminal break, on the REAL Apple adapter.
    func testStreamingRow13RedOnMissingTerminal() {
        let harness = streamHarness(BoltedHttp(streamFault: .skipTerminal))
        defer { harness.stopServer() }
        let reports = harness.runStreamRows()
        guard let r = reports.first(where: { $0.id.contains("row-13") }) else {
            return XCTFail("row-13 not among streaming rows: \(reports.map(\.id))")
        }
        print("M3 RED row-13 (skipTerminal): \(r.message)")
        XCTAssertFalse(r.passed, "a missing terminal must red row 13 — \(r.message)")
    }

    // MARK: - Row 14 — subscription hygiene (streaming-seam §3d; the F-M3-1 leak is the red case)

    /// Row 14 GREEN: after N conformant streamed responses, the driver-owned live-subscription count
    /// is back to baseline (0) — each terminal deterministically closes its stream (removes+consumes
    /// the parked `ChunkSink`). This is §3d's "live-count restored", measured on a Rust registry
    /// count, NOT an ARC/GC poll (the deterministic detection the step calls for).
    func testRow14SubscriptionHygieneGreen() {
        let harness = streamHarness(BoltedHttp())
        defer { harness.stopServer() }
        XCTAssertEqual(harness.liveStreams(), 0, "baseline: no streams before driving any")
        // runStreamRows drives two streamed responses to completion (rows 12 + 13).
        let reports = harness.runStreamRows()
        for r in reports { XCTAssertTrue(r.passed, "row \(r.id) must pass — \(r.message)") }
        XCTAssertEqual(harness.liveStreams(), 0,
                       "row 14: after conformant streamed responses, the live-subscription count must return to baseline")
    }

    /// Row 14 RED — the F-M3-1 leak made deterministic: a `.skipTerminal` adapter delivers every chunk
    /// but never sends the terminal, so the driver-owned subscription is never closed and the
    /// live-count stays above baseline. The leak is detected by the exact registry count, not by
    /// waiting on a weak reference (the step's caution).
    func testRow14RedOnLeakedSubscription() {
        let harness = streamHarness(BoltedHttp(streamFault: .skipTerminal))
        defer { harness.stopServer() }
        _ = harness.runStreamRows()   // both streams never finish → their subscriptions leak
        let live = harness.liveStreams()
        print("M3 RED row-14 (skipTerminal): liveStreams=\(live)")
        XCTAssertGreaterThan(live, 0,
                             "row 14: a never-finished stream must leave a live subscription — the F-M3-1 red case")
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

    func executeStreaming(request: FfiRequest) {
        harness?.finishBody(
            token: request.token,
            end: .failed(error: .transport(message: "deliberately broken adapter (gate red half)"))
        )
    }

    func signal(token: UInt64, flow: FfiFlowSignal) {}
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

    func executeStreaming(request: FfiRequest) {
        // Not exercised by the watched-red baseline (it reds only the buffered Transport-expecting
        // rows); a trivial empty-complete keeps it a valid streaming adapter.
        harness?.finishBody(token: request.token, end: .complete(total: 0))
    }

    func signal(token: UInt64, flow: FfiFlowSignal) {}
}
