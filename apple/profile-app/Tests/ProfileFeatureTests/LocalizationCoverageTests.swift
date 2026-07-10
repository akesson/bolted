import XCTest
import GenProfileFfi

@testable import ProfileFeature

/// **Step-06 friction 7, made into a test — the Apple half, added in step 12.**
///
/// C16 introduced the `username_check_required` error key. The Kotlin shell got a template and, in
/// step 07, a test to keep it honest. *This* shell — the one the bug actually shipped on — never got
/// either, and would have rendered a raw identifier to a user on C16's most common refusal path.
/// This is that test, at last on the platform it was needed.
///
/// Like the Kotlin sibling it **drives the real core** rather than asserting against a hand-kept key
/// list. Step 12 M5 tried the step doc's proposed "generator-emitted declared key set" and found it
/// cannot be complete: rule error keys are runtime strings inside the `#[bolted::rules]` impl (the
/// generator never sees `corporate_email_domain`), `required` is a `bolted-core` constant, and
/// `draft_orphaned` is shell-supplied. Drive-the-core sees every key the core can actually emit, so
/// it is the complete design — the same conclusion step 11 reached for Kotlin.
final class LocalizationCoverageTests: XCTestCase {

    /// Every rendered error must have a template and differ from its key — the key IS the fallback.
    private func assertRenders(_ error: ErrorData, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(
            Localization.hasTemplate(error.key),
            "no template for '\(error.key)': the app would show a user a raw identifier",
            file: file, line: line)
        let message = Localization.message(error)
        XCTAssertNotEqual(message, error.key, "'\(error.key)' rendered as its own key", file: file, line: line)
        XCTAssertFalse(
            message.contains("{"), "'\(error.key)' left a placeholder unfilled: \(message)",
            file: file, line: line)
    }

    private func seeded() throws -> ProfileStoreFfi {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: coverageSeed())
        return store
    }

    private func errorsOf(_ draft: ProfileDraftFfi) -> [ErrorData] {
        let report = draft.validate()
        return report.fieldErrors.map { $0.error } + report.ruleErrors.map { $0.error }
    }

    /// Tier 1, every value type, every failure mode the core can produce for it.
    func testEveryTier1ErrorRenders() throws {
        let store = try seeded()
        let draft = store.checkout()

        try? draft.trySetUsername(raw: "ab")  // too_short
        errorsOf(draft).forEach { assertRenders($0) }
        try? draft.trySetUsername(raw: String(repeating: "x", count: 21))  // too_long
        errorsOf(draft).forEach { assertRenders($0) }
        try? draft.trySetUsername(raw: "bad!name")  // invalid_chars
        errorsOf(draft).forEach { assertRenders($0) }

        try? draft.trySetEmail(raw: "nope")  // invalid_email
        errorsOf(draft).forEach { assertRenders($0) }

        try? draft.trySetAvailability(
            raw: AvailabilityRaw(
                start: PlainDate(year: 2026, month: 12, day: 31),
                end: PlainDate(year: 2026, month: 1, day: 1)))  // range_reversed
        errorsOf(draft).forEach { assertRenders($0) }
    }

    /// `required` only exists for a create-flow draft: an unseeded store has no canonical to copy.
    func testTheRequiredErrorRenders() {
        let store = ProfileStoreFfi()
        let draft = store.checkout()
        let errors = errorsOf(draft)
        XCTAssertEqual(errors.filter { $0.key == "required" }.count, 4, "all four fields are Unset")
        errors.forEach { assertRenders($0) }
    }

    /// Tier 2, with its params: the sentence names `corp.example`, and the core supplied that word.
    func testTheTier2RuleErrorRendersWithItsParams() throws {
        let store = try seeded()
        let draft = store.checkout()
        draft.setUsernameChecker(checker: FixedChecker(.pass))
        try draft.trySetUsername(raw: "corp_alice")
        _ = try draft.runUsernameCheck()
        try draft.trySetEmail(raw: "alice@other.com")

        let violation = draft.validate().ruleErrors.first { $0.rule == "corporate_email" }
        XCTAssertNotNil(violation, "a corp_ username with a non-corp email violates the rule")
        assertRenders(violation!.error)
        XCTAssertEqual(
            Localization.message(violation!.error),
            "A corp_ username needs a corp.example email (got other.com).")
    }

    /// The async keys: `username_check_required` (progress) and `username_taken` (failure).
    func testTheAsyncCheckKeysRender() throws {
        let store = try seeded()
        let draft = store.checkout()

        // dirty, never checked -> C16 refuses, as PROGRESS
        try draft.trySetUsername(raw: "alice2")
        let required = draft.validate().ruleErrors.first { $0.rule == "username_unique" }
        XCTAssertEqual(required?.error.key, "username_check_required")
        assertRenders(required!.error)

        // a taken verdict -> a real error
        draft.setUsernameChecker(checker: FixedChecker(.fail))
        _ = try draft.runUsernameCheck()
        guard case .failed(let error) = draft.snapshot().usernameCheck else {
            return XCTFail("expected a failed verdict")
        }
        assertRenders(error)
    }

    /// `username_check_pending` cannot be produced from this shell (with a synchronous checker,
    /// begin/complete are atomic in one FFI call, so `Pending` is only ever seen on the stream). Its
    /// template is asserted directly, and this test is the note explaining why it cannot be driven.
    func testThePendingKeyHasATemplate() {
        assertRenders(ErrorData(key: "username_check_pending", params: []))
    }

    /// `draft_orphaned` is shell-supplied: the core reports orphaning as a typed `SubmitError`
    /// variant, not a key, so no driven path emits it — but the app must still be able to say it.
    func testTheOrphanedOutcomeRenders() {
        assertRenders(ErrorData(key: "draft_orphaned", params: []))
    }
}

private func coverageSeed() -> ProfileValues {
    ProfileValues(
        username: "alice",
        name: "Alice Smith",
        email: "alice@example.com",
        availability: AvailabilityRaw(
            start: PlainDate(year: 2026, month: 1, day: 1),
            end: PlainDate(year: 2026, month: 12, day: 31)))
}

private final class FixedChecker: UsernameChecker {
    let verdict: CheckVerdictFfi
    init(_ verdict: CheckVerdictFfi) { self.verdict = verdict }
    func check(value: String) -> CheckVerdictFfi { verdict }
}
