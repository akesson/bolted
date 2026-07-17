import XCTest
import GenProfileFfi

/// Feature 4 — callback traits (capabilities). A Swift-implemented `UsernameChecker` is invoked
/// from Rust and drives the single-flight begin/complete; reentrancy must not deadlock.
final class CallbackTests: XCTestCase {
    /// A `.fail` verdict blocks validation (a `username_unique` rule violation); a later `.pass`
    /// verdict unblocks it. Mirrors the step-01 behaviour tests, now across the FFI boundary.
    /// Since D34 the capability is fixed at checkout, so "later" is the same checker answering
    /// differently — a verdict is a fact about the value's world, not about checker identity.
    func testCheckerBlocksThenUnblocks() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout(usernameChecker: SequencedChecker([.fail, .pass]))
        XCTAssertTrue(draft.validate().isOK) // clean checkout of a valid canonical

        XCTAssertTrue(try draft.runUsernameCheck())
        XCTAssertTrue(draft.validate().ruleNames.contains("username_unique"))

        XCTAssertTrue(try draft.runUsernameCheck())
        XCTAssertFalse(draft.validate().ruleNames.contains("username_unique"))
    }

    /// A declared-absent capability (`nil` at checkout, D34): driving the check is a no-op
    /// (returns false) and does not block a clean draft.
    func testNoCheckerIsNoop() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout(usernameChecker: nil)
        XCTAssertFalse(try draft.runUsernameCheck())
        XCTAssertTrue(draft.validate().isOK)
    }

    /// Reentrancy / deadlock: the Swift checker, while being called, synchronously re-enters the
    /// SAME draft (a read AND a mutation). The wrapper's rule — never hold the `Mutex` across an
    /// outcall — is what makes this safe. If this returns, there is no deadlock.
    func testCheckerReentrancyDoesNotDeadlock() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let checker = ReentrantChecker()
        let draft = store.checkout(usernameChecker: checker)
        checker.draft = draft

        XCTAssertTrue(try draft.runUsernameCheck()) // would hang if the outcall held the lock
        XCTAssertTrue(checker.reentered)
        // the reentrant mutation took effect
        XCTAssertEqual(draft.snapshot().name.validity, .valid(value: "Reentrant"))
    }
}
