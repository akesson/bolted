package dev.bolted.profileapp

import androidx.lifecycle.SavedStateHandle
import com.example.gen_profile_ffi.TextValidity
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.CheckStateFfi
import dev.bolted.profileapp.generated.ProfileStashCodec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * **Process death, mid-draft** — the risk step 07 exists for, and ARCHITECTURE §9's last undesigned
 * Phase-2 mechanism (C20/C21).
 *
 * The simulation is as faithful as a headless test can be. `SavedStateHandle.savedStateProvider()
 * .saveState()` produces the exact `Bundle` the framework persists; [parcelRoundTrip] pushes it
 * through a real `Parcel`, the exact serialization the framework performs; the old `ViewModel` and
 * its `ProfileStoreFfi` are then destroyed, and a new VM is built from the revived handle. The one
 * thing it cannot prove is that Android *chose* to kill us.
 */
class StashRestoreTest {

    /** A fresh handle: no stash, so the VM checks out rather than restores. */
    @Test
    fun withNoStashTheViewModelChecksOutAFreshDraft() {
        val host = VmHost()
        val vm = host.create()
        assertFalse(vm.restoredFromStash)
        assertEquals("alice", onMain { vm.buffers.value }.username)
        host.clear()
    }

    /**
     * The headline. Two dirty fields go into the stash; while we are dead the server moves *one* of
     * them. The restored draft conflicts that one, and leaves the other alone — because the stash
     * carries the ancestor, and C19 keeps an unmoved canonical from raising a conflict.
     */
    @Test
    fun c21_restoreConflictsOnlyTheFieldsWhoseCanonicalMoved() {
        val handle = SavedStateHandle()
        val first = VmHost()
        val vm1 = first.create(handle)
        onMain {
            vm1.editName("My Name")
            vm1.editEmail("mine@other.com")
        }

        // Android saves, then kills us.
        val revived = handle.persistAndRevive()
        first.clear()

        // While we were dead the server moved `email`, and only `email`.
        revived[ProfileViewModel.SERVER_KEY] = ProfileStashCodec.encodeValues(SEED.copy(email = "server@corp.example"))

        val second = VmHost()
        val vm2 = second.create(revived)
        assertTrue("the edit session came back", vm2.restoredFromStash)

        val conflict = onMain { vm2.conflict(ProfileFieldId.EMAIL) }
        assertNotNull("email moved on the server while we were dead", conflict)
        assertEquals(
            "a restored conflict must name the CURRENT canonical, not the one we died holding",
            "server@corp.example",
            conflict!!.theirs,
        )
        assertEquals("mine@other.com", onMain { vm2.buffers.value }.email)

        // `name` was untouched by the server: dirty, not conflicted.
        assertNull(onMain { vm2.conflict(ProfileFieldId.NAME) })
        assertTrue(onMain { vm2.isDirty(ProfileFieldId.NAME) })
        assertEquals("My Name", onMain { vm2.buffers.value }.name)

        record("c21.restored_conflicts", onMain { vm2.snapshot.value }.conflicts.toString())
        second.clear()
    }

    /** C06 does not stop being true because the process died. */
    @Test
    fun c20_anInvalidAttemptSurvivesProcessDeath() {
        val handle = SavedStateHandle()
        val first = VmHost()
        val vm1 = first.create(handle)
        onMain { vm1.editEmail("not-an-email") }
        assertNotNull(onMain { vm1.inlineError(ProfileFieldId.EMAIL) })

        val revived = handle.persistAndRevive()
        first.clear()

        val second = VmHost()
        val vm2 = second.create(revived)
        assertEquals("the user's rejected text is still theirs", "not-an-email", onMain { vm2.buffers.value }.email)
        assertTrue(onMain { vm2.snapshot.value }.email.validity is TextValidity.Invalid)
        assertEquals(
            "That is not a valid email address.",
            onMain { vm2.inlineError(ProfileFieldId.EMAIL) },
        )
        second.clear()
    }

    /**
     * C20: an async verdict does NOT survive, because it endorses a value against a server state
     * that may have moved. C16 then demands a fresh check — and the shell renders that as progress,
     * not as an error the user caused (`Localization.isProgress`).
     */
    @Test
    fun c20_anAsyncVerdictDoesNotSurviveAndC16DemandsAFreshOne() {
        val handle = SavedStateHandle()
        val first = VmHost(ProfileViewModel.Timing(debounceMs = 10))
        val vm1 = first.create(handle)
        onMain { vm1.editUsername("alice2") }
        awaitUntil(what = "the verdict") { vm1.snapshot.value.usernameCheck is CheckStateFfi.Passed }

        val revived = handle.persistAndRevive()
        first.clear()

        val second = VmHost(ProfileViewModel.Timing(debounceMs = 10_000)) // no check fires
        val vm2 = second.create(revived)

        assertEquals("alice2", onMain { vm2.buffers.value }.username)
        assertTrue(onMain { vm2.isDirty(ProfileFieldId.USERNAME) })
        assertTrue(
            "a verdict from before the death endorses nothing",
            onMain { vm2.snapshot.value }.usernameCheck is CheckStateFfi.Unchecked,
        )
        assertNull("and it is NOT an error", onMain { vm2.inlineError(ProfileFieldId.USERNAME) })
        assertEquals(
            "Checking that this username is free…",
            onMain { vm2.progressHint(ProfileFieldId.USERNAME) },
        )

        onMain { vm2.submit() }
        assertTrue("C16 blocks it", onMain { vm2.lastSubmit.value } is SubmitOutcome.Validation)
        second.clear()
    }

