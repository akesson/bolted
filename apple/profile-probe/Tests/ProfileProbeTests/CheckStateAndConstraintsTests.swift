import Foundation
import XCTest
import SpikeProfileFfi

/// Step-03 FFI additions (Deliverable B): the async-check sub-state now reaches the snapshot
/// (step-02 finding 7) and constraint metadata crosses the boundary. These probes prove both
/// project correctly, including a *genuinely observed* `Pending` (the snapshot the spinner binds
/// to) and the value-bound reset (invariant 13) seen through FFI.
final class CheckStateAndConstraintsTests: XCTestCase {
    // ---- check sub-state in snapshots ---------------------------------------------------------

    /// Unchecked → Passed, and Unchecked → Failed carrying the `username_taken` error data. This is
    /// the state a shell renders directly, rather than inferring it from a `validate()` rule error.
    func testPassedAndFailedSubStateInSnapshot() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)

        draft.setUniquenessChecker(checker: StubChecker(.unique))
        XCTAssertTrue(draft.runUsernameCheck())
        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)

        // a taken verdict surfaces as .failed carrying the check's ErrorData (username value
        // unchanged between checks, so no reset intervenes).
        draft.setUniquenessChecker(checker: StubChecker(.taken))
        XCTAssertTrue(draft.runUsernameCheck())
        guard case .failed(let error) = draft.snapshot().usernameCheck else {
            return XCTFail("expected .failed sub-state")
        }
        XCTAssertEqual(error.key, "username_taken")
    }

    /// The headline for the spinner: a `Pending` sub-state is genuinely observable on the draft
    /// stream *while the check is in flight*. A blocking checker (released by the test) holds the
    /// callout open; the wrapper emits Pending BEFORE the lock-free callout, so a stream consumer
    /// sees `.unchecked`-then-`.pending`-then-`.passed` without the check ever completing early.
    func testPendingObservableOnStreamDuringCheck() async throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)

        let checker = BlockingChecker(.unique)
        draft.setUniquenessChecker(checker: checker)

        // Subscribe first — streams are future-only — then drive the (blocking) check off-thread.
        let stream = draft.snapshots()
        let sawPending = expectation(description: "pending observed on the stream")
        sawPending.assertForOverFulfill = false
        let sawPassed = expectation(description: "passed observed on the stream")
        sawPassed.assertForOverFulfill = false

        let observer = Task {
            for await snap in stream {
                switch snap.usernameCheck {
                case .pending: sawPending.fulfill()
                case .passed: sawPassed.fulfill()
                default: break
                }
            }
        }

        let driver = CheckDriver(draft)
        DispatchQueue.global().async { driver.run() }

        await fulfillment(of: [sawPending], timeout: 5)  // emitted before the callout completes
        checker.releaseChecker()                         // let the callout return .unique
        await fulfillment(of: [sawPassed], timeout: 5)   // now it settles
        observer.cancel()

        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)
    }

    /// Invariant 13 through FFI: completing a check then editing the username to a DIFFERENT value
    /// resets the sub-state to `.unchecked`; editing to the SAME value leaves the verdict standing.
    func testResetOnEditVisibleThroughFfi() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()

        draft.setUniquenessChecker(checker: StubChecker(.unique))
        XCTAssertTrue(draft.runUsernameCheck())
        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)

        try draft.trySetUsername(raw: "different")             // value moved -> reset
        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)

        XCTAssertTrue(draft.runUsernameCheck())                // re-endorse "different"
        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)
        try draft.trySetUsername(raw: "different")             // same value -> verdict stands
        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)
    }

    /// take-theirs on a conflicted username moves the value and resets the check (i13); keep-mine
    /// preserves the value and the verdict. Visible entirely through the snapshot.
    func testResolutionResetOnUsernameThroughFfi() throws {
        func conflictedDraft() throws -> ProfileDraftFfi {
            let store = ProfileStoreFfi()
            try store.applyCanonical(values: validValues())      // username "alice"
            let draft = store.checkout()
            try draft.trySetUsername(raw: "mine")                // dirty
            draft.setUniquenessChecker(checker: StubChecker(.unique))
            XCTAssertTrue(draft.runUsernameCheck())              // verdict endorses "mine"
            XCTAssertEqual(draft.snapshot().usernameCheck, .passed)
            var conflicting = validValues()
            conflicting.username = "theirs"
            try store.applyCanonical(values: conflicting)        // conflict on username
            XCTAssertTrue(draft.snapshot().conflicts.contains(.username))
            XCTAssertEqual(draft.snapshot().usernameCheck, .passed) // yours preserved -> stands
            return draft
        }

        let takeDraft = try conflictedDraft()
        takeDraft.resolveTakeTheirs(field: .username)
        XCTAssertEqual(takeDraft.snapshot().usernameCheck, .unchecked)

        let keepDraft = try conflictedDraft()
        keepDraft.resolveKeepMine(field: .username)
        XCTAssertEqual(keepDraft.snapshot().usernameCheck, .passed)
    }

    // ---- constraint metadata ------------------------------------------------------------------

    /// Constraints round-trip: the exact `Required` + intrinsic constraints for every field. The
    /// app builds counters / max-length / required markers from THIS — never a Swift-side literal.
    func testConstraintsRoundTrip() {
        let store = ProfileStoreFfi()
        XCTAssertEqual(
            store.constraints(field: .username),
            [.required, .lenChars(min: 3, max: 20), .custom(key: "ascii_alnum_underscore")]
        )
        XCTAssertEqual(
            store.constraints(field: .name),
            [.required, .lenChars(min: 1, max: 30)]
        )
        XCTAssertEqual(
            store.constraints(field: .email),
            [.required, .custom(key: "email")]
        )
        XCTAssertEqual(
            store.constraints(field: .availability),
            [.required, .custom(key: "start_le_end")]
        )
        // every profile field leads with Required (the source of the UI's required marker).
        for field: ProfileFieldId in [.username, .name, .email, .availability] {
            XCTAssertEqual(store.constraints(field: field).first, .required)
        }
    }
}

// --- Step-03 test helpers ----------------------------------------------------------------------

/// Blocks inside `checkUnique` until the test releases it, so a stream consumer can observe the
/// in-flight `Pending` snapshot. `@unchecked Sendable`: the semaphore and the immutable verdict are
/// the only state and both are thread-safe.
final class BlockingChecker: UniquenessChecker, @unchecked Sendable {
    private let release = DispatchSemaphore(value: 0)
    private let verdict: UniquenessVerdictFfi
    init(_ verdict: UniquenessVerdictFfi) { self.verdict = verdict }
    func checkUnique(username: String) -> UniquenessVerdictFfi {
        release.wait()
        return verdict
    }
    func releaseChecker() { release.signal() }
}

/// Wraps a (non-Sendable) draft handle so it can be driven from a background queue. Safe because
/// `ProfileDraftFfi` is internally synchronised (an `Arc<Mutex<…>>` on the Rust side).
final class CheckDriver: @unchecked Sendable {
    private let draft: ProfileDraftFfi
    init(_ draft: ProfileDraftFfi) { self.draft = draft }
    func run() { _ = draft.runUsernameCheck() }
}
