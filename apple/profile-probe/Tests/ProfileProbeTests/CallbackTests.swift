import XCTest
import GenProfileFfi

/// Feature 4 — callback traits (capabilities). A Swift-implemented `UsernameChecker` is invoked
/// from Rust and drives the single-flight begin/complete; reentrancy must not deadlock.
final class CallbackTests: XCTestCase {
    /// A `.fail` verdict blocks validation (a `username_unique` rule violation); a later `.pass`
    /// verdict unblocks it. Mirrors the step-01 behaviour tests, now across the FFI boundary.
    func testCheckerBlocksThenUnblocks() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertTrue(draft.validate().isOK) // clean checkout of a valid canonical

        draft.setUsernameChecker(checker: StubChecker(.fail))
        XCTAssertTrue(try draft.runUsernameCheck())
        XCTAssertTrue(draft.validate().ruleNames.contains("username_unique"))

        draft.setUsernameChecker(checker: StubChecker(.pass))
        XCTAssertTrue(try draft.runUsernameCheck())
        XCTAssertFalse(draft.validate().ruleNames.contains("username_unique"))
    }

    /// With no checker set, driving the check is a no-op (returns false) and does not block.
    func testNoCheckerIsNoop() throws {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        let draft = store.checkout()
        XCTAssertFalse(try draft.runUsernameCheck())
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
        draft.setUsernameChecker(checker: checker)

        XCTAssertTrue(try draft.runUsernameCheck()) // would hang if the outcall held the lock
        XCTAssertTrue(checker.reentered)
        // the reentrant mutation took effect
        XCTAssertEqual(draft.snapshot().name.validity, .valid(value: "Reentrant"))
    }
}
