package dev.bolted.genprofilesmoke

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.example.gen_profile_ffi.AvailabilityRaw
import com.example.gen_profile_ffi.CheckStateFfi
import com.example.gen_profile_ffi.CheckVerdictFfi
import com.example.gen_profile_ffi.DraftClosedFfi
import com.example.gen_profile_ffi.PersonNameErrorFfi
import com.example.gen_profile_ffi.PlainDate
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.ProfileValues
import com.example.gen_profile_ffi.TextFieldSync
import com.example.gen_profile_ffi.TextValidity
import com.example.gen_profile_ffi.UsernameChecker
import com.example.gen_profile_ffi.UsernameErrorFfi
import com.example.gen_profile_ffi.ping
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Step 11, M0: everything here crosses a real JNI boundary on ART into Rust that nobody wrote by
 * hand — the Kotlin twin of `apple/gen-profile-smoke`. The first test to touch the bindings also
 * proves `System.loadLibrary("gen_profile_ffi")` resolved every symbol (step 05's failure mode was
 * an undefined-symbol dlopen abort right there).
 */
@RunWith(AndroidJUnit4::class)
class GeneratedBindingsSmoke {

    private fun seed(store: ProfileStoreFfi, username: String = "ada") {
        store.applyCanonical(
            ProfileValues(
                username = username,
                name = "Ada",
                email = "ada@corp.example",
                availability = AvailabilityRaw(
                    start = PlainDate(2026u, 1u, 1u),
                    end = PlainDate(2026u, 12u, 31u),
                ),
            )
        )
    }

    @Test
    fun theSoLoadsAndTheSkeletonPings() {
        assertEquals("pong: hi", ping("hi"))
    }

    @Test
    fun aDraftChecksOutAndSnapshots() {
        val store = ProfileStoreFfi()
        seed(store)
        val draft = store.checkout()
        val snapshot = draft.snapshot()

        for (state in listOf(snapshot.username, snapshot.name, snapshot.email)) {
            assertFalse(state.dirty)
            assertEquals(TextFieldSync.InSync, state.sync)
        }
        assertEquals(TextValidity.Valid("ada"), snapshot.username.validity)
    }

    @Test
    fun aKeystrokeValidatesWithATypedError() {
        val store = ProfileStoreFfi()
        seed(store)
        val draft = store.checkout()

        val error = assertThrows(UsernameErrorFfi::class.java) { draft.trySetUsername("ab") }
        assertEquals(UsernameErrorFfi.TooShort(min = 3u, actual = 2u), error)
    }

    /** D23: a mutating verb on a released handle refuses with a typed error, on ART too. */
    @Test
    fun aMutatorRefusesASubmittedDraft() {
        val store = ProfileStoreFfi()
        seed(store)
        val draft = store.checkout()
        draft.submit()
        assertFalse(draft.isLive())

        assertThrows(PersonNameErrorFfi::class.java) { draft.trySetName("Grace") }
        assertThrows(DraftClosedFfi::class.java) { draft.resolveKeepMine(ProfileFieldId.USERNAME) }
    }

    /** The generated capability trait, implemented in Kotlin, called from Rust with no lock held. */
    @Test
    fun theGeneratedCheckerCapabilityRoundTrips() {
        val store = ProfileStoreFfi()
        seed(store)
        val draft = store.checkout()
        draft.trySetUsername("  grace  ")

        val asked = mutableListOf<String>()
        draft.setUsernameChecker(object : UsernameChecker {
            override fun check(value: String): CheckVerdictFfi {
                asked.add(value)
                return CheckVerdictFfi.FAIL
            }
        })
        assertTrue(draft.runUsernameCheck())

        assertEquals("the sanitizer runs before the checker is asked", listOf("grace"), asked)
        val check = draft.snapshot().usernameCheck
        assertTrue("expected a failed verdict, got $check", check is CheckStateFfi.Failed)
        // `failed_key` comes from the declaration — no shell names a localisation key.
        assertEquals("username_taken", (check as CheckStateFfi.Failed).error.key)
    }

    /** `close()` releases the store-side handle: the live count drops, C1-style. */
    @Test
    fun closeReleasesTheDraft() {
        val store = ProfileStoreFfi()
        seed(store)
        val draft = store.checkout()
        assertEquals(1u, store.liveDraftCount())

        draft.close()
        assertEquals(0u, store.liveDraftCount())
    }
}
