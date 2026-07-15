// Step-02 probes (docs/steps/step-02-boltffi-probe.md): the four load-bearing BoltFFI
// features (C1) and the observation contract (C2), exercised against the exported
// step-01 profile feature. Test names map to probe ids.
//
// Measurement tests print `MEASURE …` / `PROBE …` lines; run them in release:
//   swift test -c release --filter Measurements

import XCTest
import SpikeProfileFfi

// MARK: - helpers

/// Thread-safe collector — the consumer side of stream probes.
final class Box<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [T] = []
    func append(_ x: T) {
        lock.lock()
        items.append(x)
        lock.unlock()
    }
    var snapshot: [T] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }
}

func date(_ y: UInt16, _ m: UInt8, _ d: UInt8) -> FfiDate {
    FfiDate(year: y, month: m, day: d)
}

func seedCanonical(_ facet: ProfileFacet, name: String = "Alice") throws {
    try facet.applyCanonical(
        username: "alice",
        name: name,
        email: "alice@example.com",
        start: date(2026, 7, 1),
        end: date(2026, 7, 31)
    )
}

/// Consume `stream` into a box until the item count is stable for `idleMs` (or `maxMs` cap).
func collectUntilIdle<T: Sendable>(
    _ stream: AsyncStream<T>, idleMs: Int = 400, maxMs: Int = 6000
) async -> [T] {
    let box = Box<T>()
    let task = Task {
        for await x in stream { box.append(x) }
    }
    var lastCount = -1
    var idle = 0
    var total = 0
    while idle < idleMs && total < maxMs {
        try? await Task.sleep(nanoseconds: 50_000_000)
        total += 50
        let c = box.snapshot.count
        if c == lastCount { idle += 50 } else {
            idle = 0
            lastCount = c
        }
    }
    task.cancel()
    return box.snapshot
}

// MARK: - C1: the four load-bearing features

final class C1FeatureTests: XCTestCase {

    // C1a — classes with methods; method returning another exported class; constraint metadata.
    func testC1aClassRoundTripAndConstraints() throws {
        let facet = ProfileFacet()
        XCTAssertEqual(facet.version(), 0)
        XCTAssertFalse(facet.snapshot().exists)

        let draft = facet.checkout() // class-returning-class
        XCTAssertEqual(draft.draftId(), 1)
        XCTAssertEqual(draft.status(), .live)

        // Constraint metadata crosses as typed data — no literals needed shell-side.
        let constraints = facet.constraintsFor(field: .username)
        XCTAssertTrue(constraints.contains(.required))
        XCTAssertTrue(constraints.contains(.lenChars(min: 3, max: 20)))
        XCTAssertTrue(constraints.contains(.custom(name: "ascii_alnum_underscore")))
    }

    // C1b — Result methods throw typed payload-carrying enums.
    func testC1bTypedErrorPayloads() throws {
        let facet = ProfileFacet()
        let draft = facet.checkout()

        do {
            try draft.trySetUsername(raw: "ab")
            XCTFail("expected tooShort")
        } catch let e as FfiUsernameError {
            XCTAssertEqual(e, .tooShort(min: 3, actual: 2))
        }

        do {
            try draft.trySetEmail(raw: "not-an-email")
            XCTFail("expected invalid")
        } catch let e as FfiEmailError {
            XCTAssertEqual(e, .invalid)
        }

        do {
            try draft.trySetAvailability(start: date(2026, 5, 1), end: date(2026, 4, 1))
            XCTFail("expected startAfterEnd")
        } catch let e as FfiDateRangeError {
            XCTAssertEqual(e, .startAfterEnd(start: date(2026, 5, 1), end: date(2026, 4, 1)))
        }

        // A failed try_set is RECORDED (Invalid { raw }): the attempt survives in the snapshot.
        let snap = draft.snapshot()
        XCTAssertEqual(snap.username.text, "ab")
        XCTAssertFalse(snap.username.valid)
        XCTAssertEqual(snap.username.errorKey, "too_short")
        XCTAssertTrue(snap.username.dirty)

        // And sanitization applies on the valid path (trim).
        try draft.trySetUsername(raw: "  alice  ")
        XCTAssertEqual(draft.snapshot().username.text, "alice")
    }

