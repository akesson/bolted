package dev.bolted.profileapp

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.ComposeContentTestRule
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import org.junit.Rule
import org.junit.Test

/**
 * Real events into a real render tree — **on a headless device**.
 *
 * This is the tier step 03 could not automate: XCUITest drives a window, so `test:apple:ui` needs a
 * logged-in GUI session and Accessibility permission, and has never once run here. Compose's test
 * framework composes and asserts against the semantics tree in-process, on the same `aosp-atd`
 * Gradle-Managed Device the probe uses. Two of Bolted's three shells can verify their UI without a
 * human; the one that cannot is Apple's.
 *
 * The rule launches the real [MainActivity], so this is the real ViewModel, the real
 * `ProfileStoreFfi`, and the real 400 ms debounce.
 */
class ProfileFormTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()

    private fun ComposeContentTestRule.exists(tag: String) =
        onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()

    private fun awaitTag(tag: String) = compose.waitUntil(5_000) { compose.exists(tag) }

    @Test
    fun theFormRendersTheSeededProfile() {
        compose.onNodeWithTag("field-username").assertTextContains("alice")
        compose.onNodeWithTag("counter-username").assertTextEquals("5/20")
        compose.onNodeWithTag("submit").assertIsDisplayed()
    }

    /**
     * A keystroke goes through `try_set` into the real core — the dirty marker proves the core parsed
     * and *trimmed* it — while the focused control keeps exactly what was typed. The echo rule, in a
     * real render tree. Blur hands ownership back and the sanitized value lands.
     *
     * The counter's maximum came from `ProfileField::constraints()`, not from `ProfileForm.kt`.
     */
    @Test
    fun typingDrivesTheCoreWithoutRewritingTheFocusedControl() {
        compose.onNodeWithTag("field-name").performTextClearance()
        compose.onNodeWithTag("field-name").performTextInput("  My Name  ")
        compose.waitForIdle()

        compose.onNodeWithTag("field-name").assertTextContains("  My Name  ") // untouched: cursor safety
        compose.onNodeWithTag("counter-name").assertTextEquals("11/30") // the buffer's length
        compose.onNodeWithTag("dirty-name").assertIsDisplayed() // ...but the core took the trimmed value

        // Move focus away: the buffer refreshes to the core's sanitized value.
        compose.onNodeWithTag("field-email").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("field-name").assertTextContains("My Name")
        compose.onNodeWithTag("counter-name").assertTextEquals("7/30")
    }

    @Test
    fun aRejectedInputRendersTheCoreSentenceWithTheCoreNumbers() {
        compose.onNodeWithTag("field-username").performTextClearance()
        compose.onNodeWithTag("field-username").performTextInput("ab")
        compose.waitForIdle()
        compose.onNodeWithTag("error-username").assertTextEquals("Too short — minimum 3, got 2.")
    }

    /**
     * C16 renders as progress, never as an error. A dirty username inside the debounce window has no
     * verdict yet; showing "Checking that this username is free…" in red, next to "Too short", would
     * teach users to ignore red.
     */
    @Test
    fun anUnrunCheckShowsProgressNotAnError() {
        compose.onNodeWithTag("field-username").performTextClearance()
        compose.onNodeWithTag("field-username").performTextInput("alice2")
        compose.waitForIdle()

        compose.onNodeWithTag("progress-username")
            .assertTextEquals("Checking that this username is free…")
        compose.onNodeWithTag("error-username").assertDoesNotExist()
    }

    /** A canonical change rebases the live draft underneath the composition, and it repaints. */
    @Test
    fun aServerChangeRebasesIntoTheRenderedFields() {
        // clean + unfocused -> silent adopt
        compose.onNodeWithTag("sim-name").performClick()
        compose.waitUntil(5_000) { compose.exists("canonical-name") }
        compose.waitForIdle()
        compose.onNodeWithTag("field-name").assertTextContains("Server Name")
        compose.onNodeWithTag("conflict-theirs-name").assertDoesNotExist()

        // dirty -> conflict; the banner shows theirs, the input keeps mine
        compose.onNodeWithTag("field-email").performTextClearance()
        compose.onNodeWithTag("field-email").performTextInput("mine@other.com")
        compose.onNodeWithTag("sim-email").performClick()
        awaitTag("conflict-theirs-email")

        compose.onNodeWithTag("conflict-theirs-email").assertTextEquals("server: team@corp.example")
        compose.onNodeWithTag("field-email").assertTextContains("mine@other.com")

        // take theirs -> adopt, clean, banner gone
        compose.onNodeWithTag("taketheirs-email").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("field-email").assertTextContains("team@corp.example")
        compose.onNodeWithTag("conflict-theirs-email").assertDoesNotExist()
        compose.onNodeWithTag("dirty-email").assertDoesNotExist()
    }

    /**
     * C19 where a user meets it: I edit `name`, the server changes `email`. Until step 07 this raised
     * a conflict banner on `name` whose "Take theirs" button held the user's own ancestor, and
     * refused the submit.
     */
    @Test
    fun c19_editingOneFieldWhileTheServerChangesAnotherRaisesNoBanner() {
        compose.onNodeWithTag("field-name").performTextClearance()
        compose.onNodeWithTag("field-name").performTextInput("My Name")
        compose.onNodeWithTag("sim-email").performClick()
        compose.waitUntil(5_000) { compose.exists("canonical-name") }
        compose.waitForIdle()

        compose.onNodeWithTag("conflict-theirs-name").assertDoesNotExist()
        compose.onNodeWithTag("dirty-name").assertIsDisplayed()
        compose.onNodeWithTag("field-name").assertTextContains("My Name")
    }

    /** Submit is refused while conflicted, and the edit session survives it (C17). */
    @Test
    fun submitIsRefusedOnConflictThenSucceedsAfterResolution() {
        compose.onNodeWithTag("field-name").performTextClearance()
        compose.onNodeWithTag("field-name").performTextInput("My Name")
        compose.onNodeWithTag("sim-name").performClick()
        awaitTag("conflict-theirs-name")

        compose.onNodeWithTag("submit").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("submit-conflicted").assertTextEquals("Resolve conflicts: name")
        compose.onNodeWithTag("field-name").assertTextContains("My Name")

        compose.onNodeWithTag("keepmine-name").performClick()
        compose.onNodeWithTag("submit").performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("submit-success").assertIsDisplayed()
        awaitTag("canonical-name")
        compose.onNodeWithTag("canonical-name").assertTextEquals("canonical: My Name")
    }

    /**
     * **Risk 2: configuration change.** Rotation destroys the Activity, not the `ViewModelStore`, so
     * the core-side draft handle simply survives — no stash, no `close()`, no re-checkout. The
     * manifest deliberately does not declare `configChanges`, or the app would pass this test by
     * never taking it.
     */
    @Test
    fun theDraftSurvivesAConfigurationChange() {
        compose.onNodeWithTag("field-name").performTextClearance()
        compose.onNodeWithTag("field-name").performTextInput("My Name")
        compose.waitForIdle()
        compose.onNodeWithTag("dirty-name").assertIsDisplayed()

        compose.activityRule.scenario.recreate()
        compose.waitForIdle()

        compose.onNodeWithTag("field-name").assertTextContains("My Name")
        compose.onNodeWithTag("dirty-name").assertIsDisplayed()
        // It never restored from a stash: it is the SAME VM, holding the SAME core-side draft.
        compose.onNodeWithTag("restored-banner").assertDoesNotExist()
    }
}
