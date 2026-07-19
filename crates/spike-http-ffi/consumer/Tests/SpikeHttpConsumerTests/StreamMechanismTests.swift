import XCTest
import Foundation
import SpikeHttpFfi

/// Step-24 M2 — the S-FFI response-streaming mechanism verdict (feature-matrix row 16).
///
/// Re-runs the step-02 stream shapes INSIDE an http round-trip at boltffi 0.27.5:
/// a localhost chunked-HTTP server → URLSession consumes it on the Swift side → the adapter
/// pushes each chunk across the FFI into `StreamProbe` → the core re-delivers to a LIVE
/// consumer via one of three mechanisms (F1 ffi_stream push, F2 callback-trait push,
/// F3 wake-and-read pull). Measurements, not vibes: completeness, stall point, latency
/// p50/p99, wall time, delivery-thread affinity.
///
/// The stall (0.27.3) fired within tens of emissions with a live consumer; these tests
/// print `PROBE` lines with the numbers so the verdict artifact can cite them.
final class StreamMechanismTests: XCTestCase {

    private static let chunkCount = 100

    /// Thread-safe collector of per-chunk delivery records.
    private final class Collector: @unchecked Sendable {
        struct Rec { let seq: UInt64; let latencyUs: Double; let recvNs: UInt64 }
        private let lock = NSLock()
        private var recs: [Rec] = []
        private var threads = Set<String>()
        private(set) var sawMain = false

        func record(seq: UInt64, tSendNs: UInt64) {
            let now = DispatchTime.now().uptimeNanoseconds
            let lat = now > tSendNs ? Double(now - tSendNs) / 1000.0 : 0
            let isMain = Thread.isMainThread
            let tdesc = "\(Thread.current)"
            lock.lock()
            recs.append(Rec(seq: seq, latencyUs: lat, recvNs: now))
            threads.insert(tdesc)
            if isMain { sawMain = true }
            lock.unlock()
        }
        var count: Int { lock.lock(); defer { lock.unlock() }; return recs.count }
        var records: [Rec] { lock.lock(); defer { lock.unlock() }; return recs }
        var threadCount: Int { lock.lock(); defer { lock.unlock() }; return threads.count }
    }

    private struct Result {
        let delivered: Int
        let ingested: UInt64
        let stallPoint: UInt64   // last contiguous seq the consumer received (== count when whole)
        let p50: Double
        let p99: Double
        let wallMs: Double       // first→last consumer-receipt span
        let threads: Int
        let sawMain: Bool
    }

    private func summarize(_ c: Collector, ingested: UInt64) -> Result {
        let recs = c.records
        let lats = recs.map(\.latencyUs).sorted()
        let p50 = lats.isEmpty ? 0 : lats[lats.count / 2]
        let p99 = lats.isEmpty ? 0 : lats[min(lats.count - 1, Int(0.99 * Double(lats.count)))]
        let recvNs = recs.map(\.recvNs).sorted()
        let wallMs = recvNs.count >= 2
            ? Double(recvNs.last! - recvNs.first!) / 1_000_000.0 : 0
        // Stall point = highest N such that seqs 1...N were all received.
        let seen = Set(recs.map(\.seq))
        var contiguous: UInt64 = 0
        for s in 1...UInt64(Self.chunkCount) where seen.contains(s) { contiguous = s } // last present
        var stall: UInt64 = 0
        for s in 1...UInt64(Self.chunkCount) { if seen.contains(s) { stall = s } else { break } }
        _ = contiguous
        return Result(delivered: recs.count, ingested: ingested, stallPoint: stall,
                      p50: p50, p99: p99, wallMs: wallMs,
                      threads: c.threadCount, sawMain: c.sawMain)
    }

    /// The adapter half: consume the localhost chunked HTTP body with URLSession and push each
    /// line across the FFI via `deliver`. Runs as its own task (concurrent with the consumer).
    private func runAdapter(
        url: String, deliver: @escaping @Sendable (Chunk) -> Void
    ) async throws {
        let config = URLSessionConfiguration.ephemeral
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        let session = URLSession(configuration: config)
        guard let u = URL(string: url) else { throw XCTSkip("bad url") }
        let (bytes, _) = try await session.bytes(from: u)
        var seq: UInt64 = 0
        for try await line in bytes.lines {
            guard line.hasPrefix("chunk-") else { continue }
            seq += 1
            let chunk = Chunk(
                token: 1,
                seq: seq,
                bytes: Data(line.utf8),
                tSendNs: DispatchTime.now().uptimeNanoseconds,
                last: seq == UInt64(Self.chunkCount)
            )
            deliver(chunk)
        }
    }

    private func waitUntil(_ cond: @escaping () -> Bool, idleMs: Int = 500, maxMs: Int = 15000) async {
        var idle = 0, total = 0, last = -1
        while total < maxMs {
            try? await Task.sleep(nanoseconds: 25_000_000)
            total += 25
            if cond() { // give a short settle window after the condition first holds
                try? await Task.sleep(nanoseconds: 150_000_000)
                return
            }
            let c = last // unused churn guard
            _ = c
            idle += 25
        }
    }

    // MARK: F1 — ffi_stream async push (the 15/100 shape)

