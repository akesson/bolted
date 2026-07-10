import Foundation
import XCTest
import GenProfileFfi

/// Feature 2 — async streams (snapshots). BoltFFI exposes each `#[ffi_stream]` as a Swift
/// `AsyncStream` with an `.unbounded` buffer, fed by a background poll loop that drains the Rust
/// side's bounded, drop-newest ring in batches. So there are TWO buffers; see the report.
final class StreamTests: XCTestCase {
    /// End to end: a mutation produces a snapshot the Swift consumer receives, with the new value.
    func testSnapshotDeliveredOnMutation() async throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        let stream = draft.snapshots()
        let box = SnapshotBox()
        let received = expectation(description: "snapshot delivered")

        let task = Task {
            for await snap in stream {
                if case .valid(let value) = snap.username.validity, value == "alice" {
                    box.value = snap
                    received.fulfill()
                    break
                }
            }
        }
        try draft.trySetUsername(raw: "alice")
        await fulfillment(of: [received], timeout: 3)
        task.cancel()

        XCTAssertEqual(box.value?.username.validity, .valid(value: "alice"))
    }

    /// Subscribe-race: a fresh subscription replays NOTHING — it delivers only future events. A
    /// value set BEFORE subscribing is visible via the `snapshot()` recovery getter but is not
    /// re-delivered on the stream. The `version` stamp is how a get-current-then-subscribe caller
    /// (step 03's SwiftUI view) detects a missed event in the gap.
    func testFreshSubscriptionIsFutureOnly() async throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        try draft.trySetUsername(raw: "before") // BEFORE subscribing
        XCTAssertEqual(draft.snapshot().username.validity, .valid(value: "before"))

        let stream = draft.snapshots() // subscribe AFTER the mutation
        let box = SnapshotBox()
        let received = expectation(description: "future-only delivery")
        let task = Task {
            for await snap in stream {
                box.value = snap
                received.fulfill()
                break
            }
        }
        try draft.trySetUsername(raw: "after")
        await fulfillment(of: [received], timeout: 3)
        task.cancel()

        // The first (and only) delivered event is "after" — "before" was never replayed.
        XCTAssertEqual(box.value?.username.validity, .valid(value: "after"))
    }

    /// Overflow / drop-newest kill-bar: after a burst of mutations against a subscriber that never
    /// consumes, the current state is ALWAYS recoverable via the `snapshot()` getter — so
    /// drop-newest is not a kill (the `observe` verb's "always-valid current state" holds).
    func testBurstIsRecoverableViaSnapshotGetter() throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        _ = draft.snapshotsSmall() // a tiny (4-slot) ring subscriber we deliberately never drain

        for i in 3..<60 {
            try? draft.trySetUsername(raw: "user\(i)")
        }
        // Regardless of any ring drops, current state is authoritative and complete.
        XCTAssertEqual(draft.snapshot().username.validity, .valid(value: "user59"))
    }

    /// Main-actor consumption: consuming the stream from a `@MainActor` task delivers snapshots on
    /// the main thread (step 03 lives on the main actor). Records the delivery thread.
    @MainActor
    func testMainActorConsumption() async throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        let stream = draft.snapshots()
        let box = SnapshotBox()
        let received = expectation(description: "main-actor delivery")

        let task = Task { @MainActor in
            for await snap in stream {
                XCTAssertTrue(Thread.isMainThread) // delivery resumes on the main actor
                box.value = snap
                received.fulfill()
                break
            }
        }
        try draft.trySetUsername(raw: "alice")
        await fulfillment(of: [received], timeout: 3)
        task.cancel()

        XCTAssertEqual(box.value?.username.validity, .valid(value: "alice"))
    }

    /// A stream consumer that reacts to a snapshot by immediately calling draft methods must not
    /// deadlock (the wrapper emits outside its lock).
    func testStreamConsumerReentrancyDoesNotDeadlock() async throws {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        let stream = draft.snapshots()
        let done = expectation(description: "reentrant call from consumer")

        let task = Task {
            for await _ in stream {
                _ = draft.validate()
                _ = draft.snapshot()
                done.fulfill()
                break
            }
        }
        try draft.trySetUsername(raw: "alice")
        await fulfillment(of: [done], timeout: 3)
        task.cancel()
    }
}
