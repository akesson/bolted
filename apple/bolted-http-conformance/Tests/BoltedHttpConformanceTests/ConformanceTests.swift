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

    // MARK: - A1 streaming probe (probe-grade — the step-24 F1 verdict, Apple edition)

    /// A1: a streamed response through the S-FFI-chosen mechanism (F1 `ffi_stream` async push)
    /// inside a real http round-trip on the Apple path. URLSession consumes the test server's
    /// `/chunked` endpoint; each chunk crosses the FFI (`deliverChunk`) into the harness and is
    /// re-delivered to a LIVE Swift consumer draining `chunkStream()` as an `AsyncStream<Chunk>`.
    /// Proves ordered / lossless / complete for two pacings (burst `delay=0`, paced `delay=200µs`).
    func testA1StreamingProbeIsOrderedLosslessComplete() async throws {
        for delay: UInt64 in [0, 200] {
            let r = try await runA1(delayUs: delay, dropSeq: nil, classicLoading: nil)
            print("A1 F1 ffi_stream (delay=\(delay)µs): delivered=\(r.delivered)/\(Self.a1ChunkCount) "
                + "ingested=\(r.ingested) stallPoint=\(r.stallPoint) ordered=\(r.ordered) "
                + "p50=\(String(format: "%.1f", r.p50))µs p99=\(String(format: "%.1f", r.p99))µs "
                + "wall=\(String(format: "%.2f", r.wallMs))ms consumerThreads=\(r.consumerThreads) "
                + "producerThreads=\(r.producerThreads) consumerOffMain=\(!r.sawMain) "
                + "consumerHopsOffProducer=\(r.hopsOffProducer)")
            XCTAssertEqual(Int(r.ingested), Self.a1ChunkCount,
                           "http round-trip + cross-FFI ingest must be whole (delay=\(delay))")
            XCTAssertEqual(r.delivered, Self.a1ChunkCount,
                           "F1 re-delivery completeness — lossless (delay=\(delay))")
            XCTAssertEqual(Int(r.stallPoint), Self.a1ChunkCount,
                           "ordered, no gap: 1…N contiguous (delay=\(delay))")
            XCTAssertTrue(r.ordered, "chunks delivered in ascending seq order (delay=\(delay))")
            XCTAssertFalse(r.sawMain, "F1 consumer must resume OFF the main thread (delay=\(delay))")
        }
    }

    /// A1 control (watched-red): a deliberately-corrupting variant that DROPS one chunk before it
    /// crosses the FFI. The completeness check must DETECT the loss (stall point < N, delivered < N)
    /// — a probe that cannot fail proves nothing.
    func testA1CorruptionControlDetectsLoss() async throws {
        let drop = Self.a1ChunkCount / 2
        let r = try await runA1(delayUs: 0, dropSeq: drop, classicLoading: nil)
        print("A1 CONTROL (drop seq=\(drop)): delivered=\(r.delivered)/\(Self.a1ChunkCount) "
            + "stallPoint=\(r.stallPoint) ingested=\(r.ingested)")
        XCTAssertLessThan(r.delivered, Self.a1ChunkCount,
                          "the control must lose a chunk — the probe can detect loss")
        XCTAssertLessThan(Int(r.stallPoint), Self.a1ChunkCount,
                          "the gap must be detected: contiguous 1…N breaks at the dropped seq")
        XCTAssertEqual(Int(r.stallPoint), drop - 1,
                       "the stall point is exactly one before the dropped chunk")
    }

    // MARK: - A6 classic-loading-mode sweep

    /// A6: run the ENTIRE suite (C1 + extra + C2 + C3) AND the A1 probe with
    /// `usesClassicLoadingMode = false`, comparing row-by-row to the OS-default baseline. Expected:
    /// no divergence — but a finding either way. One flag on the adapter/session; the adapter is not
    /// forked.
    func testA6ClassicLoadingModeSweep() async throws {
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

        // The A1 probe under classic-off, compared to the default-mode completeness. The DEFAULT
        // path is the frozen reference and MUST stay whole (the F1 verdict). The classic-off run is
        // RECORDED, not asserted complete: kill-criterion 3 says a stall/reorder is a finding to log
        // and continue — not a gate failure. `ingested` vs `delivered` localises any divergence:
        // ingested<N ⇒ URLSession's streaming producer dropped under classic-off (foreign side);
        // ingested==N & delivered<N ⇒ the F1 ffi_stream re-delivery stalled (the step-02 ghost).
        let a1Default = try await runA1(delayUs: 0, dropSeq: nil, classicLoading: nil)
        let a1Classic = try await runA1(delayUs: 0, dropSeq: nil, classicLoading: false)
        print("A6 A1 probe default:    ingested=\(a1Default.ingested)/\(Self.a1ChunkCount) "
            + "delivered=\(a1Default.delivered) stallPoint=\(a1Default.stallPoint) ordered=\(a1Default.ordered)")
        print("A6 A1 probe classic-off: ingested=\(a1Classic.ingested)/\(Self.a1ChunkCount) "
            + "delivered=\(a1Classic.delivered) stallPoint=\(a1Classic.stallPoint) ordered=\(a1Classic.ordered)")
        // The default (frozen) path is the load-bearing A1 result — it must be whole.
        XCTAssertEqual(a1Default.delivered, Self.a1ChunkCount,
                       "A6: the A1 stream on the DEFAULT (frozen) Apple path must stay complete")
        if a1Classic.delivered != Self.a1ChunkCount || Int(a1Classic.ingested) != Self.a1ChunkCount {
            let locus = Int(a1Classic.ingested) < Self.a1ChunkCount
                ? "URLSession streaming PRODUCER (foreign side; not the F1 mechanism)"
                : "the F1 ffi_stream RE-DELIVERY (the step-02 stall ghost)"
            print("A6 [DIVERGE] A1 under classic-off: ingested=\(a1Classic.ingested) "
                + "delivered=\(a1Classic.delivered)/\(Self.a1ChunkCount) — divergence locus: \(locus). "
                + "Kill-criterion 3: recorded, not worked around (see M3 notes / verdict).")
        }
        // Whatever the classic-off delivery, ordering must never be violated (a reorder would be a
        // harder failure than loss — the F1 mechanism must never scramble sequence).
        XCTAssertTrue(a1Classic.ordered, "A6: chunks must stay in-order even under classic-off (no reorder)")
    }

    // MARK: - A1 probe machinery

    static let a1ChunkCount = 200

    fileprivate struct A1Result {
        let delivered: Int
        let ingested: UInt64
        let stallPoint: UInt64     // highest N with 1…N all received (== count when whole)
        let ordered: Bool
        let p50: Double
        let p99: Double
        let wallMs: Double
        let consumerThreads: Int
        let producerThreads: Int
        let sawMain: Bool
        let hopsOffProducer: Bool  // consumer ran on a thread the producer never used
    }

    /// Drive the A1 probe once: attach a live `ffi_stream` consumer, run a URLSession streaming
    /// round-trip against `/chunked`, push each chunk across the FFI. `dropSeq` (control) omits one
    /// chunk before it crosses; `classicLoading` sets the consumer session's loading mode (A6).
    private func runA1(delayUs: UInt64, dropSeq: Int?, classicLoading: Bool?) async throws -> A1Result {
        let count = Self.a1ChunkCount
        let harness = HttpHarness(adapter: BoltedHttp())
        let info = harness.startServer()
        XCTAssertFalse(info.httpBase.isEmpty, "the in-process test server failed to start")
        defer { harness.stopServer() }
        let url = "\(info.httpBase)/chunked?count=\(count)&delay_us=\(delayUs)"

        let collector = StreamCollector()
        // LIVE consumer attached BEFORE delivery begins (the step-02 stall shape).
        let stream = harness.chunkStream()
        let consumer = Task {
            for await chunk in stream {
                collector.recordConsumer(seq: chunk.seq, tSendNs: chunk.tSendNs)
            }
        }
        try? await Task.sleep(nanoseconds: 50_000_000)

        // The producer half: a URLSession DELEGATE consumer (`didReceive data`) reads the chunked
        // body on the session's own OperationQueue — a dedicated thread OFF the Swift cooperative
        // pool the `ffi_stream` consumer resumes on, so producer and consumer never contend, and the
        // cross-FFI push happens on a real background thread (the F1 re-entrancy rationale).
        let producer = StreamingProducer(
            count: count, dropSeq: dropSeq, classicLoading: classicLoading,
            recordProducerThread: { collector.recordProducer() },
            deliver: { seq, bytes, tSend, last in
                harness.deliverChunk(chunk: Chunk(seq: seq, bytes: bytes, tSendNs: tSend, last: last))
            })
        try await producer.run(url: url)
        // Wait for the consumer to drain to the expected count (count, or count-1 when a chunk was
        // dropped in the control). The producer has already pushed every chunk into a ring that far
        // exceeds the count (nothing is dropped by the ring), so the buffered chunks are guaranteed
        // deliverable — the wait is patient (not a fixed settle window) so consumer scheduling
        // contention under a full-suite run cannot truncate the count.
        let target = dropSeq != nil ? count - 1 : count
        await waitForDelivery(collector, target: target)
        let result = collector.summarize(count: count, ingested: harness.chunkIngested())
        // Tear the consumer down deterministically: close the stream (its `AsyncStream` ends) and
        // await the task so no stalled/live consumer lingers as a dead subscription in the shared
        // `ffi_stream` runtime to starve the next run.
        harness.closeChunkStream()
        _ = await consumer.value
        return result
    }

    /// Poll until `target` records are delivered (then a short settle for any stragglers), bounded by
    /// a generous cap. Buffered chunks are never dropped by the ring, so given wall-time the consumer
    /// reaches `target`; only a genuine stall exhausts the cap.
    private func waitForDelivery(_ collector: StreamCollector, target: Int, maxMs: Int = 30000) async {
        var total = 0
        while total < maxMs {
            if collector.consumerCount >= target {
                try? await Task.sleep(nanoseconds: 200_000_000)  // settle for late stragglers
                return
            }
            try? await Task.sleep(nanoseconds: 25_000_000)
            total += 25
        }
    }
}

