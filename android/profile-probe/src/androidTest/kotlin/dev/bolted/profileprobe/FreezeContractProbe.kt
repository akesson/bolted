package dev.bolted.profileprobe

import com.example.gen_profile_ffi.TextFieldSync
import com.example.gen_profile_ffi.TextValidity
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.SubmitErrorFfi
import com.example.gen_profile_ffi.CheckStateFfi
import com.example.gen_profile_ffi.DraftClosedFfi
import com.example.gen_profile_ffi.PersonNameErrorFfi
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

/**
 * The invariants the design freeze (step 06) added — C14, C15, C16, C17 — exercised on ART through
 * the Kotlin bindings. `docs/CONFORMANCE.md` states each normatively.
 *
 * The Swift probe (`FreezeContractTests.swift`) asserts the same things. Running both is the point:
 * a contract that only holds on one codegen backend is a property of that generator, not of the
 * design. Generating both from the C-IDs per language is a step-12 candidate; step 11 repointed
 * this file at bindings nobody wrote by hand.
 */
class FreezeContractProbe {
    private lateinit var store: ProfileStoreFfi

    @Before
    fun setUp() {
        store = seededStore()
    }

    @After
    fun tearDown() {
        store.close()
    }

    /** C14 (was F6): editing a conflicted field to `theirs` resolves the conflict. */
    @Test
    fun c14_editingAConflictedFieldToTheirsAutoConverges() {
        store.checkout().use { draft ->
            draft.trySetName("My Name")
            store.applyCanonical(SEED.copy(name = "Server Name"))
            assertTrue(draft.snapshot().name.sync is TextFieldSync.Conflicted)

            draft.trySetName("Server Name") // type their value

            val snap = draft.snapshot()
            assertTrue("editing to theirs must clear the conflict", snap.name.sync is TextFieldSync.InSync)
            assertFalse(snap.name.dirty)
            assertTrue(snap.conflicts.isEmpty())
            record("c14.auto_converged", "true")
        }
    }

    /**
     * C15: the draft's base version tracks the canonical it is actually based on.
     *
     * Step 05 found this stamp frozen at checkout — which meant the version-guarded reconcile that
     * step 02 shipped for the future-only subscribe race could never fire on a draft stream. This
     * probe is the direct rebuttal of that finding.
     */
    @Test
    fun c15_rebaseAdvancesTheDraftBaseVersion() {
        store.checkout().use { draft ->
            val atCheckout = draft.snapshot().version
            store.applyCanonical(SEED.copy(name = "Server Name"))
            val afterRebase = draft.snapshot().version

            record("c15.version", "checkout=$atCheckout after_rebase=$afterRebase")
            assertTrue(
                "the stamp must advance: was $atCheckout, still $afterRebase",
                afterRebase > atCheckout,
            )
        }
    }

    /** C16 (was F2): a dirty username whose check never ran cannot submit. */
    @Test
    fun c16_anUnrunCheckOnADirtyUsernameBlocksSubmit() {
        store.checkout().use { draft ->
            draft.trySetUsername("alice2")
            assertEquals(CheckStateFfi.Unchecked, draft.snapshot().usernameCheck)

            try {
                draft.submit()
                fail("expected SubmitErrorFfi.Validation — an unchecked dirty username must not commit")
            } catch (e: SubmitErrorFfi.Validation) {
                val violation = e.report.ruleErrors.single { it.rule == "username_unique" }
                assertEquals("username_check_required", violation.error.key)
                assertEquals(listOf(ProfileFieldId.USERNAME), violation.pins)
                record("c16.blocked_key", violation.error.key)
            }

            // A passing verdict unblocks it.
            draft.setUsernameChecker(uniqueChecker())
            assertTrue(draft.runUsernameCheck())
            assertEquals(CheckStateFfi.Passed, draft.snapshot().usernameCheck)
            draft.submit()
        }
        assertEquals("alice2", canonicalUsername())
    }

    /** ...and the other half: a clean username needs no check, or an email edit could never submit. */
    @Test
    fun c16_aCleanUsernameNeedsNoCheckToSubmit() {
        store.checkout().use { draft ->
            draft.trySetEmail("bob@example.com")
            assertEquals(CheckStateFfi.Unchecked, draft.snapshot().usernameCheck)
            assertFalse(draft.snapshot().username.dirty)
            draft.submit() // must not throw
        }
        assertEquals("alice", canonicalUsername())
    }

