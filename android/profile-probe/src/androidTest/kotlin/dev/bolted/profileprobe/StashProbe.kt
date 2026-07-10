package dev.bolted.profileprobe

import com.example.gen_profile_ffi.DraftStatusFfi
import com.example.gen_profile_ffi.EmailErrorFfi
import com.example.gen_profile_ffi.TextFieldSync
import com.example.gen_profile_ffi.TextValidity
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.ProfileStoreFfi
import com.example.gen_profile_ffi.StashRefusedFfi
import com.example.gen_profile_ffi.SubmitErrorFfi
import com.example.gen_profile_ffi.CheckStateFfi
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
 * `AvailabilityStash` whose optionals wrap a *record*. That is the first time an
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
            fresh.restore(fresh.acceptStash(stash)).use { restored ->
                val snap = restored.snapshot()

                assertEquals(listOf(ProfileFieldId.EMAIL), snap.conflicts)
                val sync = snap.email.sync
                assertTrue(sync is TextFieldSync.Conflicted)
                assertEquals(
                    "a restored conflict must name the CURRENT canonical",
                    "server@corp.example",
                    (sync as TextFieldSync.Conflicted).theirs,
                )
                val mine = snap.email.validity
                assertTrue(mine is TextValidity.Valid && mine.value == "mine@other.com")

                // `name` was untouched by the server: dirty, not conflicted (C19 doing the work).
                assertTrue(snap.name.dirty)
                assertTrue(snap.name.sync is TextFieldSync.InSync)
                val restoredName = snap.name.validity
                assertTrue(restoredName is TextValidity.Valid && restoredName.value == "My Name")

                // The verdict did not survive (C20).
                assertEquals(CheckStateFfi.Unchecked, snap.usernameCheck)
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
            store.restore(store.acceptStash(stash)).use { restored ->
                val validity = restored.snapshot().email.validity
                assertTrue("the user's rejected text is still theirs", validity is TextValidity.Invalid)
                assertEquals("not-an-email", (validity as TextValidity.Invalid).raw)
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
            empty.restore(empty.acceptStash(stash)).use { restored ->
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

    /**
     * D27 — the versioned stash envelope, at the JNI boundary. `acceptStash` gates the schema version
     * carried in the DTO: a current-version stash is accepted into a token `restore` consumes; a
     * stash from a schema this build does not accept throws `StashRefusedFfi`, typed, before any field
     * is trusted. The typed throw crossing JNI is the point — a `null` would be indistinguishable
     * from "no stash".
     */
    @Test
    fun d27_acceptStashRefusesAStashFromAnUnknownSchema() {
        val stash =
            seededStore().use { store ->
                store.checkout().use { draft ->
                    draft.trySetName("My Name")
                    draft.stash()
                }
            }

        seededStore().use { fresh ->
            // Current version: accepted, and restores the edit session.
            fresh.restore(fresh.acceptStash(stash)).use { restored ->
                val v = restored.snapshot().name.validity
                assertTrue("the rescued edit survives", v is TextValidity.Valid && v.value == "My Name")
            }

            // A schema version this build does not accept: refused, typed, both versions named.
            val stale = stash.copy(schemaVersion = stash.schemaVersion + 1u)
            try {
                fresh.acceptStash(stale)
                fail("expected StashRefusedFfi.SchemaVersion — a refused stash is not `null`")
            } catch (e: StashRefusedFfi.SchemaVersion) {
                assertEquals(stash.schemaVersion + 1u, e.stashed)
                record("d27.stash_refused", "typed")
            }
        }
    }
}
