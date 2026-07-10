package dev.bolted.profileapp

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.spike_profile_ffi.DraftStatusFfi
import com.example.spike_profile_ffi.ProfileFieldId
import com.example.spike_profile_ffi.ProfileSnapshot
import com.example.spike_profile_ffi.UsernameCheckFfi

/**
 * The view. It holds **no business logic and no constraint literal** — ARCHITECTURE §1's greppable
 * defect. The character counter's maximum comes from `vm.maxLength(field)`, which reads
 * `ProfileField::constraints()` through the FFI, which reads the same `&'static [Constraint]` the
 * core validates against. There is no `20` and no `30` in this file. Search it.
 *
 * Error *sentences* likewise: `vm.inlineError(field)` renders `ErrorData` through
 * [Localization]; the numbers inside them are params the core supplied.
 */
@OptIn(androidx.compose.ui.ExperimentalComposeUiApi::class)
@Composable
fun ProfileForm(vm: ProfileViewModel) {
    val snapshot by vm.snapshot.collectAsStateWithLifecycle()
    val buffers by vm.buffers.collectAsStateWithLifecycle()
    val canonical by vm.canonical.collectAsStateWithLifecycle()
    val lastSubmit by vm.lastSubmit.collectAsStateWithLifecycle()

    Column(
        Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
            .semantics { testTagsAsResourceId = true },
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (snapshot.status == DraftStatusFfi.ORPHANED) {
            Text(
                Localization.message(errorData("draft_orphaned")),
                Modifier.testTag("orphan-banner"),
                color = MaterialTheme.colorScheme.error,
            )
        }
        if (vm.restoredFromStash) {
            Text("Restored your unsaved changes.", Modifier.testTag("restored-banner"))
        }

        TextFieldRow(vm, ProfileFieldId.USERNAME, "Username", buffers.username, snapshot, vm::editUsername)
        TextFieldRow(vm, ProfileFieldId.NAME, "Name", buffers.name, snapshot, vm::editName)
        TextFieldRow(vm, ProfileFieldId.EMAIL, "Email", buffers.email, snapshot, vm::editEmail)

        Text("Availability: ${dateText(buffers.start)} → ${dateText(buffers.end)}", Modifier.testTag("availability"))

        Button(onClick = vm::submit, modifier = Modifier.testTag("submit")) { Text("Submit") }
        SubmitOutcomeView(lastSubmit)

        canonical?.let {
            Text("canonical: ${display(it.name.validity)}", Modifier.testTag("canonical-name"))
        }

        // The "server", so live rebase and conflicts are reachable by hand and by test.
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            TextButton({ vm.applyServerChange(ProfileViewModel.ServerChange.Name("Server Name")) }, Modifier.testTag("sim-name")) { Text("srv name") }
            TextButton({ vm.applyServerChange(ProfileViewModel.ServerChange.Email("team@corp.example")) }, Modifier.testTag("sim-email")) { Text("srv email") }
            TextButton({ vm.applyServerChange(ProfileViewModel.ServerChange.ResetToSeed) }, Modifier.testTag("sim-reset")) { Text("reset") }
        }
    }
}

/**
 * `snapshot` is passed in rather than read off `vm` inside the body, and that is load-bearing.
 *
 * Compose only observes `State` reads that happen *during composition*. `vm.conflict(field)` reaches
 * into a `StateFlow`, which is not a `State` — Compose sees nothing. Worse, **strong skipping** (on
 * by default since the Compose compiler moved into Kotlin 2.x) makes this row skippable on unchanged
 * parameters, and `vm` is the same instance forever. Both conflict-banner tests failed exactly this
 * way before the snapshot became a parameter: the core conflicted, the VM knew, and the UI never
 * asked again.
 *
 * The generator's rule (step 10): a Compose shell must take every piece of core state as a parameter
 * or read it through `collectAsStateWithLifecycle`. It must never call a method that reads state.
 */
@Composable
private fun TextFieldRow(
    vm: ProfileViewModel,
    field: ProfileFieldId,
    label: String,
    value: String,
    snapshot: ProfileSnapshot,
    onChange: (String) -> Unit,
) {
    val tag = field.name.lowercase()
    val max = vm.maxLength(field) // from the core. Not from here.

    Column(Modifier.fillMaxWidth()) {
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            label = { Text(if (vm.isRequired(field)) "$label *" else label) },
            singleLine = true,
            isError = vm.inlineError(field, snapshot) != null,
            modifier = Modifier
                .fillMaxWidth()
                .testTag("field-$tag")
                .onFocusChanged { if (it.isFocused) vm.focus(field) else vm.blur(field) },
        )

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (max != null) {
                Text("${value.length}/$max", Modifier.testTag("counter-$tag"))
            }
            if (vm.isDirty(field, snapshot)) Text("•", Modifier.testTag("dirty-$tag"))
            if (field == ProfileFieldId.USERNAME && snapshot.usernameCheck is UsernameCheckFfi.Pending) {
                Text("⏳", Modifier.testTag("spinner-username"))
            }
        }

        vm.inlineError(field, snapshot)?.let {
            Text(it, Modifier.testTag("error-$tag"), color = MaterialTheme.colorScheme.error, overflow = TextOverflow.Visible)
        }
        // C16 is progress, not failure: it must never be red, or users learn to ignore red.
        vm.progressHint(field, snapshot)?.let {
            Text(it, Modifier.testTag("progress-$tag"))
        }

        vm.conflict(field, snapshot)?.let { info ->
            Text("server: ${info.theirs}", Modifier.testTag("conflict-theirs-$tag"))
            info.base?.let { Text("was: $it", Modifier.testTag("conflict-base-$tag")) }
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton({ vm.resolveKeepMine(field) }, Modifier.testTag("keepmine-$tag")) { Text("Keep mine") }
                TextButton({ vm.resolveTakeTheirs(field) }, Modifier.testTag("taketheirs-$tag")) { Text("Take theirs") }
            }
        }
    }
}

@Composable
private fun SubmitOutcomeView(outcome: SubmitOutcome?) {
    when (outcome) {
        null -> Unit
        SubmitOutcome.Success -> Text("Submitted", Modifier.testTag("submit-success"))
        SubmitOutcome.Orphaned ->
            Text(Localization.message(errorData("draft_orphaned")), Modifier.testTag("submit-orphaned"))
        SubmitOutcome.AlreadySubmitted -> Text("Already submitted", Modifier.testTag("submit-already"))
        is SubmitOutcome.Conflicted ->
            Text(
                "Resolve conflicts: " + outcome.fields.joinToString { it.name.lowercase() },
                Modifier.testTag("submit-conflicted"),
            )
        is SubmitOutcome.Validation -> {
            val lines = buildList {
                outcome.report.fieldErrors.forEach {
                    add("${it.field.name.lowercase()}: ${Localization.message(it.error)}")
                }
                outcome.report.ruleErrors.forEach { add(Localization.message(it.error)) }
            }
            Text(lines.joinToString("\n"), Modifier.testTag("submit-validation"))
        }
    }
}

private fun errorData(key: String) = com.example.spike_profile_ffi.ErrorData(key, emptyList())