    // C1b — the validation report as structured data (nested records, vecs, enum fields).
    func testC1bValidationReportData() throws {
        let facet = ProfileFacet()
        let draft = facet.checkout()
        try draft.trySetUsername(raw: "corp_bob")
        // The evolved core (C16) demands a uniqueness verdict once username is dirty;
        // satisfy it so the report isolates the corporate_email rule below.
        XCTAssertTrue(draft.completeUsernameCheck(token: draft.beginUsernameCheck(), unique: true))
        try draft.trySetName(raw: "Bob")
        try draft.trySetEmail(raw: "bob@example.com") // violates corporate_email
        try draft.trySetAvailability(start: date(2026, 7, 1), end: date(2026, 7, 2))

        let report = draft.validate()
        XCTAssertFalse(report.ok)
        XCTAssertTrue(report.fieldErrors.isEmpty)
        XCTAssertEqual(report.ruleErrors.count, 1)
        let rule = report.ruleErrors[0]
        XCTAssertEqual(rule.rule, "corporate_email")
        XCTAssertEqual(rule.pins, [.email])
        XCTAssertEqual(rule.error.key, "corporate_email_domain")
        XCTAssertTrue(rule.error.params.contains(FfiParam(name: "expected", value: "corp.example")))

        try draft.trySetEmail(raw: "bob@corp.example")
        XCTAssertTrue(draft.validate().ok)
    }

    // C1a — full lifecycle over the boundary: checkout → edit → live rebase → conflict →
    // resolve → submit (class as parameter) → canonical updated → handle consumed.
    func testC1aLifecycleRebaseConflictSubmit() throws {
        let facet = ProfileFacet()
        try seedCanonical(facet)

        let draft = facet.checkout()
        try draft.trySetName(raw: "Bobby") // dirty

        // Canonical moves underneath (live rebase): name → conflict, others adopt silently.
        try facet.applyCanonical(
            username: "alice",
            name: "Carol",
            email: "alice@new.example",
            start: date(2026, 7, 1),
            end: date(2026, 7, 31)
        )
        XCTAssertEqual(draft.conflicts(), [.name])
        let snap = draft.snapshot()
        XCTAssertTrue(snap.name.conflicted)
        XCTAssertEqual(snap.name.text, "Bobby") // yours preserved
        XCTAssertEqual(snap.name.theirs, "Carol")
        XCTAssertEqual(snap.email.text, "alice@new.example") // non-dirty adopted

        // Submit refuses while conflicted — typed, and the draft survives.
        do {
            try facet.submit(draft: draft)
            XCTFail("expected conflicted")
        } catch let e as FfiSubmitError {
            XCTAssertEqual(e, .conflicted(fields: [.name]))
        }
        XCTAssertEqual(draft.status(), .live)

        draft.resolveKeepMine(field: .name)
        XCTAssertEqual(draft.conflicts(), [])
        try facet.submit(draft: draft) // exported class as parameter
        XCTAssertEqual(facet.snapshot().name, "Bobby")
        XCTAssertEqual(draft.status(), .consumed)

        // The consumed handle is dead, typed-ly.
        do {
            try facet.submit(draft: draft)
            XCTFail("expected draftClosed")
        } catch let e as FfiSubmitError {
            XCTAssertEqual(e, .draftClosed)
        }
        do {
            try draft.trySetName(raw: "x")
            XCTFail("expected draftClosed")
        } catch let e as FfiPersonNameError {
            XCTAssertEqual(e, .draftClosed)
        }
    }

    // C1a/C1b — refused submits are typed and non-destructive; orphaning crosses the boundary.
    func testC1SubmitRefusalsAndOrphan() throws {
        let facet = ProfileFacet()
        try seedCanonical(facet)

        let draft = facet.checkout()
        try draft.trySetUsername(raw: "corp_alice") // valid set, but rule now fails
        XCTAssertTrue(draft.completeUsernameCheck(token: draft.beginUsernameCheck(), unique: true))
        do {
            try facet.submit(draft: draft)
            XCTFail("expected validation")
        } catch let e as FfiSubmitError {
            guard case let .validation(report) = e else {
                return XCTFail("expected .validation, got \(e)")
            }
            XCTAssertEqual(report.ruleErrors.first?.rule, "corporate_email")
        }
        // Draft survived the refusal — fix and resubmit.
        XCTAssertEqual(draft.status(), .live)
        try draft.trySetEmail(raw: "alice@corp.example")
        try facet.submit(draft: draft)
        XCTAssertEqual(facet.snapshot().username, "corp_alice")

        // Orphaning: canonical deleted while a draft is open.
        let draft2 = facet.checkout()
        facet.deleteCanonical()
        XCTAssertEqual(draft2.status(), .orphaned)
        do {
            try facet.submit(draft: draft2)
            XCTFail("expected orphaned")
        } catch let e as FfiSubmitError {
            XCTAssertEqual(e, .orphaned)
        }
    }

