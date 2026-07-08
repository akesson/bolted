import XCTest
import SpikeProfileFfi

/// Feature 4 — callback traits (capabilities). A Swift-implemented `UniquenessChecker` is invoked
/// from Rust and drives the single-flight begin/complete; reentrancy must not deadlock.
final class CallbackTests: XCTestCase {
    /// A `.taken` verdict blocks validation (a `username_unique` rule violation); a later `.unique`
    /// verdict unblocks it. Mirrors the step-01 behaviour tests, now across the FFI boundary.
    func testCheckerBlocksThenUnblocks() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertTrue(draft.validate().isOK) // clean checkout of a valid canonical

        draft.setUniquenessChecker(checker: StubChecker(.taken))
        XCTAssertTrue(draft.runUsernameCheck())
        XCTAssertTrue(draft.validate().ruleNames.contains("username_unique"))

        draft.setUniquenessChecker(checker: StubChecker(.unique))
        XCTAssertTrue(draft.runUsernameCheck())
        XCTAssertFalse(draft.validate().ruleNames.contains("username_unique"))
    }

    /// With no checker set, driving the check is a no-op (returns false) and does not block.
    func testNoCheckerIsNoop() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertFalse(draft.runUsernameCheck())
        XCTAssertTrue(draft.validate().isOK)
    }

    /// Reentrancy / deadlock: the Swift checker, while being called, synchronously re-enters the
    /// SAME draft (a read AND a mutation). The wrapper's rule — never hold the `Mutex` across an
    /// outcall — is what makes this safe. If this returns, there is no deadlock.
    func testCheckerReentrancyDoesNotDeadlock() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        let checker = ReentrantChecker()
        checker.draft = draft
        draft.setUniquenessChecker(checker: checker)

        XCTAssertTrue(draft.runUsernameCheck()) // would hang if the outcall held the lock
        XCTAssertTrue(checker.reentered)
        // the reentrant mutation took effect
        XCTAssertEqual(draft.snapshot().name.validity, .valid(value: "Reentrant"))
    }
}
