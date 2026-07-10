package dev.bolted.profileprobe

import com.example.spike_profile_ffi.ProfileDraftFfi
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.UniquenessChecker
import com.example.spike_profile_ffi.UniquenessVerdictFfi
import com.example.spike_profile_ffi.UsernameCheckFfi
import java.util.concurrent.atomic.AtomicReference
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Probe matrix E — callback traits (capabilities) on ART.
 *
 * Step 02's finding was that reentrancy is safe *because of the wrapper's discipline*: never hold
 * the `Mutex` across an outcall (`run_username_check` takes the checker out of its slot, drops every
 * lock, then calls it). If reentrancy deadlocks *here* despite that discipline, the cause is
 * JNI-side locking, and that is kill-bar territory for feature 4.
 */
class CallbackProbe {
    private lateinit var store: ProfileStoreFfi
    private lateinit var draft: ProfileDraftFfi

    @Before
    fun setUp() {
        store = seededStore()
        draft = store.checkout()
    }

    @After
    fun tearDown() {
        draft.close()
        store.close()
    }

    private fun checker(body: (String) -> UniquenessVerdictFfi) =
        object : UniquenessChecker {
            override fun checkUnique(username: String): UniquenessVerdictFfi = body(username)
        }

    /** No checker installed ⇒ the single-flight driver reports it did not run. */
    @Test
    fun withoutACheckerTheCheckDoesNotRun() {
        assertTrue(!draft.runUsernameCheck())
    }

    /** A Kotlin-implemented capability is invoked from Rust and its verdict lands in the core. */
    @Test
    fun aTakenVerdictBlocksAndAUniqueVerdictUnblocks() {
        val seen = AtomicReference<String>()
        draft.setUniquenessChecker(
            checker { username ->
                seen.set(username)
                if (username == "admin") UniquenessVerdictFfi.TAKEN else UniquenessVerdictFfi.UNIQUE
            }
        )

        draft.trySetUsername("admin")
        assertTrue(draft.runUsernameCheck())
        assertEquals("Rust passed the current text to the Kotlin checker", "admin", seen.get())

        val failed = draft.snapshot().usernameCheck
        assertTrue("expected Failed, got $failed", failed is UsernameCheckFfi.Failed)
        assertEquals("username_taken", (failed as UsernameCheckFfi.Failed).error.key)
        assertTrue(
            "a failed check must block validation",
            draft.validate().ruleErrors.any { it.rule == "username_unique" },
        )

        draft.trySetUsername("alice_two")
        assertTrue(draft.runUsernameCheck())
        assertTrue(draft.snapshot().usernameCheck is UsernameCheckFfi.Passed)
        assertTrue(draft.validate().ruleErrors.none { it.rule == "username_unique" })
    }

    /**
     * C13, value-bound verdict reset: typing changes the checked value, so the core must discard the
     * old verdict rather than show a `Passed`/`Failed` belonging to text that is gone.
     */
    @Test
    fun editingAfterAVerdictResetsTheCheck() {
        draft.setUniquenessChecker(checker { UniquenessVerdictFfi.TAKEN })
        draft.trySetUsername("admin")
        draft.runUsernameCheck()
        assertTrue(draft.snapshot().usernameCheck is UsernameCheckFfi.Failed)

        draft.trySetUsername("admin2")
        assertTrue(
            "the verdict belonged to 'admin'; it must not survive the edit",
            draft.snapshot().usernameCheck is UsernameCheckFfi.Unchecked,
        )
    }

    /**
     * **The deadlock probe.** The Kotlin checker, while Rust is inside `run_username_check`,
     * synchronously re-enters the *same* exported object with a read (`validate`) and a mutation
     * (`trySetName`). If this hangs, the suite times out and that is the finding.
     */
    @Test(timeout = 20_000)
    fun aReentrantCheckerDoesNotDeadlock() {
        val reentered = AtomicReference(false)
        draft.setUniquenessChecker(
            checker {
                draft.validate() // read, re-entering the same Rust object
                draft.trySetName("Reentrant Name") // mutation, ditto
                reentered.set(true)
                UniquenessVerdictFfi.UNIQUE
            }
        )

        draft.trySetUsername("alice_two")
        assertTrue(draft.runUsernameCheck())
        assertTrue("the checker never ran", reentered.get())

        val name = draft.snapshot().name.validity
        record("callback.reentrant_mutation_applied", name.toString())
        assertTrue(
            "the reentrant mutation must take effect, not be silently dropped",
            name.toString().contains("Reentrant Name"),
        )
    }
}