/// The A1 producer: a URLSession delegate consumer that reads the `/chunked` body via
/// `didReceive data` on the session's own serial `OperationQueue` (a dedicated thread, OFF the Swift
/// cooperative pool the `ffi_stream` consumer resumes on — so the two never contend). It buffers raw
/// bytes, splits complete `chunk-NNNNNN\n` lines, and pushes each across the FFI via `deliver`.
/// `dropSeq` (the corruption control) omits one chunk before it crosses. `@unchecked Sendable`: the
/// delegate queue is serial, so `buffer`/`seq` are single-threaded.
private final class StreamingProducer: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let count: Int
    private let dropSeq: Int?
    private let recordProducerThread: () -> Void
    private let deliver: (UInt64, Data, UInt64, Bool) -> Void
    private var session: URLSession!
    private var buffer = [UInt8]()
    private var seq: UInt64 = 0
    private var continuation: CheckedContinuation<Void, Error>?

    init(count: Int, dropSeq: Int?, classicLoading: Bool?,
         recordProducerThread: @escaping () -> Void,
         deliver: @escaping (UInt64, Data, UInt64, Bool) -> Void) {
        self.count = count
        self.dropSeq = dropSeq
        self.recordProducerThread = recordProducerThread
        self.deliver = deliver
        super.init()
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        if let classic = classicLoading, #available(macOS 15.4, iOS 18.4, *) {
            config.usesClassicLoadingMode = classic
        }
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1  // serial delegate callbacks — ordered chunk boundaries
        self.session = URLSession(configuration: config, delegate: self, delegateQueue: queue)
    }

    /// Run the streamed round-trip to completion (resumes when the task completes / errors).
    func run(url: String) async throws {
        guard let u = URL(string: url) else { throw XCTSkip("bad url") }
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            self.continuation = cont
            session.dataTask(with: u).resume()
        }
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        buffer.append(contentsOf: data)
        var start = 0
        var i = 0
        while i < buffer.count {
            if buffer[i] == 0x0A {  // newline: one complete line
                let lineBytes = Array(buffer[start..<i])
                start = i + 1
                if let line = String(bytes: lineBytes, encoding: .utf8), line.hasPrefix("chunk-") {
                    seq += 1
                    if let drop = dropSeq, seq == UInt64(drop) {
                        i += 1
                        continue  // control: drop before it crosses the FFI
                    }
                    recordProducerThread()
                    deliver(seq, Data(lineBytes), DispatchTime.now().uptimeNanoseconds,
                            seq == UInt64(count))
                }
            }
            i += 1
        }
        if start > 0 { buffer.removeFirst(start) }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        let cont = continuation
        continuation = nil
        session.finishTasksAndInvalidate()  // break the delegate retain cycle (F-M1-7)
        if let error { cont?.resume(throwing: error) } else { cont?.resume(returning: ()) }
    }
}

