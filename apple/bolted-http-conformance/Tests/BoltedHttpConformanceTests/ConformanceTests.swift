import XCTest
import BoltedHttpApple

/// Step 25 M0 — the harness-bridge gate. Two halves, both required:
///
/// - **GREEN** (`testC1Rule01IsGreenOnTheRealAdapter`): one real C1 row passes end to end through
///   the FFI — the walking-skeleton `BoltedHttp` URLSession adapter drives the suite's `/ok` row
///   and the structured driver reports it green.
/// - **RED** (`testC1Rule01IsRedWithABrokenAdapter`): a deliberately-broken adapter variant makes
///   the *same* row red, and the report carries a legible typed message. The bridge must be proven
///   able to fail before its greens mean anything.
final class ConformanceTests: XCTestCase {
    /// The composition-root dance: adapter first, harness second, then the weak back-reference.
    private func makeHarness(_ adapter: HttpAdapter) -> HttpHarness {
        let harness = HttpHarness(adapter: adapter)
        if let real = adapter as? BoltedHttp { real.harness = harness }
        if let broken = adapter as? BrokenHttp { broken.harness = harness }
        return harness
    }

    private func rule01(_ reports: [RowReport]) -> RowReport? {
        reports.first { $0.id.contains("rule-01") }
    }

    // MARK: - GREEN half

    func testC1Rule01IsGreenOnTheRealAdapter() {
        let harness = makeHarness(BoltedHttp())
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        let reports = harness.runC1()
        guard let row = rule01(reports) else {
            return XCTFail("rule-01 not among reported rows: \(reports.map(\.id))")
        }
        XCTAssertTrue(
            row.passed,
            "C1 rule-01 must be green on the real adapter — message: \(row.message)"
        )
        XCTAssertFalse(row.skipped, "rule-01 must run, not skip")
    }

    // MARK: - RED half

    func testC1Rule01IsRedWithABrokenAdapter() {
        let harness = makeHarness(BrokenHttp())
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }

        let reports = harness.runC1()
        guard let row = rule01(reports) else {
            return XCTFail("rule-01 not among reported rows: \(reports.map(\.id))")
        }
        XCTAssertFalse(
            row.passed,
            "a broken adapter must drive rule-01 red — the bridge must be able to fail"
        )
        XCTAssertFalse(
            row.message.isEmpty,
            "a red row must carry a legible, typed failure message"
        )
        // Visible in the test log: the exact typed reason the structured driver produced.
        print("M0 RED-HALF rule-01 message: \(row.message)")
    }
}

/// The M0 gate's break: an adapter that never performs a request, failing every effect immediately.
/// rule-01 expects a successful GET of `/ok`, so a blanket failure makes it red with the typed
/// reason `ExpectedSuccessGotError { got: Transport }` — a legible message, proving the bridge
/// carries reds, not just greens. Isolated to the test target; the shipped adapter is untouched.
final class BrokenHttp: HttpAdapter {
    weak var harness: HttpHarness?

    func execute(request: FfiRequest) {
        harness?.completeErr(
            token: request.token,
            error: .transport(message: "deliberately broken adapter (M0 gate red half)")
        )
    }
}
