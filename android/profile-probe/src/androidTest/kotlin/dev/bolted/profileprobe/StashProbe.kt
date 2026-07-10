package dev.bolted.profileprobe

import com.example.spike_profile_ffi.DraftStatusFfi
import com.example.spike_profile_ffi.EmailErrorFfi
import com.example.spike_profile_ffi.EmailFieldSync
import com.example.spike_profile_ffi.EmailValidity
import com.example.spike_profile_ffi.PersonNameFieldSync
import com.example.spike_profile_ffi.PersonNameValidity
import com.example.spike_profile_ffi.ProfileFieldId
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.SubmitErrorFfi
import com.example.spike_profile_ffi.UsernameCheckFfi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * C20 / C21 through JNI: the draft stash crosses the boundary, and `restore` rebases it onto whatever
 * canonical says now.
 *
 * Android is the platform this mechanism exists for — it is the one that kills processes holding live
 * drafts — so the wire encoding gets exercised here before the Compose app depends on it. Note the
 * nested optionals: `ProfileStashFfi` holds `TextFieldStashFfi(raw: String?, base: String?)` and a
 * `DateRangeFieldStashFfi` whose optionals wrap a *record*. That is the first time an
 * `Option<record>` has crossed on ART.
 *
 * The app-level tier (process death through a real `Bundle` and `Parcel`) lives in `android/profile-app`.
 */
class StashProbe {

    /** The whole point of stashing the ancestor: only the fields the server moved come back conflicted. */
    @Test
    fun c21_restoreConflictsOnlyTheFieldsWhoseCanonicalMoved() {
        val stash =
            seededStore().use { store ->
                store.checkout().use { draft ->
                    draft.trySetName("My Name")
                    draft.trySetEmail("mine@other.com")
                    draft.stash()
                }
            }

        assertEquals("My Name", stash.name.raw)
        assertEquals("Alice Anderson", stash.name.base) // the ancestor crosses too
        // Option<record>: the composite value object's stash, both halves present after a checkout.
        assertEquals(SEED.availability, stash.availability.raw)
        assertEquals(SEED.availability, stash.availability.base)

        // A new process: a new store, seeded from a server that moved `email` while we were dead.
        ProfileStoreFfi.new().use { fresh ->
            fresh.applyCanonical(SEED.copy(email = "server@corp.example"))
            fresh.restore(stash).use { restored ->
                val snap = restored.snapshot()

                assertEquals(listOf(ProfileFieldId.EMAIL), snap.conflicts)
                val sync = snap.email.sync
                assertTrue(sync is EmailFieldSync.Conflicted)
                assertEquals(
                    "a restored conflict must name the CURRENT canonical",
                    "server@corp.example",
                    (sync as EmailFieldSync.Conflicted).theirs,
                )
                val mine = snap.email.validity
                assertTrue(mine is EmailValidity.Valid && mine.value == "mine@other.com")

                // `name` was untouched by the server: dirty, not conflicted (C19 doing the work).
                assertTrue(snap.name.dirty)
                assertTrue(snap.name.sync is PersonNameFieldSync.InSync)
                val restoredName = snap.name.validity
                assertTrue(restoredName is PersonNameValidity.Valid && restoredName.value == "My Name")

                // The verdict did not survive (C20).
                assertEquals(UsernameCheckFfi.Unchecked, snap.usernameCheck)
                record("c21.restored_conflicts", snap.conflicts.toString())
            }
        }
    }

    /** An `Invalid { raw }` survives process death: C06 does not stop being true because we died. */
    @Test
    fun c20_anInvalidAttemptSurvivesTheStash() {
        seededStore().use { store ->
            val stash =
                store.checkout().use { draft ->
                    var rejected = false
                    try {
                        draft.trySetEmail("not-an-email")
                    } catch (_: EmailErrorFfi) {
                        rejected = true // and the core recorded Invalid { raw }, as a keystroke would
                    }
                    assertTrue("no @: the core must reject it", rejected)
                    draft.stash()
                }

            assertEquals("not-an-email", stash.email.raw)
            store.restore(stash).use { restored ->
                val validity = restored.snapshot().email.validity
                assertTrue("the user's rejected text is still theirs", validity is EmailValidity.Invalid)
                assertEquals("not-an-email", (validity as EmailValidity.Invalid).raw)
            }
        }
    }

    /** The entity was deleted while we were dead: the restored draft orphans, it does not resurrect it. */
    @Test
    fun c21_restoreIntoADeletedCanonicalOrphansTheDraft() {
        val stash =
            seededStore().use { store ->
                store.checkout().use { draft ->
                    draft.trySetName("My Name")
                    draft.stash()
                }
            }

        ProfileStoreFfi.new().use { empty -> // no canonical: the server 404s
            empty.restore(stash).use { restored ->
                assertEquals(DraftStatusFfi.ORPHANED, restored.snapshot().status)
                try {
                    restored.submit()
                    fail("expected SubmitErrorFfi.Orphaned")
                } catch (_: SubmitErrorFfi.Orphaned) {
                    record("c21.orphaned_on_restore", "true")
                }
            }
        }
    }
}
