package dev.bolted.profileprobe

import com.example.spike_profile_ffi.PersonNameFieldSync
import com.example.spike_profile_ffi.PersonNameValidity
import com.example.spike_profile_ffi.ProfileFieldId
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.SubmitErrorFfi
import com.example.spike_profile_ffi.UsernameCheckFfi
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
 * design. Step 10 will generate both from the C-IDs.
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
            assertTrue(draft.snapshot().name.sync is PersonNameFieldSync.Conflicted)

            draft.trySetName("Server Name") // type their value

            val snap = draft.snapshot()
            assertTrue("editing to theirs must clear the conflict", snap.name.sync is PersonNameFieldSync.InSync)
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
            assertEquals(UsernameCheckFfi.Unchecked, draft.snapshot().usernameCheck)

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
            draft.setUniquenessChecker(uniqueChecker())
            assertTrue(draft.runUsernameCheck())
            assertEquals(UsernameCheckFfi.Passed, draft.snapshot().usernameCheck)
            draft.submit()
        }
        assertEquals("alice2", canonicalUsername())
    }

    /** ...and the other half: a clean username needs no check, or an email edit could never submit. */
    @Test
    fun c16_aCleanUsernameNeedsNoCheckToSubmit() {
        store.checkout().use { draft ->
            draft.trySetEmail("bob@example.com")
            assertEquals(UsernameCheckFfi.Unchecked, draft.snapshot().usernameCheck)
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
            assertTrue(validity is PersonNameValidity.Valid && validity.value == "My Name")

            // Resolve and resubmit on the SAME draft.
            draft.resolveKeepMine(ProfileFieldId.NAME)
            draft.submit()
            assertFalse("a successful submit tombstones the handle", draft.isLive())
        }
    }

    private fun canonicalUsername(): String {
        val validity = store.canonical()?.username?.validity
        return (validity as? com.example.spike_profile_ffi.UsernameValidity.Valid)?.value
            ?: error("canonical must be valid")
    }
}
