import XCTest
@testable import GenProfileFfi

/// Everything here crosses a real FFI boundary into Rust that nobody wrote by hand.
final class GeneratedBindingsSmokeTests: XCTestCase {

    private func seed(_ store: ProfileStoreFfi, username: String = "ada") throws {
        try store.applyCanonical(values: ProfileValues(
            username: username,
            name: "Ada",
            email: "ada@corp.example",
            availability: AvailabilityRaw(
                start: PlainDate(year: 2026, month: 1, day: 1),
                end: PlainDate(year: 2026, month: 12, day: 31))))
    }

    func testTheSkeletonStillPings() {
        XCTAssertEqual(ping(input: "hi"), "pong: hi")
    }

    /// D24: three `Raw = String` value types share one Swift type, hosted in `bolted-ffi`.
    func testTheSharedTextFieldStateCrossesFromTheDependencyCrate() throws {
        let store = ProfileStoreFfi()
        try seed(store)
        let draft = store.checkout(usernameChecker: nil)
        let snapshot = draft.snapshot()

        let states: [TextFieldState] = [snapshot.username, snapshot.name, snapshot.email]
        for state in states {
            XCTAssertFalse(state.dirty)
            XCTAssertEqual(state.sync, TextFieldSync.inSync)
        }
        XCTAssertEqual(snapshot.username.validity, TextValidity.valid(value: "ada"))
    }

    /// No constraint literal in Swift: the numbers come from the core (ARCHITECTURE §1).
    func testConstraintsCrossAsData() {
        let store = ProfileStoreFfi()
        XCTAssertEqual(store.constraints(field: .username), [
            .required,
            .lenChars(min: 3, max: 20),
            .custom(key: "ascii_alnum_underscore"),
        ])
    }

    /// Tier 1 rejects at the boundary, with a typed Swift error carrying its params.
    func testAnInvalidValueThrowsATypedError() throws {
        let store = ProfileStoreFfi()
        try seed(store)
        let draft = store.checkout(usernameChecker: nil)
        XCTAssertThrowsError(try draft.trySetUsername(raw: "ab")) { error in
            XCTAssertEqual(error as? UsernameErrorFfi, .tooShort(min: 3, actual: 2))
        }
    }

    /// D23. Before step 10 this call returned silently, having done nothing.
    func testAMutatorRefusesASubmittedDraft() throws {
        let store = ProfileStoreFfi()
        try seed(store)
        let draft = store.checkout(usernameChecker: nil)
        try draft.submit()
        XCTAssertFalse(draft.isLive())

        XCTAssertThrowsError(try draft.trySetName(raw: "Grace")) { error in
            XCTAssertEqual(error as? PersonNameErrorFfi, .draftClosed)
        }
        XCTAssertThrowsError(try draft.resolveKeepMine(field: .username)) { error in
            XCTAssertEqual(error as? DraftClosedFfi, .draftClosed)
        }
    }

    /// The generated capability trait, implemented on the Swift side, called from Rust with no lock
    /// held. `failed_key` comes from the declaration, not from this file.
    func testTheGeneratedCheckerCapabilityRoundTrips() throws {
        final class Taken: UsernameChecker {
            var asked: [String] = []
            func check(value: String) -> CheckVerdictFfi {
                asked.append(value)
                return .fail
            }
        }

        let store = ProfileStoreFfi()
        try seed(store)
        let checker = Taken()
        let draft = store.checkout(usernameChecker: checker)
        try draft.trySetUsername(raw: "  grace  ")

        XCTAssertTrue(try draft.runUsernameCheck())

        XCTAssertEqual(checker.asked, ["grace"], "the sanitizer ran before the checker was asked")
        guard case let .failed(error) = draft.snapshot().usernameCheck else {
            return XCTFail("expected a failed verdict, got \(draft.snapshot().usernameCheck)")
        }
        XCTAssertEqual(error.key, "username_taken")

        // C13: moving the checked value discards the verdict bound to it.
        try draft.trySetUsername(raw: "hopper")
        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)
    }

    /// The composite value object, projected by the hand-written `custom` module the generator
    /// demanded and could not write.
    func testTheCompositeCrossesAsARecord() throws {
        let store = ProfileStoreFfi()
        try seed(store)
        let draft = store.checkout(usernameChecker: nil)
        XCTAssertThrowsError(try draft.trySetAvailability(raw: AvailabilityRaw(
            start: PlainDate(year: 2026, month: 6, day: 1),
            end: PlainDate(year: 2026, month: 1, day: 1)))) { error in
            guard case .startAfterEnd = error as? AvailabilityErrorFfi else {
                return XCTFail("expected startAfterEnd, got \(error)")
            }
        }
    }
}
