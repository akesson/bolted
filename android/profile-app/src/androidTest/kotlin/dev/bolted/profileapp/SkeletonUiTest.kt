package dev.bolted.profileapp

import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import org.junit.Rule
import org.junit.Test

/**
 * **Kill criterion 1**, answered before anything is built on the answer.
 *
 * Step 03 found that XCUITest cannot run headless: it drives a real window, so it needs a logged-in
 * GUI session *and* Accessibility permission for the controlling process. Step 04 banked the
 * contrast — `wasm-bindgen-test` gives the web shell real events into a real render tree with no GUI
 * session at all — and asked whether Android has the same property.
 *
 * It does. This suite launches a real Activity, composes a real tree, and asserts against real
 * semantics nodes, on the same headless `aosp-atd` Gradle-Managed Device the probe uses (`dev34`).
 * No window server, no permission dialog, no human.
 *
 * The `ping()` round trip is here too: it proves the packed `.so` loads inside an **app** process,
 * which the probe (an instrumented library) never demonstrated.
 */
class SkeletonUiTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun theAppComposesAndTheNativeLibraryLoads() {
        compose.onNodeWithTag("skeleton").assertTextEquals("pong: bolted")
    }
}