    // Single-flight across the boundary: latest begin wins, stale completion ignored (I10).
    func testC1SingleFlightCheckAcrossBoundary() throws {
        let facet = ProfileFacet()
        let draft = facet.checkout()
        try draft.trySetUsername(raw: "alice")

        let t1 = draft.beginUsernameCheck()
        let t2 = draft.beginUsernameCheck() // supersedes t1
        XCTAssertNotEqual(t1, 0)
        XCTAssertNotEqual(t2, 0)

        // Pending check blocks validation, as data.
        XCTAssertTrue(draft.validate().ruleErrors.contains { $0.error.key == "username_check_pending" })

        XCTAssertFalse(draft.completeUsernameCheck(token: t1, unique: true)) // stale → ignored
        XCTAssertTrue(draft.completeUsernameCheck(token: t2, unique: false)) // latest wins
        XCTAssertTrue(draft.validate().ruleErrors.contains { $0.error.key == "username_taken" })

        let t3 = draft.beginUsernameCheck()
        XCTAssertTrue(draft.completeUsernameCheck(token: t3, unique: true))
        XCTAssertFalse(draft.validate().ruleErrors.contains { $0.rule == "username_unique" })
    }

    // C1d — #[export(single_threaded)]: generated Swift has NO thread guard (inspected);
    // verify calls work and record that safety is entirely by convention.
    func testC1dSingleThreadedProbe() {
        let probe = SingleThreadedProbe()
        XCTAssertEqual(probe.increment(), 1)
        XCTAssertEqual(probe.increment(), 2)
        var offMain: UInt64 = 0
        DispatchQueue.global().sync {
            offMain = probe.increment() // sequential off-main call: no guard, no crash
        }
        XCTAssertEqual(offMain, 3)
        print("PROBE single_threaded: no generated guard; off-main sequential call succeeded")
    }
}

// MARK: - C2: the observation contract

final class C2ObservationTests: XCTestCase {

    // C2a(i) — burst of 100 into the default-capacity (256) stream.
    func testC2aBurstDefaultCapacity() async throws {
        let facet = ProfileFacet()
        let stream = facet.snapshots()
        facet.emitBurst(count: 100)
        let got = await collectUntilIdle(stream)
        let versions = got.map(\.version)
        XCTAssertEqual(versions, versions.sorted(), "delivery must preserve order")
        print("PROBE burst cap=256: delivered=\(got.count)/100 last=\(versions.last.map(String.init) ?? "none")")
        XCTAssertEqual(versions.last, 100, "final value must arrive on an under-capacity burst")
        XCTAssertEqual(got.count, 100, "capacity 256 should deliver all 100")
    }

    // C2a(ii) — the same burst against a capacity-1 stream: the naive `Latest` candidate.
    // Assertions are deliberately loose (ordering only); the deliverable is the recorded
    // behavior — especially whether the FINAL value survives.
    func testC2aBurstCapacityOne() async throws {
        let facet = ProfileFacet()
        let stream = facet.snapshotsLatest()
        facet.emitBurst(count: 100)
        let got = await collectUntilIdle(stream)
        let versions = got.map(\.version)
        XCTAssertEqual(versions, versions.sorted(), "delivery must preserve order")
        XCTAssertFalse(got.isEmpty, "at least one event must arrive")
        let finalArrived = versions.last == 100
        print("PROBE burst cap=1: delivered=\(got.count)/100 last=\(versions.last.map(String.init) ?? "none") finalArrived=\(finalArrived)")
    }

    private func waitUntilIdle(_ box: Box<UInt64>, idleMs: Int = 400, maxMs: Int = 5000) async {
        var last = -1
        var idle = 0
        var total = 0
        while idle < idleMs && total < maxMs {
            try? await Task.sleep(nanoseconds: 50_000_000)
            total += 50
            let c = box.snapshot.count
            if c == last { idle += 50 } else {
                idle = 0
                last = c
            }
        }
    }