/// Thread-safe collector of A1 per-chunk delivery records (consumer side) and producer-thread
/// observations. `@unchecked Sendable`: all state is `NSLock`-guarded.
private final class StreamCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var seqs: [UInt64] = []
    private var latenciesUs: [Double] = []
    private var recvNs: [UInt64] = []
    private var consumerThreads = Set<String>()
    private var producerThreads = Set<String>()
    private var _sawMain = false

    func recordConsumer(seq: UInt64, tSendNs: UInt64) {
        let now = DispatchTime.now().uptimeNanoseconds
        let lat = now > tSendNs ? Double(now - tSendNs) / 1000.0 : 0
        let isMain = Thread.isMainThread
        let td = "\(Thread.current)"
        lock.lock()
        seqs.append(seq); latenciesUs.append(lat); recvNs.append(now)
        consumerThreads.insert(td)
        if isMain { _sawMain = true }
        lock.unlock()
    }

    func recordProducer() {
        let td = "\(Thread.current)"
        lock.lock(); producerThreads.insert(td); lock.unlock()
    }

    var consumerCount: Int { lock.lock(); defer { lock.unlock() }; return seqs.count }

    func summarize(count: Int, ingested: UInt64) -> ConformanceTests.A1Result {
        lock.lock(); defer { lock.unlock() }
        let lats = latenciesUs.sorted()
        let p50 = lats.isEmpty ? 0 : lats[lats.count / 2]
        let p99 = lats.isEmpty ? 0 : lats[min(lats.count - 1, Int(0.99 * Double(lats.count)))]
        let recv = recvNs.sorted()
        let wallMs = recv.count >= 2 ? Double(recv.last! - recv.first!) / 1_000_000.0 : 0
        // Ordered: the delivery order is ascending in seq (the ffi_stream preserves push order).
        var ordered = true
        for i in 1..<max(seqs.count, 1) where i < seqs.count { if seqs[i] < seqs[i - 1] { ordered = false; break } }
        // Stall point: highest N such that 1…N were all received.
        let seen = Set(seqs)
        var stall: UInt64 = 0
        if count > 0 {
            for s in 1...UInt64(count) { if seen.contains(s) { stall = s } else { break } }
        }
        let hopsOff = consumerThreads.isDisjoint(with: producerThreads) && !consumerThreads.isEmpty
        return ConformanceTests.A1Result(
            delivered: seqs.count, ingested: ingested, stallPoint: stall, ordered: ordered,
            p50: p50, p99: p99, wallMs: wallMs,
            consumerThreads: consumerThreads.count, producerThreads: producerThreads.count,
            sawMain: _sawMain, hopsOffProducer: hopsOff)
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
