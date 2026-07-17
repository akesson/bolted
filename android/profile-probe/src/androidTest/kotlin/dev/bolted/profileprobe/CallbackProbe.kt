package dev.bolted.profileprobe

import com.example.gen_profile_ffi.ProfileDraftFfi
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.UsernameChecker
import com.example.gen_profile_ffi.CheckVerdictFfi
import com.example.gen_profile_ffi.CheckStateFfi
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

    // D34: the capability is a checkout argument, so each test checks out its own draft with the
    // checker it probes; setUp only seeds the store, tearDown closes whatever the test checked out.
    @Before
    fun setUp() {
        store = seededStore()
    }

    @After
    fun tearDown() {
        draft.close()
        store.close()
    }

    private fun checker(body: (String) -> CheckVerdictFfi) =
        object : UsernameChecker {
            override fun check(value: String): CheckVerdictFfi = body(value)
        }

    /** A declared-absent capability (`null` at checkout, D34) ⇒ the driver reports it did not run. */
    @Test
    fun withoutACheckerTheCheckDoesNotRun() {
        draft = store.checkout(null)
        assertTrue(!draft.runUsernameCheck())
    }

    /** A Kotlin-implemented capability is invoked from Rust and its verdict lands in the core. */
    @Test
    fun aTakenVerdictBlocksAndAUniqueVerdictUnblocks() {
        val seen = AtomicReference<String>()
        draft = store.checkout(
            checker { username ->
                seen.set(username)
                if (username == "admin") CheckVerdictFfi.FAIL else CheckVerdictFfi.PASS
            }
        )

        draft.trySetUsername("admin")
        assertTrue(draft.runUsernameCheck())
        assertEquals("Rust passed the current text to the Kotlin checker", "admin", seen.get())

        val failed = draft.snapshot().usernameCheck
        assertTrue("expected Failed, got $failed", failed is CheckStateFfi.Failed)
        assertEquals("username_taken", (failed as CheckStateFfi.Failed).error.key)
        assertTrue(
            "a failed check must block validation",
            draft.validate().ruleErrors.any { it.rule == "username_unique" },
        )

        draft.trySetUsername("alice_two")
        assertTrue(draft.runUsernameCheck())
        assertTrue(draft.snapshot().usernameCheck is CheckStateFfi.Passed)
        assertTrue(draft.validate().ruleErrors.none { it.rule == "username_unique" })
    }

    /**
     * C13, value-bound verdict reset: typing changes the checked value, so the core must discard the
     * old verdict rather than show a `Passed`/`Failed` belonging to text that is gone.
     */
    @Test
    fun editingAfterAVerdictResetsTheCheck() {
        draft = store.checkout(checker { CheckVerdictFfi.FAIL })
        draft.trySetUsername("admin")
        draft.runUsernameCheck()
        assertTrue(draft.snapshot().usernameCheck is CheckStateFfi.Failed)

        draft.trySetUsername("admin2")
        assertTrue(
            "the verdict belonged to 'admin'; it must not survive the edit",
            draft.snapshot().usernameCheck is CheckStateFfi.Unchecked,
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
        // The closure captures the class's `draft` property; by the time Rust calls the checker,
        // checkout has returned and the property names the same draft the check runs on.
        draft = store.checkout(
            checker {
                draft.validate() // read, re-entering the same Rust object
                draft.trySetName("Reentrant Name") // mutation, ditto
                reentered.set(true)
                CheckVerdictFfi.PASS
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
