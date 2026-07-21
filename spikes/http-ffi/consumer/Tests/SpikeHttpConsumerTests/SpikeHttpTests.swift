import XCTest
import SpikeHttpFfi

/// Step-02 third-cluster probes (crates/bolted-http/docs/architecture.md §4):
/// capability round-trip, error taxonomy, single-flight, and measurements.
/// Network-dependent: success/DNS/TLS cases need outbound connectivity.
final class SpikeHttpTests: XCTestCase {
    /// The composition-root wiring dance: adapter first, core second, back-ref third.
    private func makeCore() -> (SpikeCore, BoltedHttpAdapter) {
        let adapter = BoltedHttpAdapter()
        let core = SpikeCore(adapter: adapter)
        adapter.core = core
        return (core, adapter)
    }

    private func awaitOutcome(
        _ core: SpikeCore, token: UInt64, timeout: TimeInterval
    ) -> HttpOutcome {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            let outcome = core.outcome(token: token)
            if outcome != .pending { return outcome }
            Thread.sleep(forTimeInterval: 0.05)
        }
        return core.outcome(token: token)
    }

    // MARK: round-trip

    func testSuccessRoundTrip() {
        let (core, adapter) = makeCore()
        let token = core.fetch(url: "https://www.apple.com/", deadlineMs: 15_000)
        let outcome = awaitOutcome(core, token: token, timeout: 20)
        guard case let .succeeded(status, bodyLen, finalUrl) = outcome else {
            return XCTFail("expected success, got \(outcome)")
        }
        XCTAssertEqual(status, 200)
        XCTAssertGreaterThan(bodyLen, 0)
        XCTAssertTrue(finalUrl.contains("apple.com"), "final URL reported: \(finalUrl)")
        print("MEASURE completion-thread: \(adapter.lastCompletionThread)")
    }

    func testRedirectReportsFinalUrl() {
        let (core, _) = makeCore()
        // apple.com 301-redirects to www.apple.com; the stack follows silently
        // and only the final URL is reported — the portable-core redirect rule.
        let token = core.fetch(url: "https://apple.com/", deadlineMs: 15_000)
        let outcome = awaitOutcome(core, token: token, timeout: 20)
        guard case let .succeeded(_, _, finalUrl) = outcome else {
            return XCTFail("expected success, got \(outcome)")
        }
        XCTAssertTrue(finalUrl.contains("www.apple.com"), "expected followed redirect, got \(finalUrl)")
        print("MEASURE redirect final URL: \(finalUrl)")
    }

    // MARK: error taxonomy — the first rows of the conformance suite

    func testTimeoutMapsToTypedKey() {
        let (core, _) = makeCore()
        // Non-routable address: the connect attempt hangs until the deadline.
        let token = core.fetch(url: "https://10.255.255.1/", deadlineMs: 1_500)
        let outcome = awaitOutcome(core, token: token, timeout: 30)
        guard case let .failed(error) = outcome else {
            return XCTFail("expected failure, got \(outcome)")
        }
        XCTAssertEqual(error, .timeout(deadlineMs: 1_500))
    }

    func testDnsFailureMapsToTypedKey() {
        let (core, _) = makeCore()
        // RFC 2606: .invalid never resolves.
        let token = core.fetch(url: "https://bolted-spike-host.invalid/", deadlineMs: 10_000)
        let outcome = awaitOutcome(core, token: token, timeout: 15)
        guard case let .failed(error) = outcome else {
            return XCTFail("expected failure, got \(outcome)")
        }
        guard case let .dnsFailure(host) = error else {
            return XCTFail("expected dnsFailure, got \(error)")
        }
        XCTAssertEqual(host, "bolted-spike-host.invalid")
    }

    func testTlsFailureMapsToTypedKey() {
        let (core, _) = makeCore()
        let token = core.fetch(url: "https://self-signed.badssl.com/", deadlineMs: 10_000)
        let outcome = awaitOutcome(core, token: token, timeout: 15)
        guard case let .failed(error) = outcome else {
            return XCTFail("expected failure, got \(outcome)")
        }
        guard case .tlsFailure = error else {
            return XCTFail("expected tlsFailure, got \(error)")
        }
    }

    // MARK: single-flight

    func testStaleCompletionIsIgnored() {
        let (core, _) = makeCore()
        // Token that the core never issued: dropped entirely.
        core.completeErr(token: 999, error: .timeout(deadlineMs: 1))
        XCTAssertEqual(core.outcome(token: 999), .pending)

        // First completion wins; a duplicate must not overwrite it.
        let token = core.fetch(url: "https://10.255.255.1/", deadlineMs: 60_000)
        core.completeErr(token: token, error: .timeout(deadlineMs: 1))
        core.completeErr(token: token, error: .dnsFailure(host: "dup"))
        XCTAssertEqual(core.outcome(token: token), .failed(error: .timeout(deadlineMs: 1)))
    }

    // MARK: measurements (printed, not asserted)

    func testMeasurements() {
        let (core, _) = makeCore()
        let iterations = 100_000

        // Swift → Rust: no-op class method.
        var start = DispatchTime.now()
        for _ in 0..<iterations { core.noop() }
        var elapsed = DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds
        print("MEASURE noop Swift→Rust: \(Double(elapsed) / Double(iterations)) ns/call")

        // Rust → Swift: callback-trait method, timed on the Rust side.
        let pingTotal = core.measurePing(iterations: UInt64(iterations))
        print("MEASURE ping Rust→Swift: \(Double(pingTotal) / Double(iterations)) ns/call")

        // Payload cost: bytes across the boundary (encode + copy + decode).
        for (size, iters) in [(1_024, 10_000), (65_536, 1_000), (1_048_576, 100)] {
            let payload = Data(repeating: 0xAB, count: size)
            start = DispatchTime.now()
            for _ in 0..<iters {
                XCTAssertEqual(core.echoLen(payload: payload), UInt64(size))
            }
            elapsed = DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds
            let perCall = Double(elapsed) / Double(iters)
            print("MEASURE echo \(size)B: \(perCall / 1_000) µs/call")
        }
    }
}
