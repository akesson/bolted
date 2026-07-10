import XCTest

@testable import SpikeProfileFfi

/// The invariants the design freeze (step 06) added, exercised **through BoltFFI** rather than
/// against `bolted-core` directly. `docs/CONFORMANCE.md` is the normative statement of each; this
/// file is the Swift half of the evidence that they survive a codegen backend.
///
/// Step 10 will generate these from the C-IDs. Until then they are hand-written, like everything
/// else in the spike.
final class FreezeContractTests: XCTestCase {

    private func seededStore() throws -> ProfileStoreFfi {
        let store = ProfileStoreFfi()
        try store.applyCanonical(values: validValues())
        return store
    }

    // ---- C14: editing a conflicted field to `theirs` auto-converges ----------------------------

    /// Was step-01 friction F6. Typing their value now resolves the conflict, exactly as a
    /// convergent rebase does when the canonical change arrives second (C04).
    func testC14EditingToTheirsAutoConverges() throws {
        let store = try seededStore()
        let draft = store.checkout()

        try draft.trySetName(raw: "My Name")
        var theirs = validValues()
        theirs.name = "Server Name"
        try store.applyCanonical(values: theirs)

        guard case .conflicted = draft.snapshot().name.sync else {
            return XCTFail("a dirty field must conflict when canonical moves under it")
        }

        try draft.trySetName(raw: "Server Name") // type their value

        let snap = draft.snapshot()
        guard case .inSync = snap.name.sync else {
            return XCTFail("editing to theirs must clear the conflict")
        }
        XCTAssertFalse(snap.name.dirty)
        XCTAssertTrue(snap.conflicts.isEmpty)
    }

    // ---- C15: a rebase advances the draft's base version ----------------------------------------

    /// Before the freeze, a draft snapshot's `version` was written once at checkout and never again,
    /// so the version-guarded reconcile step 02 shipped for the subscribe race could never fire on a
    /// draft stream. It fires now.
    func testC15RebaseAdvancesTheDraftBaseVersion() throws {
        let store = try seededStore()
        let draft = store.checkout()
        let atCheckout = draft.snapshot().version

        var theirs = validValues()
        theirs.name = "Server Name"
        try store.applyCanonical(values: theirs)

        XCTAssertGreaterThan(
            draft.snapshot().version, atCheckout,
            "the stamp must track the canonical the draft is actually based on")
    }

    // ---- C16: an unrun check blocks a dirty field, and only a dirty field ------------------------

