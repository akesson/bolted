package dev.bolted.profileapp

import com.example.spike_profile_ffi.ProfileFieldId
import com.example.spike_profile_ffi.UsernameCheckFfi
import com.example.spike_profile_ffi.UsernameValidity
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Kotlin sibling of `ProfileViewModelTests.swift` and `profile-web`'s controller tests, run on
 * ART. Same behaviours on trial: constraint-derived affordances, the echo rule, live rebase,
 * conflict resolution, submit, the debounced async check.
 *
 * A test may name a constraint value. Shell code may not.
 */
class ProfileViewModelTest {

    // ---- constraint-derived affordances --------------------------------------------------------

    @Test
    fun affordancesDeriveFromCoreConstraints() {
        val host = VmHost()
        val vm = host.create()
        assertEquals(20, vm.maxLength(ProfileFieldId.USERNAME))
        assertEquals(30, vm.maxLength(ProfileFieldId.NAME))
        assertNull("Email declares no LenChars — the counter must vanish", vm.maxLength(ProfileFieldId.EMAIL))
        assertTrue(vm.isRequired(ProfileFieldId.USERNAME) && vm.isRequired(ProfileFieldId.AVAILABILITY))
        host.clear()
    }

    // ---- the echo rule --------------------------------------------------------------------------

    @Test
    fun theFocusedBufferIsNeverRewrittenFromTheCore() {
        val host = VmHost()
        val vm = host.create()
        onMain {
            vm.focus(ProfileFieldId.USERNAME)
            vm.editUsername("  bob_1  ")
        }

        // The core parsed and sanitized...
        assertEquals("bob_1", (onMain { vm.snapshot.value }.username.validity as UsernameValidity.Valid).value)
        // ...but the focused control still holds exactly what the user typed. Cursor safety.
        assertEquals("  bob_1  ", onMain { vm.buffers.value }.username)

        // Blur hands ownership back to the core: the sanitized value lands.
        onMain { vm.blur(ProfileFieldId.USERNAME) }
        assertEquals("bob_1", onMain { vm.buffers.value }.username)
        host.clear()
    }

    /**
     * The D9 regression, ported from the web and Swift shells. Sanitization can make a field CLEAN
     * while the control holds live keystrokes: typing `"  alice  "` over the base `"alice"` trims to
     * the same value, so `dirty` is false. Keyed on `dirty`, an unrelated rebase would repaint the
     * buffer, eat the spaces and jump the caret. The predicate is `touched`.
     */
    @Test
    fun aFocusedFieldThatSanitizesBackToBaseStillKeepsItsText() {
        val host = VmHost()
        val vm = host.create()
        onMain {
            vm.focus(ProfileFieldId.USERNAME)
            vm.editUsername("  alice  ") // trims to "alice" == base: CLEAN, but still being typed in
        }
        assertFalse("value never moved", onMain { vm.isDirty(ProfileFieldId.USERNAME) })

        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }
        awaitUntil(what = "the rebase to land") { vm.buffers.value.name == "Server Name" }