    // C2b — the wake-and-read `Latest` encoding: capacity-1 wake stream + snapshot() getter.
    // A dropped wake *should* be harmless (a drop implies a wake is pending), so the pattern
    // *should* converge to the final truth. This is a CHARACTERIZING probe: it records
    // whether convergence holds, and — if the stream stalls — whether a later push revives
    // it (distinguishing a stalled drain loop from ordinary coalescing).
    func testC2bWakeAndReadStallProbe() async throws {
        let facet = ProfileFacet()
        let reads = Box<UInt64>()
        let wakeStream = facet.wakes()
        let task = Task {
            for await _ in wakeStream {
                reads.append(facet.snapshot().version)
            }
        }
        defer { task.cancel() }

        for i in 1...100 {
            try facet.applyCanonical(
                username: "alice",
                name: "Name\(i)",
                email: "alice@example.com",
                start: date(2026, 7, 1),
                end: date(2026, 7, 31)
            )
        }
        await waitUntilIdle(reads)

        let seen = reads.snapshot
        XCTAssertFalse(seen.isEmpty)
        XCTAssertEqual(facet.snapshot().name, "Name100") // core truth is intact regardless
        let converged = seen.last == 100
        print("PROBE wake-and-read cap=1: wakes-consumed=\(seen.count)/100 finalRead=\(seen.last ?? 0) converged=\(converged)")

        // Revival check: one more real mutation after the stream went quiet.
        let before = reads.snapshot.count
        try facet.applyCanonical(
            username: "alice", name: "Name101", email: "alice@example.com",
            start: date(2026, 7, 1), end: date(2026, 7, 31)
        )
        try await Task.sleep(nanoseconds: 800_000_000)
        let revived = reads.snapshot.count > before
        print("PROBE wake-and-read cap=1: revivedAfterNewPush=\(revived) (false = drain loop permanently stalled)")
    }

    // C2b′ — same shape against the default-capacity (256) snapshot stream, pushed
    // incrementally (not preloaded like C2a): does capacity change stall behavior?
    func testC2bIncrementalDefaultCapacityStallProbe() async throws {
        let facet = ProfileFacet()
        let reads = Box<UInt64>()
        let stream = facet.snapshots()
        let task = Task {
            for await snap in stream {
                reads.append(snap.version)
            }
        }
        defer { task.cancel() }

        for i in 1...100 {
            try facet.applyCanonical(
                username: "alice",
                name: "Incr\(i)",
                email: "alice@example.com",
                start: date(2026, 7, 1),
                end: date(2026, 7, 31)
            )
        }
        await waitUntilIdle(reads)

        let seen = reads.snapshot
        XCTAssertFalse(seen.isEmpty)
        XCTAssertEqual(seen, seen.sorted(), "delivery must preserve order")
        let converged = seen.last == 100
        let before = seen.count
        try facet.applyCanonical(
            username: "alice", name: "Incr101", email: "alice@example.com",
            start: date(2026, 7, 1), end: date(2026, 7, 31)
        )
        try await Task.sleep(nanoseconds: 800_000_000)
        let revived = reads.snapshot.count > before
        print("PROBE incremental cap=256: delivered=\(before)/100 last=\(seen.last ?? 0) converged=\(converged) revivedAfterNewPush=\(revived)")
    }