    /// Was step-01 friction F2, and the *default* path on two shells: an unchecked username sailed
    /// straight through submit. Now the core refuses, typed and pinned.
    func testC16UnrunCheckOnADirtyUsernameBlocksSubmit() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.trySetUsername(raw: "alice2")

        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .validation(let report)? = error as? SubmitErrorFfi else {
                return XCTFail("expected .validation, got \(error)")
            }
            XCTAssertTrue(report.ruleNames.contains("username_unique"))
            let violation = report.ruleErrors.first { $0.rule == "username_unique" }
            XCTAssertEqual(violation?.error.key, "username_check_required")
            XCTAssertEqual(violation?.pins, [.username])
        }

        // Run the check; a passing verdict unblocks the submit.
        draft.setUniquenessChecker(checker: StubChecker(.unique))
        XCTAssertTrue(draft.runUsernameCheck())
        XCTAssertEqual(draft.snapshot().usernameCheck, .passed)
        XCTAssertNoThrow(try draft.submit())
        XCTAssertEqual(store.canonical()?.username.validity, .valid(value: "alice2"))
    }

    /// The other half, and the reason C16 is not simply "unchecked blocks": a clean username still
    /// holds the canonical value, verified when it was committed. Editing only the email must not
    /// require a uniqueness lookup on a username nobody touched.
    func testC16CleanUsernameNeedsNoCheckToSubmit() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.trySetEmail(raw: "bob@example.com")

        XCTAssertEqual(draft.snapshot().usernameCheck, .unchecked)
        XCTAssertFalse(draft.snapshot().username.dirty)
        XCTAssertNoThrow(try draft.submit())
        XCTAssertEqual(store.canonical()?.email.validity, .valid(value: "bob@example.com"))
    }

    /// C05 + C16 together: reverting an edited username to the canonical value makes it clean, so
    /// the demand for a check goes away with it.
    func testC16RevertingTheUsernameWithdrawsTheDemandForACheck() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.trySetUsername(raw: "alice2")
        try draft.trySetUsername(raw: "alice") // back to canonical

        XCTAssertFalse(draft.snapshot().username.dirty)
        XCTAssertTrue(draft.validate().isOK)
        XCTAssertNoThrow(try draft.submit())
    }

    // ---- C17: a refused submit leaves the handle live; a successful one tombstones it ------------

    /// The FFI has tombstoned handles since step 02 (the foreign handle outlives the core draft).
    /// The freeze made the core API say so too. What is new here is the *refusal* half: the draft
    /// goes straight back, so a rejected submit never destroys an edit session (F3).
    func testC17RefusedSubmitLeavesTheDraftAliveAndEditable() throws {
        let store = try seededStore()
        let draft = store.checkout()

        try draft.trySetName(raw: "My Name")
        var theirs = validValues()
        theirs.name = "Server Name"
        try store.applyCanonical(values: theirs)

        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .conflicted(let fields)? = error as? SubmitErrorFfi else {
                return XCTFail("expected .conflicted, got \(error)")
            }
            XCTAssertEqual(fields, [.name])
        }

        XCTAssertTrue(draft.isLive(), "a refused submit must not consume the draft")
        XCTAssertEqual(draft.snapshot().name.validity, .valid(value: "My Name"), "my edit survived")

        // Resolve and resubmit on the SAME draft.
        draft.resolveKeepMine(field: .name)
        XCTAssertNoThrow(try draft.submit())
        XCTAssertFalse(draft.isLive())
        XCTAssertEqual(store.canonical()?.name.validity, .valid(value: "My Name"))
    }

    func testC17SecondSubmitIsAlreadySubmitted() throws {
        let store = try seededStore()
        let draft = store.checkout()
        XCTAssertNoThrow(try draft.submit())
        XCTAssertFalse(draft.isLive())

        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .alreadySubmitted? = error as? SubmitErrorFfi else {
                return XCTFail("expected .alreadySubmitted, got \(error)")
            }
        }
    }

    // ---- C19: rebase is a three-way merge --------------------------------------------------------

    /// Editing one field while the server changes a *different* one must not conflict mine. The
    /// store rebases the whole draft, so `name` is rebased onto its own ancestor.
    func testC19ADirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove() throws {
        let store = try seededStore()
        let draft = store.checkout()

        try draft.trySetName(raw: "My Name")
        var moved = validValues()
        moved.email = "team@corp.example" // the server touches email, and only email
        try store.applyCanonical(values: moved)

        let snap = draft.snapshot()
        XCTAssertTrue(snap.conflicts.isEmpty, "`name`'s canonical never moved")
        guard case .inSync = snap.name.sync else {
            return XCTFail("an unmoved canonical must not conflict a dirty field")
        }
        XCTAssertTrue(snap.name.dirty)
        XCTAssertEqual(snap.name.validity, .valid(value: "My Name"))
    }

    // ---- C20 / C21: the draft stash crosses the boundary and restores ---------------------------

    /// The stash DTO round-trips through BoltFFI, and `restore` rebases it onto whatever canonical
    /// says now: `email` moved while we were "dead" and comes back **conflicted**; `name` did not
    /// and comes back merely dirty.
    func testC21RestoreConflictsOnlyTheFieldsWhoseCanonicalMoved() throws {
        let store = try seededStore()
        let stash: ProfileStashFfi
        do {
            let draft = store.checkout()
            try draft.trySetName(raw: "My Name")
            try draft.trySetEmail(raw: "mine@other.com")
            stash = draft.stash()
            XCTAssertEqual(stash.name.raw, "My Name")
            XCTAssertEqual(stash.name.base, "Alice Smith") // the ancestor crosses too
        }

        // A new process: a new store, seeded from a server that moved `email`.
        let fresh = ProfileStoreFfi()
        var moved = validValues()
        moved.email = "server@corp.example"
        try fresh.applyCanonical(values: moved)

        let restored = fresh.restore(stash: stash)
        let snap = restored.snapshot()

        XCTAssertEqual(snap.conflicts, [.email])
        guard case .conflicted(_, let theirs) = snap.email.sync else {
            return XCTFail("email moved on the server; it must come back conflicted")
        }
        XCTAssertEqual(theirs, "server@corp.example", "a restored conflict names CURRENT canonical")
        XCTAssertEqual(snap.email.validity, .valid(value: "mine@other.com"))

        XCTAssertTrue(snap.name.dirty)
        guard case .inSync = snap.name.sync else {
            return XCTFail("`name` was untouched by the server; it must not conflict")
        }
        XCTAssertEqual(snap.name.validity, .valid(value: "My Name"))

        // The verdict did not survive (C20), so C16 refuses a dirty username until it is re-checked.
        XCTAssertEqual(snap.usernameCheck, .unchecked)
        XCTAssertEqual(snap.version, fresh.canonical()?.version)
    }

    /// The entity was deleted while the process was dead: the restored draft orphans (C11), it does
    /// not quietly commit and resurrect it.
    func testC21RestoreIntoADeletedCanonicalOrphansTheDraft() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.trySetName(raw: "My Name")
        let stash = draft.stash()

        let empty = ProfileStoreFfi() // no canonical: the server 404s
        let restored = empty.restore(stash: stash)

        XCTAssertEqual(restored.snapshot().status, .orphaned)
        XCTAssertEqual(empty.liveDraftCount(), 1) // present, but not rebasing — see below
        XCTAssertThrowsError(try restored.submit()) { error in
            guard case .orphaned? = error as? SubmitErrorFfi else {
                return XCTFail("expected .orphaned, got \(error)")
            }
        }
    }

    /// FINDING (step-07 friction 4): `liveDraftCount()` does **not** mean the same thing on the two
    /// sides of the boundary. `bolted_core::Store::live_draft_count` counts *drafts the store would
    /// rebase*; this wrapper counts *un-submitted drafts*. They agree everywhere C18 looks, and
    /// disagree on exactly the two drafts that are present-but-never-rebased: a restored orphan
    /// (above) and a create-flow draft (here). The Rust suite asserts 0 for both
    /// (`c21_restore_into_a_deleted_canonical_orphans_the_draft`,
    /// `c21_a_restored_create_flow_draft_is_never_moved`).
    ///
    /// Same name, two semantics, across a boundary step 10 will *generate*. Pin it to a C-ID.
    func testLiveDraftCountDisagreesWithTheCoreOnACreateFlowDraft() throws {
        let empty = ProfileStoreFfi() // no canonical: every checkout is create-flow
        let draft = empty.checkout()
        XCTAssertFalse(draft.snapshot().username.dirty)
        XCTAssertEqual(
            empty.liveDraftCount(), 1,
            "the wrapper counts un-submitted drafts; bolted_core::Store would say 0 here"
        )
    }
}