        assertEquals("  alice  ", onMain { vm.buffers.value }.username)
        host.clear()
    }

    // ---- live rebase ----------------------------------------------------------------------------

    @Test
    fun aCleanFieldAdoptsCanonicalSilently() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }
        awaitUntil(what = "name to adopt") { vm.buffers.value.name == "Server Name" }

        assertFalse(onMain { vm.isDirty(ProfileFieldId.NAME) })
        assertNull(onMain { vm.conflict(ProfileFieldId.NAME) })
        host.clear()
    }

    @Test
    fun aDirtyFieldConflictsAndPreservesMine() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }
        awaitUntil(what = "the conflict") { vm.conflict(ProfileFieldId.NAME) != null }

        val info = onMain { vm.conflict(ProfileFieldId.NAME) }!!
        assertEquals("Server Name", info.theirs)
        assertEquals("Alice Smith", info.base)
        assertEquals("My Name", onMain { vm.buffers.value }.name)
        host.clear()
    }

    /** C19: the server touched `email`, so `name` must come back dirty, not conflicted. */
    @Test
    fun c19_aDirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Email("team@corp.example")) }
        awaitUntil(what = "the rebase") { vm.buffers.value.email == "team@corp.example" }

        assertNull("`name`'s canonical never moved", onMain { vm.conflict(ProfileFieldId.NAME) })
        assertTrue(onMain { vm.isDirty(ProfileFieldId.NAME) })
        assertEquals("My Name", onMain { vm.buffers.value }.name)
        host.clear()
    }

    @Test
    fun takeTheirsAdoptsAndRefreshesTheBufferEvenWhenFocused() {
        val host = VmHost()
        val vm = host.create()
        onMain {
            vm.focus(ProfileFieldId.NAME)
            vm.editName("My Name")
            vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name"))
        }
        awaitUntil(what = "the conflict") { vm.conflict(ProfileFieldId.NAME) != null }

        onMain { vm.resolveTakeTheirs(ProfileFieldId.NAME) }
        assertEquals("Server Name", onMain { vm.buffers.value }.name)
        assertFalse(onMain { vm.isDirty(ProfileFieldId.NAME) })
        assertNull(onMain { vm.conflict(ProfileFieldId.NAME) })
        host.clear()
    }

    // ---- the async check, and C16 as progress ---------------------------------------------------

    @Test
    fun aDebouncedBurstCollapsesIntoOneCheck() {
        val host = VmHost(ProfileViewModel.Timing(debounceMs = 60))
        val vm = host.create()
        onMain {
            vm.focus(ProfileFieldId.USERNAME)
            for (prefix in listOf("b", "bo", "bob", "bob_", "bob_1")) vm.editUsername(prefix)
        }
        awaitUntil(what = "the single check to land") {
            vm.snapshot.value.usernameCheck is UsernameCheckFfi.Passed
        }
        assertEquals("a burst of five keystrokes is one lookup", 1, vm.checkRunCount)
        record("debounce.checks_for_5_keystrokes", vm.checkRunCount.toString())
        host.clear()
    }

    /**
     * C16's cost. A dirty username with no verdict blocks submit — but the shell must render that as
     * *progress*, not as an error, or the first submit inside the debounce window looks like a
     * failure the user caused. Predicted by the step-06 report; this is the test.
     */
    @Test
    fun anUnrunCheckRendersAsProgressNotAsAnError() {
        val host = VmHost(ProfileViewModel.Timing(debounceMs = 10_000)) // never fires during the test
        val vm = host.create()
        onMain { vm.editUsername("alice2") }

        assertNull("not an error", onMain { vm.inlineError(ProfileFieldId.USERNAME) })
        assertEquals(
            "Checking that this username is free…",
            onMain { vm.progressHint(ProfileFieldId.USERNAME) },
        )
        assertTrue(Localization.isProgress("username_check_required"))

        // ...and it really does block the submit, so this is not cosmetic.
        onMain { vm.submit() }
        assertTrue(onMain { vm.lastSubmit.value } is SubmitOutcome.Validation)
        host.clear()
    }

    @Test
    fun aTakenUsernameRendersAsAnError() {
        val host = VmHost(ProfileViewModel.Timing(debounceMs = 10))
        val vm = host.create()
        onMain { vm.editUsername("admin") }
        awaitUntil(what = "the taken verdict") {
            vm.snapshot.value.usernameCheck is UsernameCheckFfi.Failed
        }
        assertEquals("That username is already taken.", onMain { vm.inlineError(ProfileFieldId.USERNAME) })
        host.clear()
    }

    // ---- submit ---------------------------------------------------------------------------------

    @Test
    fun submitSucceedsAndChecksOutAFreshDraft() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        onMain { vm.submit() }

        assertEquals(SubmitOutcome.Success, onMain { vm.lastSubmit.value })
        assertFalse("a fresh draft is clean", onMain { vm.isDirty(ProfileFieldId.NAME) })
        awaitUntil(what = "canonical to catch up") {
            display(vm.canonical.value!!.name.validity) == "My Name"
        }
        host.clear()
    }

    @Test
    fun submitIsRefusedWhileConflictedAndTheDraftSurvives() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }
        awaitUntil(what = "the conflict") { vm.conflict(ProfileFieldId.NAME) != null }

        onMain { vm.submit() }
        val outcome = onMain { vm.lastSubmit.value }
        assertTrue(outcome is SubmitOutcome.Conflicted)
        assertEquals(listOf(ProfileFieldId.NAME), (outcome as SubmitOutcome.Conflicted).fields)
        assertEquals("the edit session survives a refusal (C17)", "My Name", onMain { vm.buffers.value }.name)

        onMain { vm.resolveKeepMine(ProfileFieldId.NAME) }
        onMain { vm.submit() }
        assertEquals(SubmitOutcome.Success, onMain { vm.lastSubmit.value })
        host.clear()
    }

    @Test
    fun anInvalidFieldBlocksSubmitWithAReportBuiltFromCoreErrorData() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("") }
        assertEquals("Too short — minimum 1, got 0.", onMain { vm.inlineError(ProfileFieldId.NAME) })

        onMain { vm.submit() }
        val outcome = onMain { vm.lastSubmit.value } as SubmitOutcome.Validation
        assertTrue(outcome.report.fieldErrors.any { it.field == ProfileFieldId.NAME })
        host.clear()
    }

    // ---- delivery + lifecycle -------------------------------------------------------------------

    /**
     * Risk 3: the generated `snapshots()` is a `callbackFlow`. A form repainting on every keystroke
     * must not hop threads to do it, and Compose state may only be written from the main thread.
     */
    @Test
    fun draftSnapshotsAreDeliveredOnTheMainThread() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        onMain { vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }
        awaitUntil(what = "a stream-delivered snapshot") { vm.lastSnapshotThread != null }

        assertEquals("main", vm.lastSnapshotThread)
        record("delivery.thread", vm.lastSnapshotThread!!)
        host.clear()
    }

    /**
     * C18 on a real lifecycle. On ART the GC never runs a Rust `Drop`, so a `ViewModel` that forgets
     * `close()` leaks a draft the store rebases forever (step-05 H1). `onCleared()` is the only
     * place it can happen.
     */
    @Test
    fun onClearedClosesTheDraft() {
        val host = VmHost()
        val vm = host.create()
        onMain { vm.editName("My Name") }
        assertNull("not cleared yet", vm.liveDraftsAfterClose)

        host.clear() // what a finishing Activity does

        assertNotNull("onCleared() must have run", vm.liveDraftsAfterClose)
        assertEquals("the draft must be freed (C18)", 0, vm.liveDraftsAfterClose)
        record("c18.live_drafts_after_close", vm.liveDraftsAfterClose.toString())
    }
}