    // C2b″ — batch (pull) mode: consumer-driven popBatch, no continuation machinery.
    // This is the candidate reliable path if the push modes stall.
    func testC2bBatchModePullIsReliable() async throws {
        let facet = ProfileFacet()
        let sub = facet.snapshotsBatch()
        facet.emitBurst(count: 100)

        var got: [UInt64] = []
        for _ in 0..<50 {
            let batch = sub.popBatch(maxCount: 32)
            got.append(contentsOf: batch.map(\.version))
            if got.count >= 100 { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertEqual(got.count, 100, "batch pull must deliver everything under capacity")
        XCTAssertEqual(got, got.sorted())
        XCTAssertEqual(got.last, 100)

        // And it keeps working after going quiet (no stall by construction).
        try seedCanonical(facet)
        try await Task.sleep(nanoseconds: 100_000_000)
        let more = sub.popBatch(maxCount: 32)
        XCTAssertFalse(more.isEmpty, "pull path must see later pushes")
        print("PROBE batch-mode pull: 100/100 delivered in order; later push visible=\(!more.isEmpty)")
    }

    // C2d — which thread does a callback-mode stream fire on?
    func testC2dCallbackThread() async throws {
        let facet = ProfileFacet()
        let threads = Box<String>()
        let cancellable = facet.wakeCallbacks { _ in
            // NOTE: no facet calls in here — probing thread identity only.
            threads.append("main=\(Thread.isMainThread) \(Thread.current)")
        }
        defer { cancellable.cancel() }

        facet.emitBurst(count: 1) // push from this (XCTest) thread
        DispatchQueue.global().sync {
            facet.emitBurst(count: 1) // push from a background thread
        }
        try await Task.sleep(nanoseconds: 500_000_000)

        let seen = threads.snapshot
        XCTAssertFalse(seen.isEmpty)
        for t in seen { print("PROBE callback-thread: \(t)") }
    }
}

// MARK: - measurements (run with -c release)

final class Measurements: XCTestCase {

    private func measure(_ label: String, iterations: Int, unit: String = "ns", _ block: () -> Void) {
        // Warm-up.
        for _ in 0..<min(iterations / 10, 1000) { block() }
        let start = DispatchTime.now().uptimeNanoseconds
        for _ in 0..<iterations { block() }
        let elapsed = DispatchTime.now().uptimeNanoseconds - start
        let per = Double(elapsed) / Double(iterations)
        let shown = unit == "us" ? per / 1000 : per
        print("MEASURE \(label): \(String(format: "%.2f", shown)) \(unit)/call (\(iterations)x)")
    }

    func testMeasurements() async throws {
        let facet = ProfileFacet()
        try seedCanonical(facet)
        let draft = facet.checkout()

        // Baseline no-op method call.
        measure("noop", iterations: 100_000) { facet.noop() }

        // The keystroke bet, Apple-side: String in → sanitize+validate → typed Result out.
        measure("try_set_username (valid, 12 chars)", iterations: 10_000) {
            try? draft.trySetUsername(raw: "alice_writes")
        }
        measure("try_set_username (invalid, too short)", iterations: 10_000) {
            try? draft.trySetUsername(raw: "ab")
        }

        // Draft snapshot fetch (4 field views) and facet snapshot fetch.
        measure("draft.snapshot()", iterations: 10_000, unit: "us") { _ = draft.snapshot() }
        measure("facet.snapshot()", iterations: 10_000, unit: "us") { _ = facet.snapshot() }

        // C2c — window-scale payload: 50 rows with strings.
        measure("window_rows(50)", iterations: 2_000, unit: "us") {
            _ = facet.windowRows(offset: 5000, len: 50)
        }
        let rows = facet.windowRows(offset: 5000, len: 50)
        XCTAssertEqual(rows.count, 50)

        // validate() round-trip (report crosses as data).
        measure("draft.validate()", iterations: 10_000, unit: "us") { _ = draft.validate() }
    }

    // C2e — input→snapshot latency through the wake stream, measured end to end.
    func testC2eInputToSnapshotLatency() async throws {
        let facet = ProfileFacet()
        let latencies = Box<Double>()
        let sent = Box<UInt64>() // t0 per version, nanoseconds
        let wakeStream = facet.wakes()
        let task = Task {
            for await _ in wakeStream {
                let now = DispatchTime.now().uptimeNanoseconds
                let starts = sent.snapshot
                if let t0 = starts.last {
                    latencies.append(Double(now - t0) / 1000.0)
                }
            }
        }

        for i in 1...50 {
            sent.append(DispatchTime.now().uptimeNanoseconds)
            try facet.applyCanonical(
                username: "alice",
                name: "Latency\(i)",
                email: "alice@example.com",
                start: date(2026, 7, 1),
                end: date(2026, 7, 31)
            )
            try await Task.sleep(nanoseconds: 20_000_000) // spaced: measuring latency, not rate
        }
        try await Task.sleep(nanoseconds: 300_000_000)
        task.cancel()

        let ls = latencies.snapshot.sorted()
        XCTAssertFalse(ls.isEmpty)
        let median = ls[ls.count / 2]
        print("MEASURE input→snapshot latency: median=\(String(format: "%.1f", median))µs "
            + "min=\(String(format: "%.1f", ls.first ?? 0))µs max=\(String(format: "%.1f", ls.last ?? 0))µs (n=\(ls.count))")
    }
}