    /**
     * C17: a refused submit hands the draft straight back. Before the freeze the FFI wrapper
     * pre-checked and returned early; now `commit` owns the gates and returns the draft on failure,
     * and the foreign handle stays live.
     */
    @Test
    fun c17_aRefusedSubmitLeavesTheDraftAliveAndEditable() {
        store.checkout().use { draft ->
            draft.trySetName("My Name")
            store.applyCanonical(SEED.copy(name = "Server Name"))

            try {
                draft.submit()
                fail("expected SubmitErrorFfi.Conflicted")
            } catch (e: SubmitErrorFfi.Conflicted) {
                assertEquals(listOf(ProfileFieldId.NAME), e.fields)
            }

            assertTrue("a refused submit must not consume the draft", draft.isLive())
            val validity = draft.snapshot().name.validity
            assertTrue(validity is TextValidity.Valid && validity.value == "My Name")

            // Resolve and resubmit on the SAME draft.
            draft.resolveKeepMine(ProfileFieldId.NAME)
            draft.submit()
            assertFalse("a successful submit tombstones the handle", draft.isLive())
        }
    }

    /**
     * D23: a mutating verb on a released handle refuses with a typed error. The positive control for
     * the migration's one real trap — `runCatching {}` at a call site would swallow this refusal and
     * reproduce exactly the silent no-op D23 abolished. Verified to go red with the refusal
     * swallowed (the swallow was planted, watched fail, and removed).
     */
    @Test
    fun d23_aMutatorOnASubmittedDraftThrowsDraftClosed() {
        store.checkout().use { draft ->
            // With NO checker set, `runUsernameCheck` short-circuits to `false` before it ever looks
            // at the draft — the refusal below is only reachable with a checker installed.
            draft.setUsernameChecker(uniqueChecker())
            draft.submit() // C17: the store releases the draft
            assertFalse(draft.isLive())

            try {
                draft.resolveKeepMine(ProfileFieldId.USERNAME)
                fail("expected DraftClosedFfi — the refusal must reach the shell, typed")
            } catch (_: DraftClosedFfi) {
                record("d23.resolve_refused", "typed")
            }
            try {
                draft.trySetName("Grace")
                fail("expected PersonNameErrorFfi.DraftClosed")
            } catch (e: PersonNameErrorFfi) {
                assertTrue("got $e", e is PersonNameErrorFfi.DraftClosed)
            }
            try {
                draft.runUsernameCheck()
                fail("expected DraftClosedFfi from runUsernameCheck with a checker installed")
            } catch (_: DraftClosedFfi) {
                record("d23.check_refused", "typed")
            }
        }
    }

    /**
     * C19: rebase is a three-way merge. The store rebases every field of a draft on every canonical
     * change, so a field the server never touched is rebased onto its own ancestor. That must not
     * conflict it — the "take theirs" button would hold the user's own base value, and `submit`
     * would be refused with nothing to resolve.
     *
     * Found while planning step 07; it had been latent in every shell since step 01.
     */
    @Test
    fun c19_aDirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove() {
        store.checkout().use { draft ->
            draft.trySetName("My Name")
            store.applyCanonical(SEED.copy(email = "team@corp.example")) // email, and only email

            val snap = draft.snapshot()
            assertTrue("`name`'s canonical never moved", snap.conflicts.isEmpty())
            assertTrue(snap.name.sync is TextFieldSync.InSync)
            assertTrue("my edit survives", snap.name.dirty)
            val validity = snap.name.validity
            assertTrue(validity is TextValidity.Valid && validity.value == "My Name")
            record("c19.spurious_conflict", "absent")
        }
    }

    /**
     * C22: "a draft exists" and "a draft rebases" are different questions, and the store answers
     * both, separately.
     *
     * Until step 08 there were two hand-written store loops, and each had a `live_draft_count()`.
     * The core's meant "would be rebased"; this wrapper's meant "exists". They disagreed by one on
     * every create-flow draft and every orphan, for five steps, and no test could compare them
     * because they lived in different crates. D16 deleted one loop; the wrapper now asks the core.
     */
    @Test
    fun c22_draftCountAndRebasingDraftCountAreDifferentQuestions() {
        ProfileStoreFfi().use { empty ->
            empty.checkout().use { _ ->
                assertEquals("a create-flow draft exists", 1u, empty.liveDraftCount())
                assertEquals("and is never rebased (C12)", 0u, empty.rebasingDraftCount())

                empty.applyCanonical(SEED)
                empty.checkout().use { _ ->
                    assertEquals("an entity-backed checkout is both", 2u, empty.liveDraftCount())
                    assertEquals(1u, empty.rebasingDraftCount())
                }
                // close() removed it from both — and on ART, close() is the only thing that would
                assertEquals(1u, empty.liveDraftCount())
                assertEquals(0u, empty.rebasingDraftCount())
            }
            assertEquals(0u, empty.liveDraftCount())
        }
    }

    private fun canonicalUsername(): String {
        val validity = store.canonical()?.username?.validity
        return (validity as? com.example.gen_profile_ffi.TextValidity.Valid)?.value
            ?: error("canonical must be valid")
    }
}