    private func runF1(delayUs: UInt64) async throws -> Result {
        let probe = StreamProbe()
        let url = probe.startChunkServer(chunks: UInt32(Self.chunkCount), delayUs: delayUs)
        XCTAssertFalse(url.isEmpty, "server must bind")
        let collector = Collector()

        // LIVE consumer attached before delivery begins.
        let stream = probe.f1Stream()
        let consumer = Task {
            for await chunk in stream { collector.record(seq: chunk.seq, tSendNs: chunk.tSendNs) }
        }
        defer { consumer.cancel() }
        try? await Task.sleep(nanoseconds: 50_000_000)

        try await runAdapter(url: url) { probe.deliverF1(chunk: $0) }
        await waitUntil({ collector.count >= Self.chunkCount })
        return summarize(collector, ingested: probe.ingested())
    }

    func testF1FfiStreamPush() async throws {
        for delay: UInt64 in [0, 200] {
            let r = try await runF1(delayUs: delay)
            print("PROBE F1 ffi_stream (delay=\(delay)µs): delivered=\(r.delivered)/\(Self.chunkCount) "
                + "ingested=\(r.ingested) stallPoint=\(r.stallPoint) "
                + "p50=\(String(format: "%.1f", r.p50))µs p99=\(String(format: "%.1f", r.p99))µs "
                + "wall=\(String(format: "%.2f", r.wallMs))ms threads=\(r.threads) sawMain=\(r.sawMain)")
            XCTAssertEqual(Int(r.ingested), Self.chunkCount, "http round-trip must ingest all chunks")
            XCTAssertEqual(r.delivered, Self.chunkCount, "F1 delivery completeness (delay=\(delay))")
        }
    }

    // MARK: F2 — callback-trait push (~8 ns machinery)

    private final class SinkImpl: ChunkSink, @unchecked Sendable {
        let collector: Collector
        init(_ c: Collector) { self.collector = c }
        func onChunk(chunk: Chunk) { collector.record(seq: chunk.seq, tSendNs: chunk.tSendNs) }
    }

    private func runF2(delayUs: UInt64) async throws -> Result {
        let probe = StreamProbe()
        let url = probe.startChunkServer(chunks: UInt32(Self.chunkCount), delayUs: delayUs)
        XCTAssertFalse(url.isEmpty, "server must bind")
        let collector = Collector()
        probe.setSink(sink: SinkImpl(collector)) // registered before delivery

        try await runAdapter(url: url) { probe.deliverF2(chunk: $0) }
        await waitUntil({ collector.count >= Self.chunkCount })
        return summarize(collector, ingested: probe.ingested())
    }

    func testF2CallbackTraitPush() async throws {
        for delay: UInt64 in [0, 200] {
            let r = try await runF2(delayUs: delay)
            print("PROBE F2 callback-trait (delay=\(delay)µs): delivered=\(r.delivered)/\(Self.chunkCount) "
                + "ingested=\(r.ingested) stallPoint=\(r.stallPoint) "
                + "p50=\(String(format: "%.1f", r.p50))µs p99=\(String(format: "%.1f", r.p99))µs "
                + "wall=\(String(format: "%.2f", r.wallMs))ms threads=\(r.threads) sawMain=\(r.sawMain)")
            XCTAssertEqual(Int(r.ingested), Self.chunkCount, "http round-trip must ingest all chunks")
            XCTAssertEqual(r.delivered, Self.chunkCount, "F2 delivery completeness (delay=\(delay))")
        }
    }

    // MARK: F3 — wake-and-read batch pull

    private func runF3(delayUs: UInt64) async throws -> Result {
        let probe = StreamProbe()
        let url = probe.startChunkServer(chunks: UInt32(Self.chunkCount), delayUs: delayUs)
        XCTAssertFalse(url.isEmpty, "server must bind")
        let collector = Collector()

        // LIVE consumer: wake → drain the buffered chunks.
        let wakes = probe.f3WakeStream()
        let consumer = Task {
            for await _ in wakes {
                for chunk in probe.drainF3() {
                    collector.record(seq: chunk.seq, tSendNs: chunk.tSendNs)
                }
            }
        }
        defer { consumer.cancel() }
        try? await Task.sleep(nanoseconds: 50_000_000)

        try await runAdapter(url: url) { probe.deliverF3(chunk: $0) }
        await waitUntil({ collector.count >= Self.chunkCount })
        // Final drain in case the last wake coalesced (drop-newest on a full cap-1 wake buffer).
        for chunk in probe.drainF3() { collector.record(seq: chunk.seq, tSendNs: chunk.tSendNs) }
        return summarize(collector, ingested: probe.ingested())
    }

    func testF3WakeAndReadPull() async throws {
        for delay: UInt64 in [0, 200] {
            let r = try await runF3(delayUs: delay)
            print("PROBE F3 wake-and-read (delay=\(delay)µs): delivered=\(r.delivered)/\(Self.chunkCount) "
                + "ingested=\(r.ingested) stallPoint=\(r.stallPoint) "
                + "p50=\(String(format: "%.1f", r.p50))µs p99=\(String(format: "%.1f", r.p99))µs "
                + "wall=\(String(format: "%.2f", r.wallMs))ms threads=\(r.threads) sawMain=\(r.sawMain)")
            XCTAssertEqual(Int(r.ingested), Self.chunkCount, "http round-trip must ingest all chunks")
            XCTAssertEqual(r.delivered, Self.chunkCount, "F3 delivery completeness (delay=\(delay))")
        }
    }
}
