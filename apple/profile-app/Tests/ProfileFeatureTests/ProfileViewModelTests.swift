import XCTest
import SpikeProfileFfi
@testable import ProfileFeature

/// Headless ViewModel probes for the four behaviours on trial (echo rule, conflict UI, live rebase,
/// submit) plus the constraint-derived affordances and the async check. No window is ever created;
/// the `@main` executable exists only for the manual protocol.
@MainActor
final class ProfileViewModelTests: XCTestCase {
    // A fully valid seed (no corp_ prefix, so the tier-2 rule stays quiet).
    func seed() -> ProfileValues {
        ProfileValues(
            username: "alice",
            name: "Alice Smith",
            email: "alice@example.com",
            availability: PlainDateRange(
                start: PlainDate(year: 2026, month: 1, day: 1),
                end: PlainDate(year: 2026, month: 12, day: 31)
            )
        )
    }

    func makeVM(debounce: Duration = .milliseconds(400)) throws -> ProfileViewModel {
        try ProfileViewModel(seed: seed(), debounce: debounce)
    }

    /// Poll the main actor until `condition` holds or the timeout elapses (lets the VM's stream
    /// tasks run between checks).
    func eventually(
        timeout: Duration = .seconds(3), _ condition: () -> Bool
    ) async {
        let start = ContinuousClock.now
        while !condition() {
            if ContinuousClock.now - start > timeout { return }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    // ---- echo rule ---------------------------------------------------------------------------

    /// Typing into a focused field never rewrites its buffer from the core's sanitized value; the
    /// core still sees the sanitized value; on blur the buffer refreshes. This is ARCHITECTURE §6.
    func testEchoRuleFocusedBufferNotRewritten() throws {
        let vm = try makeVM()
        vm.focus(.username)
        vm.usernameText = "  Bob_1  " // leading/trailing spaces
        vm.editUsername()

        XCTAssertEqual(vm.usernameText, "  Bob_1  ") // focused buffer untouched (no cursor jump)
        XCTAssertEqual(vm.snapshot.username.validity, .valid(value: "Bob_1")) // core sanitized

        vm.blur(.username)
        XCTAssertEqual(vm.usernameText, "Bob_1") // refreshes to sanitized value on blur
    }

    /// A rejected edit keeps the user's raw text in the buffer (Invalid.raw), focused or blurred.
    func testEchoRuleInvalidRawPreserved() throws {
        let vm = try makeVM()
        vm.focus(.username)
        vm.usernameText = "ab" // too short
        vm.editUsername()

        XCTAssertEqual(vm.usernameText, "ab")
        guard case .invalid(let raw, _) = vm.snapshot.username.validity else {
            return XCTFail("expected Invalid")
        }
        XCTAssertEqual(raw, "ab")

        vm.blur(.username)
        XCTAssertEqual(vm.usernameText, "ab") // Invalid.raw survives the blur refresh
    }

    // ---- constraint-derived affordances ------------------------------------------------------

    func testConstraintsDriveAffordancesNoLiterals() throws {
        let vm = try makeVM()
        XCTAssertEqual(vm.maxLength(.username), 20)
        XCTAssertEqual(vm.maxLength(.name), 30)
        XCTAssertNil(vm.maxLength(.email)) // email has no LenChars
        for field: ProfileFieldId in [.username, .name, .email, .availability] {
            XCTAssertTrue(vm.isRequired(field))
        }
    }

    // ---- live rebase -------------------------------------------------------------------------

    /// A canonical change to a CLEAN field is adopted silently: the snapshot updates, the field
    /// stays clean, and the (unfocused) buffer refreshes.
    func testLiveRebaseCleanFieldAdopts() async throws {
        let vm = try makeVM()
        vm.applyServerChange(.name("Server Name"))
        await eventually {
            if case .valid(let v) = vm.snapshot.name.validity { return v == "Server Name" }
            return false
        }
        XCTAssertEqual(vm.nameText, "Server Name")
        XCTAssertFalse(vm.isDirty(.name))
    }

    /// A canonical change to a DIRTY field conflicts, preserving yours and exposing theirs.
    func testLiveRebaseDirtyFieldConflicts() async throws {
        let vm = try makeVM()
        vm.focus(.name)
        vm.nameText = "My Name"
        vm.editName()
        vm.blur(.name)

        vm.applyServerChange(.name("Their Name"))
        await eventually { vm.snapshot.conflicts.contains(.name) }

        if case .valid(let v) = vm.snapshot.name.validity {
            XCTAssertEqual(v, "My Name") // yours preserved
        } else {
            XCTFail("expected mine preserved and valid")
        }
        XCTAssertEqual(vm.nameText, "My Name")
        XCTAssertEqual(vm.conflict(.name)?.theirs, "Their Name")
    }

    // ---- conflict resolution -----------------------------------------------------------------

    /// take-theirs refreshes the buffer to theirs and (on username) resets the check; keep-mine
    /// preserves both the value and the verdict.
    func testConflictResolutionAndCheckReset() async throws {
        func conflictedVM() async throws -> ProfileViewModel {
            let vm = try makeVM()
            vm.focus(.username)
            vm.usernameText = "mine1"
            vm.editUsername()
            vm.runCheckNow() // endorse "mine1"
            await eventually { vm.snapshot.usernameCheck == .passed }
            vm.blur(.username)
            vm.applyServerChange(.username("theirs1"))
            await eventually { vm.snapshot.conflicts.contains(.username) }
            XCTAssertEqual(vm.snapshot.usernameCheck, .passed) // yours preserved -> verdict stands
            return vm
        }

        let takeVM = try await conflictedVM()
        takeVM.resolveTakeTheirs(.username)
        XCTAssertEqual(takeVM.usernameText, "theirs1") // buffer refreshed to theirs
        XCTAssertEqual(takeVM.snapshot.usernameCheck, .unchecked) // i13: value moved -> reset

        let keepVM = try await conflictedVM()
        keepVM.resolveKeepMine(.username)
        XCTAssertEqual(keepVM.usernameText, "mine1")
        XCTAssertEqual(keepVM.snapshot.usernameCheck, .passed) // value unchanged -> verdict stands
    }

    // ---- async check -------------------------------------------------------------------------

    func testCheckPassesAndFails() async throws {
        let vm = try makeVM()
        vm.focus(.username)
        vm.usernameText = "freshname"
        vm.editUsername()
        vm.runCheckNow()
        await eventually { vm.snapshot.usernameCheck == .passed }
        XCTAssertEqual(vm.snapshot.usernameCheck, .passed)

        // "admin" is in DefaultChecker's taken set -> failed with username_taken.
        vm.usernameText = "admin"
        vm.editUsername()
        XCTAssertEqual(vm.snapshot.usernameCheck, .unchecked) // reset on the value change
        vm.runCheckNow()
        await eventually {
            if case .failed = vm.snapshot.usernameCheck { return true }
            return false
        }
        guard case .failed(let error) = vm.snapshot.usernameCheck else {
            return XCTFail("expected .failed")
        }
        XCTAssertEqual(error.key, "username_taken")
        XCTAssertEqual(vm.inlineError(.username), "That username is already taken.")
    }

    /// A burst of edits collapses to a single check (debounce + single-flight).
    func testDebounceCollapsesBurst() async throws {
        let vm = try makeVM(debounce: .milliseconds(40))
        vm.focus(.username)
        for text in ["ab", "abc", "abcd", "abcde", "abcdef"] {
            vm.usernameText = text
            vm.editUsername()
        }
        await eventually { vm.checkRunCount == 1 }
        // give any stray timer a chance to (wrongly) fire, then confirm it did not.
        try? await Task.sleep(for: .milliseconds(80))
        XCTAssertEqual(vm.checkRunCount, 1)
    }

    // ---- submit ------------------------------------------------------------------------------

    /// Invalid field -> validation report; the draft stays alive (F3), so fixing it and resubmitting
    /// succeeds without a re-checkout.
    func testSubmitValidationThenRecovers() async throws {
        let vm = try makeVM()
        vm.focus(.email)
        vm.emailText = "not-an-email"
        vm.editEmail()
        vm.blur(.email)
        vm.submit()

        guard case .validation(let report) = vm.lastSubmit else {
            return XCTFail("expected validation outcome")
        }
        XCTAssertTrue(report.fieldErrors.contains { $0.field == .email })

        // F3: the draft survived; fix and resubmit.
        vm.focus(.email)
        vm.emailText = "alice@example.com"
        vm.editEmail()
        vm.blur(.email)
        vm.submit()
        XCTAssertEqual(vm.lastSubmit, .success)
    }

    /// A conflicted draft is refused with the conflicted fields.
    func testSubmitConflicted() async throws {
        let vm = try makeVM()
        vm.focus(.username)
        vm.usernameText = "mine1"
        vm.editUsername()
        vm.blur(.username)
        vm.applyServerChange(.username("theirs1"))
        await eventually { vm.snapshot.conflicts.contains(.username) }

        vm.submit()
        guard case .conflicted(let fields) = vm.lastSubmit else {
            return XCTFail("expected conflicted outcome")
        }
        XCTAssertTrue(fields.contains(.username))
    }

    /// Success: the canonical updates via the store stream and the editor re-checks-out clean.
    func testSubmitSuccessUpdatesCanonicalAndRechecksOut() async throws {
        let vm = try makeVM()
        vm.focus(.name)
        vm.nameText = "Alice Cooper"
        vm.editName()
        vm.blur(.name)
        vm.submit()

        XCTAssertEqual(vm.lastSubmit, .success)
        await eventually {
            if case .valid(let v) = vm.canonical?.name.validity { return v == "Alice Cooper" }
            return false
        }
        XCTAssertEqual(vm.nameText, "Alice Cooper") // fresh checkout reflects the committed value
        XCTAssertFalse(vm.isDirty(.name)) // and is clean
    }
}