    /**
     * C21: a resolution taken before the death survives it, because its effect lives in the
     * *ancestor* and the ancestor is stashed. `resolve_keep_mine` set `base := "Their Name"`; the
     * server still says that, so C19's early-out leaves the restored field dirty and `InSync`. The
     * user is not asked to decide twice.
     */
    @Test
    fun c21_aResolutionSurvivesTheRestore() {
        val handle = SavedStateHandle()
        val first = VmHost()
        val vm1 = first.create(handle)
        onMain {
            vm1.editName("My Name")
            vm1.applyServerChange(ProfileViewModel.ServerChange.Name("Their Name"))
        }
        awaitUntil(what = "the conflict") { vm1.conflict(ProfileFieldId.NAME) != null }
        onMain { vm1.resolveKeepMine(ProfileFieldId.NAME) } // base := "Their Name", value stays mine

        val revived = handle.persistAndRevive()
        first.clear()

        val second = VmHost()
        val vm2 = second.create(revived)

        assertNull(
            "the user already resolved this; it must not be re-litigated",
            onMain { vm2.conflict(ProfileFieldId.NAME) },
        )
        assertEquals("My Name", onMain { vm2.buffers.value }.name)
        assertTrue(onMain { vm2.isDirty(ProfileFieldId.NAME) })
        second.clear()
    }

    /**
     * A corrupt or structurally-incomplete stash is not a crash: `decode` returns null (a *shape*
     * failure) and the VM checks out fresh. D27 moved the *version* gate off the codec and onto
     * `acceptStash` (see [d27_aStashFromAnUnknownSchemaIsRefusedAndTheVmStartsFresh]); what remains
     * here is decode's shape gate — a stash missing fields cannot be reconstructed at all.
     */
    @Test
    fun aCorruptStashDegradesToAFreshCheckout() {
        assertNull(ProfileStashCodec.decode("{not json"))
        assertNull(ProfileStashCodec.decode("""{"schema_version":1,"username":{}}""")) // present version, missing fields
        assertNull(ProfileStashCodec.decodeValues("[]"))

        val handle = SavedStateHandle()
        val host = VmHost()
        val vm = host.create(handle)
        assertFalse(vm.restoredFromStash)
        assertEquals("alice", onMain { vm.buffers.value }.username)
        host.clear()
    }

    /**
     * D27 — a persisted stash whose schema version this build does not accept is refused **wholesale**
     * and the VM starts a fresh session *observably*: `stashWasRefused` is true, distinct from a cold
     * start where it is false. A constraint tightened between app versions is the realistic cause: the
     * bytes decode fine (shape is unchanged), but `acceptStash` declines the version and no field of
     * the old edit is trusted.
     */
    @Test
    fun d27_aStashFromAnUnknownSchemaIsRefusedAndTheVmStartsFresh() {
        val handle = SavedStateHandle()
        val first = VmHost()
        val vm1 = first.create(handle)
        onMain { vm1.editName("My Name") }

        val revived = handle.persistAndRevive()
        first.clear()

        // Simulate an upgraded app: rewrite the persisted envelope's version to one this build does
        // not recognise. The bytes stay well-formed — only the schema version is unacceptable.
        val bundle = revived.get<android.os.Bundle>(ProfileViewModel.STASH_KEY)!!
        val stale = ProfileStashCodec.decode(bundle.getString("stash")!!)!!
            .let { it.copy(schemaVersion = it.schemaVersion + 1u) }
        revived[ProfileViewModel.STASH_KEY] =
            android.os.Bundle().apply { putString("stash", ProfileStashCodec.encode(stale)) }

        val second = VmHost()
        val vm2 = second.create(revived)
        assertFalse("a refused stash must not restore an edit session", vm2.restoredFromStash)
        assertTrue("and it is observably a refusal, not a cold start", vm2.stashWasRefused)
        assertEquals("the VM checked out fresh", "alice", onMain { vm2.buffers.value }.username)
        second.clear()
    }
}
