import XCTest

@testable import GenProfileFfi

/// The invariants the design freeze (step 06) added, exercised **through BoltFFI** rather than
/// against `bolted-core` directly. `docs/CONFORMANCE.md` is the normative statement of each; this
/// file is the Swift half of the evidence that they survive a codegen backend.
///
/// Generating these from the C-IDs per language is a step-12 candidate; until then they are
/// hand-written, driving bindings that no longer are (step 11).
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
        draft.setUsernameChecker(checker: StubChecker(.pass))
        XCTAssertTrue(try draft.runUsernameCheck())
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
        try draft.resolveKeepMine(field: .name)
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

    // ---- D23: a mutating verb on a released handle refuses, typed --------------------------------

    /// The positive control for the migration's one real trap: `try?` at a call site would swallow
    /// this refusal and reproduce exactly the silent no-op D23 abolished. Verified to go red with
    /// the refusal swallowed (the swallow was planted, watched fail, and removed) — a control that
    /// has never failed is a needle that has never fired.
    func testD23MutatorOnASubmittedDraftThrowsDraftClosed() throws {
        let store = try seededStore()
        let draft = store.checkout()
        // A checker is installed here so this test exercises the corpse-WITH-checker cell; the
        // corpse-with-NO-checker cell is its own control below (before step 12 that cell answered
        // `false` instead of refusing — the no-checker short-circuit ran ahead of the liveness gate).
        draft.setUsernameChecker(checker: StubChecker(.pass))
        try draft.submit() // C17: the store releases the draft
        XCTAssertFalse(draft.isLive())

        XCTAssertThrowsError(try draft.resolveKeepMine(field: .username)) { error in
            XCTAssertEqual(error as? DraftClosedFfi, .draftClosed)
        }
        XCTAssertThrowsError(try draft.trySetName(raw: "Grace")) { error in
            XCTAssertEqual(error as? PersonNameErrorFfi, .draftClosed)
        }
        XCTAssertThrowsError(try draft.runUsernameCheck()) { error in
            XCTAssertEqual(error as? DraftClosedFfi, .draftClosed)
        }
    }

    /// The D23 no-checker control (step 11 friction 1, fixed step 12 M1). A released draft with **no
    /// checker installed** must still refuse `runUsernameCheck()` — before M1 it returned `false`,
    /// indistinguishable from "no checker on a live draft". Verified red against the unfixed
    /// generator (the liveness gate reverted, regenerated, watched fail, restored).
    func testD23RunCheckRefusesAReleasedDraftWithNoCheckerInstalled() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.submit() // released, and NO checker was ever set
        XCTAssertFalse(draft.isLive())

        XCTAssertThrowsError(try draft.runUsernameCheck()) { error in
            XCTAssertEqual(
                error as? DraftClosedFfi, .draftClosed,
                "a released draft refuses even with no checker (D23), not `false`")
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

        let restored = try fresh.restore(accepted: fresh.acceptStash(stash: stash))
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
        let restored = try empty.restore(accepted: empty.acceptStash(stash: stash))

        XCTAssertEqual(restored.snapshot().status, .orphaned)
        XCTAssertEqual(empty.liveDraftCount(), 1) // it exists...
        XCTAssertEqual(empty.rebasingDraftCount(), 0) // ...and it is not rebased (C22)
        XCTAssertThrowsError(try restored.submit()) { error in
            guard case .orphaned? = error as? SubmitErrorFfi else {
                return XCTFail("expected .orphaned, got \(error)")
            }
        }
    }

    /// C22 — and the closing of step-07 friction 4.
    ///
    /// This test used to be called `testLiveDraftCountDisagreesWithTheCoreOnACreateFlowDraft`, and
    /// it asserted a bug: `liveDraftCount()` meant *"drafts the store would rebase"* in
    /// `bolted_core::Store` and *"un-submitted drafts"* in this wrapper. The two agreed everywhere
    /// C18 looked and disagreed on exactly the drafts that are present-but-never-rebased — a
    /// create-flow draft, and a restored orphan. Two hand-written store loops, so there was no
    /// single answer to make right.
    ///
    /// Step 08 (D16) deleted one of the loops. The wrapper now asks the core, which answers two
    /// questions under two names, and this test asserts the contract instead of the defect.
    func testC22DraftCountAndRebasingDraftCountAreDifferentQuestions() throws {
        let empty = ProfileStoreFfi() // no canonical: every checkout is create-flow
        let draft = empty.checkout()
        XCTAssertFalse(draft.snapshot().username.dirty)

        XCTAssertEqual(empty.liveDraftCount(), 1, "a create-flow draft exists")
        XCTAssertEqual(empty.rebasingDraftCount(), 0, "and is never rebased (C12)")

        // an entity-backed checkout is both
        try empty.applyCanonical(values: validValues())
        let edit = empty.checkout()
        XCTAssertEqual(empty.liveDraftCount(), 2)
        XCTAssertEqual(empty.rebasingDraftCount(), 1)
        _ = edit
    }

    // ---- D27: the versioned stash envelope ------------------------------------------------------

    /// `acceptStash` gates the schema version carried in the DTO: a current-version stash is accepted
    /// into a token `restore` consumes; a stash from a schema this build does not accept throws
    /// `StashRefusedFfi`, typed, before any field is trusted. Apple never stashes in practice
    /// (nothing kills a process holding a draft the way Android does), but the gate is one generated
    /// surface, so the contract is exercised on both bindings.
    func testD27AcceptStashRefusesAStashFromAnUnknownSchema() throws {
        let store = try seededStore()
        let draft = store.checkout()
        try draft.trySetName(raw: "My Name")
        var stash = draft.stash()

        // Current version: accepted, and the token restores the edit session.
        let fresh = try seededStore()
        let restored = try fresh.restore(accepted: fresh.acceptStash(stash: stash))
        XCTAssertEqual(restored.snapshot().name.validity, .valid(value: "My Name"))

        // A schema version this build does not accept: refused, typed, both versions named.
        stash.schemaVersion &+= 1
        XCTAssertThrowsError(try fresh.acceptStash(stash: stash)) { error in
            guard case .schemaVersion(let stashed, let expected)? = error as? StashRefusedFfi else {
                return XCTFail("expected StashRefusedFfi.schemaVersion, got \(error)")
            }
            XCTAssertEqual(stashed, expected &+ 1, "the refusal names the stashed and expected versions")
        }
    }
}
