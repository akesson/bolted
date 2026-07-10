package dev.bolted.profileapp

import android.os.Bundle
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.example.spike_profile_ffi.AvailabilityFieldState
import com.example.spike_profile_ffi.AvailabilityFieldSync
import com.example.spike_profile_ffi.AvailabilityValidity
import com.example.spike_profile_ffi.ConstraintFfi
import com.example.spike_profile_ffi.DraftStatusFfi
import com.example.spike_profile_ffi.EmailFieldState
import com.example.spike_profile_ffi.EmailFieldSync
import com.example.spike_profile_ffi.EmailValidity
import com.example.spike_profile_ffi.ErrorData
import com.example.spike_profile_ffi.PersonNameFieldState
import com.example.spike_profile_ffi.PersonNameFieldSync
import com.example.spike_profile_ffi.PersonNameValidity
import com.example.spike_profile_ffi.PlainDate
import com.example.spike_profile_ffi.PlainDateRange
import com.example.spike_profile_ffi.ProfileDraftFfi
import com.example.spike_profile_ffi.ProfileFieldId
import com.example.spike_profile_ffi.ProfileSnapshot
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.ProfileValues
import com.example.spike_profile_ffi.SubmitErrorFfi
import com.example.spike_profile_ffi.UniquenessChecker
import com.example.spike_profile_ffi.UniquenessVerdictFfi
import com.example.spike_profile_ffi.UsernameCheckFfi
import com.example.spike_profile_ffi.UsernameFieldState
import com.example.spike_profile_ffi.UsernameFieldSync
import com.example.spike_profile_ffi.UsernameValidity
import com.example.spike_profile_ffi.ValidationReportFfi
import com.example.spike_profile_ffi.snapshots
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * The hand-written stand-in for the `ViewModel` a shell generator (step 10) will emit for Kotlin —
 * the Android sibling of `ProfileViewModel.swift` and `profile-web`'s `ProfileController`.
 *
 * It adds only *when* (debounce, focus, the echo rule's deferral, when to stash), never *what*: no
 * constraint value and no rule threshold is restated here or in [ProfileForm]. Those arrive as
 * `ConstraintFfi` and `ErrorData` from the core.
 *
 * Three things this shell has that the other two do not, and which step 07 exists to exercise:
 *
 *  1. **`onCleared()` closes the draft.** On ART the GC *never* runs a Rust `Drop` (step 05, H1), so
 *     an abandoned draft is an unreachable zombie the store rebases forever. `close()` is the only
 *     free path (C18). Apple's ARC does it for you; Kotlin does not.
 *  2. **The draft outlives the Activity.** A rotation destroys the Activity and keeps the
 *     `ViewModelStore`, so the edit session — and the core-side handle — simply survive.
 *  3. **The draft does *not* outlive the process.** Everything above dies when Android kills us, so
 *     the stash goes into [SavedStateHandle], the only container that survives (C20/C21).
 */
class ProfileViewModel(
    private val savedState: SavedStateHandle,
    private val timing: Timing = Timing(),
    makeChecker: () -> UniquenessChecker = { DefaultChecker(latencyMs = timing.checkLatencyMs) },
) : ViewModel() {

    /** Injectable so tests can collapse the debounce and stretch the check. */
    data class Timing(val debounceMs: Long = 400, val checkLatencyMs: Long = 0)

    // ---- state the view binds to ---------------------------------------------------------------

    private val _snapshot = MutableStateFlow(EMPTY_SNAPSHOT)
    /** The draft's current state — the `observe` verb's item, and the single source of "what". */
    val snapshot: StateFlow<ProfileSnapshot> = _snapshot.asStateFlow()

    private val _canonical = MutableStateFlow<ProfileSnapshot?>(null)
    val canonical: StateFlow<ProfileSnapshot?> = _canonical.asStateFlow()

    private val _buffers = MutableStateFlow(Buffers())
    /** Per-field editing buffers: the text the user types into freely (the echo rule). */
    val buffers: StateFlow<Buffers> = _buffers.asStateFlow()

    private val _lastSubmit = MutableStateFlow<SubmitOutcome?>(null)
    val lastSubmit: StateFlow<SubmitOutcome?> = _lastSubmit.asStateFlow()

    /** Measurement: how many uniqueness checks actually fired (a debounce collapses a burst to one). */
    var checkRunCount = 0
        private set

    /** The thread a draft snapshot was last delivered on — asserted by the main-thread test. */
    @Volatile var lastSnapshotThread: String? = null
        private set

    /** `true` if this VM restored an edit session rather than checking out a fresh one. */
    val restoredFromStash: Boolean

    data class Buffers(
        val username: String = "",
        val name: String = "",
        val email: String = "",
        val start: PlainDate = SEED.availability.start,
        val end: PlainDate = SEED.availability.end,
    )

    // ---- machinery -----------------------------------------------------------------------------

    private val store = ProfileStoreFfi.new()
    private var draft: ProfileDraftFfi
    private val makeChecker = makeChecker

    private var focused: ProfileFieldId? = null

    /**
     * Has the user typed into the focused control since the core last wrote its buffer? This — not
     * `dirty` — is what the echo rule protects. Typing `"  alice  "` over the base value `"alice"`
     * leaves the field CLEAN (the core trims, so the value never moved) while the control holds live
     * keystrokes; repainting it would eat the spaces and jump the caret. Step 06 froze `dirty` and a
     * test falsified it the same day.
     */
    private var focusedTouched = false

    private var checkJob: Job? = null
    private var draftJob: Job? = null

    init {
        // A real app fetches canonical from a server here. The spike seeds it, and after a process
        // death it seeds it AGAIN — which is the whole point: the restored draft rebases onto
        // whatever the server says NOW, not onto what it said when we died.
        store.applyCanonical(savedState.serverState() ?: SEED)

        val stash = savedState.get<Bundle>(STASH_KEY)?.getString(STASH_JSON)?.let(StashCodec::decode)
        restoredFromStash = stash != null
        draft = if (stash != null) store.restore(stash) else store.checkout()
        draft.setUniquenessChecker(makeChecker())

        // The OS asks for this lazily, exactly when it is about to kill us — so the stash is built
        // once, at save time, not on every keystroke.
        savedState.setSavedStateProvider(STASH_KEY) {
            Bundle().apply {
                if (draft.isLive()) putString(STASH_JSON, StashCodec.encode(draft.stash()))
            }
        }

        _snapshot.value = draft.snapshot()
        _canonical.value = store.canonical()
        syncBuffers(draft.snapshot())

        subscribeDraft()
        viewModelScope.launch {
            store.snapshots().collect { _canonical.value = it }
        }
    }

    /**
     * On ART this is the ONLY thing that frees the Rust draft (C18, step-05 H1). `viewModelScope` is
     * cancelled before this runs, so no collector can touch a closed handle — which matters, because
     * use-after-close is silent UB today (§9, step 10 must make it a typed error).
     */
    override fun onCleared() {
        draft.close()
        // Read between the two closes, because there is no safe moment afterwards: querying a closed
        // store is itself use-after-close. C18 says this must be 0 — the assertion has nowhere else
        // to live, and a `bolted-ffi` that raised `DraftClosed` would let it live outside the VM.
        liveDraftsAfterClose = store.liveDraftCount().toInt()
        store.close()
    }

    /** C18's observable: live drafts remaining after `onCleared()` closed ours. Must be 0. */
    @Volatile var liveDraftsAfterClose: Int? = null
        private set

    // ---- editing (the echo rule) ---------------------------------------------------------------

    fun focus(field: ProfileFieldId) {
        focused = field
        focusedTouched = false
    }

    /** On blur the control no longer owns its text, so the buffer refreshes to the core's value. */
    fun blur(field: ProfileFieldId) {
        if (focused == field) {
            focused = null
            focusedTouched = false
        }
        syncBuffers(_snapshot.value)
    }

    private fun touch(field: ProfileFieldId) {
        if (focused == field) focusedTouched = true
    }

    fun editUsername(text: String) = edit(ProfileFieldId.USERNAME) {
        _buffers.value = _buffers.value.copy(username = text)
        runCatching { draft.trySetUsername(text) } // per-keystroke try_set — the bet, exercised
        reconcile(draft.snapshot())
        scheduleCheck()
    }

    fun editName(text: String) = edit(ProfileFieldId.NAME) {
        _buffers.value = _buffers.value.copy(name = text)
        runCatching { draft.trySetName(text) }
        reconcile(draft.snapshot())
    }

    fun editEmail(text: String) = edit(ProfileFieldId.EMAIL) {
        _buffers.value = _buffers.value.copy(email = text)
        runCatching { draft.trySetEmail(text) }
        reconcile(draft.snapshot())
    }

    fun editAvailability(start: PlainDate, end: PlainDate) = edit(ProfileFieldId.AVAILABILITY) {
        _buffers.value = _buffers.value.copy(start = start, end = end)
        runCatching { draft.trySetAvailability(start, end) }
        reconcile(draft.snapshot())
    }

    private inline fun edit(field: ProfileFieldId, body: () -> Unit) {
        if (!draft.isLive()) return
        touch(field)
        body()
    }

    // ---- the async uniqueness check ------------------------------------------------------------

    /**
     * Debounced trigger. Only a valid AND dirty username is worth checking; each keystroke cancels
     * the pending timer, so a burst collapses to one check. A value change resets the verdict in the
     * core (C13), so typing during a pending check invalidates it for free — the shell keeps no
     * bookkeeping of its own.
     */
    private fun scheduleCheck() {
        checkJob?.cancel()
        val snap = _snapshot.value
        if (snap.username.validity !is UsernameValidity.Valid || !snap.username.dirty) return
        checkJob = viewModelScope.launch {
            delay(timing.debounceMs)
            runCheckNow()
        }
    }

    /** Drive one check off the main thread (the foreign checker may block). Exposed for tests. */
    suspend fun runCheckNow() {
        if (!draft.isLive()) return
        checkRunCount += 1
        withContext(Dispatchers.IO) { draft.runUsernameCheck() }
    }

    val isChecking: Boolean get() = _snapshot.value.usernameCheck is UsernameCheckFfi.Pending

    // ---- conflict resolution --------------------------------------------------------------------

    fun resolveKeepMine(field: ProfileFieldId) = resolve(field) { draft.resolveKeepMine(field) }

    fun resolveTakeTheirs(field: ProfileFieldId) = resolve(field) { draft.resolveTakeTheirs(field) }

    /** A resolution moves the value from outside a keystroke, so the buffer refreshes even if focused. */
    private inline fun resolve(field: ProfileFieldId, body: () -> Unit) {
        if (!draft.isLive()) return
        body()
        val snap = draft.snapshot()
        _snapshot.value = snap
        syncBuffers(snap, force = field)
    }

    // ---- submit ---------------------------------------------------------------------------------

    fun submit() {
        if (!draft.isLive()) {
            _lastSubmit.value = SubmitOutcome.AlreadySubmitted
            return
        }
        try {
            draft.submit()
            _lastSubmit.value = SubmitOutcome.Success
            recheckout() // the draft tombstoned on success (C17); start a fresh edit session
        } catch (e: SubmitErrorFfi) {
            _lastSubmit.value = when (e) { // the draft is still alive: keep editing (C17)
                is SubmitErrorFfi.Validation -> SubmitOutcome.Validation(e.report)
                is SubmitErrorFfi.Conflicted -> SubmitOutcome.Conflicted(e.fields)
                is SubmitErrorFfi.Orphaned -> SubmitOutcome.Orphaned
                is SubmitErrorFfi.AlreadySubmitted -> SubmitOutcome.AlreadySubmitted
            }
        }
    }

    // ---- the server simulator --------------------------------------------------------------------

    sealed interface ServerChange {
        data class Username(val value: String) : ServerChange
        data class Name(val value: String) : ServerChange
        data class Email(val value: String) : ServerChange
        data object ResetToSeed : ServerChange
    }

    /** Apply a canonical change: the draft rebases underneath and its stream delivers the result. */
    fun applyServerChange(change: ServerChange) {
        val current = currentCanonicalValues() ?: return
        val next = when (change) {
            is ServerChange.Username -> current.copy(username = change.value)
            is ServerChange.Name -> current.copy(name = change.value)
            is ServerChange.Email -> current.copy(email = change.value)
            ServerChange.ResetToSeed -> SEED
        }
        runCatching { store.applyCanonical(next) }
        // A restored VM must see the same server state, so remember it across process death too.
        savedState[SERVER_KEY] = StashCodec.encodeValues(next)
    }

    // ---- constraint-derived affordances (NO literals here or in the view) -----------------------

    fun constraints(field: ProfileFieldId): List<ConstraintFfi> = store.constraints(field)

    fun maxLength(field: ProfileFieldId): Int? =
        constraints(field).filterIsInstance<ConstraintFfi.LenChars>().firstOrNull()?.max?.toInt()

    fun isRequired(field: ProfileFieldId): Boolean =
        constraints(field).any { it is ConstraintFfi.Required }

    // ---- rendering ------------------------------------------------------------------------------

    // Every reader below takes the snapshot it reads. That is not ceremony: Compose observes `State`
    // reads made *during composition*, and a `vm.conflict(field)` that reaches into a `StateFlow`
    // behind Compose's back is invisible to it. With strong skipping (on by default since the Compose
    // compiler moved into Kotlin 2.x) a row whose parameters have not changed is skipped outright, so
    // a conflict banner would simply never appear. Threading the snapshot through makes the
    // dependency a parameter, which is the only thing Compose can see. See the step-07 report.
    //
    // `maxLength`/`isRequired` need no snapshot: constraints are static metadata, not state.

    /**
     * The inline error for a field: its tier-1 `Invalid` error, plus (for username) a failed
     * uniqueness verdict. A *pending* or *never-run* check is not an error — see [progressHint].
     */
    fun inlineError(field: ProfileFieldId, snap: ProfileSnapshot = _snapshot.value): String? {
        validityError(field, snap)?.let { return Localization.message(it) }
        val check = snap.usernameCheck
        if (field == ProfileFieldId.USERNAME && check is UsernameCheckFfi.Failed) {
            return Localization.message(check.error)
        }
        return null
    }

    /**
     * C16's cost, paid honestly. A dirty username with no verdict blocks submit — but on the frame
     * after a keystroke, and on the frame after a **restore** (C20 drops the verdict on purpose),
     * that is a form still filling in, not a mistake. It renders as progress.
     */
    fun progressHint(field: ProfileFieldId, snap: ProfileSnapshot = _snapshot.value): String? {
        if (field != ProfileFieldId.USERNAME) return null
        return when {
            snap.usernameCheck is UsernameCheckFfi.Pending ->
                Localization.message(ErrorData("username_check_pending", emptyList()))
            snap.username.dirty && snap.usernameCheck is UsernameCheckFfi.Unchecked ->
                Localization.message(ErrorData("username_check_required", emptyList()))
            else -> null
        }
    }

    fun isDirty(field: ProfileFieldId, snap: ProfileSnapshot = _snapshot.value): Boolean = when (field) {
        ProfileFieldId.USERNAME -> snap.username.dirty
        ProfileFieldId.NAME -> snap.name.dirty
        ProfileFieldId.EMAIL -> snap.email.dirty
        ProfileFieldId.AVAILABILITY -> snap.availability.dirty
    }

    /** Conflict banner data: theirs (and the ancestor) as text, read from `Field` state alone. */
    fun conflict(field: ProfileFieldId, snap: ProfileSnapshot = _snapshot.value): ConflictInfo? = when (field) {
        ProfileFieldId.USERNAME -> (snap.username.sync as? UsernameFieldSync.Conflicted)
            ?.let { ConflictInfo(it.base, it.theirs) }
        ProfileFieldId.NAME -> (snap.name.sync as? PersonNameFieldSync.Conflicted)
            ?.let { ConflictInfo(it.base, it.theirs) }
        ProfileFieldId.EMAIL -> (snap.email.sync as? EmailFieldSync.Conflicted)
            ?.let { ConflictInfo(it.base, it.theirs) }
        ProfileFieldId.AVAILABILITY -> (snap.availability.sync as? AvailabilityFieldSync.Conflicted)
            ?.let { ConflictInfo(it.base?.let(::rangeText), rangeText(it.theirs)) }
    }

    // ---- private: streams, reconcile, buffers ---------------------------------------------------

    private fun subscribeDraft() {
        draftJob?.cancel()
        val d = draft
        draftJob = viewModelScope.launch {
            d.snapshots().collect { snap ->
                lastSnapshotThread = Thread.currentThread().name
                reconcile(snap)
            }
        }
    }

    /**
     * Version-guarded reconcile: an OLDER `base_version` is a stale rebase and is dropped (the
     * subscribe-race guard). This can finally fire, because D7/C15 made the stamp advance — before
     * the freeze a draft's version was written once at checkout, so the guard step 02 shipped was
     * dead code on drafts.
     */
    private fun reconcile(snap: ProfileSnapshot) {
        if (snap.version < _snapshot.value.version) return
        _snapshot.value = snap
        syncBuffers(snap)
    }

    private fun recheckout() {
        draft = store.checkout()
        draft.setUniquenessChecker(makeChecker())
        focused = null
        focusedTouched = false
        subscribeDraft()
        val snap = draft.snapshot()
        _snapshot.value = snap
        syncBuffers(snap)
    }

    /**
     * Refresh editing buffers from a snapshot. The native control owns its text while focused **and
     * typed into**; a focused control the user never touched holds nothing worth protecting and
     * adopts a rebase live (D9). `force` names a field whose value moved from outside a keystroke.
     */
    private fun syncBuffers(snap: ProfileSnapshot, force: ProfileFieldId? = null) {
        val keepFocused = focusedTouched && force != focused
        fun keep(field: ProfileFieldId) = focused == field && keepFocused

        var next = _buffers.value
        if (!keep(ProfileFieldId.USERNAME)) next = next.copy(username = display(snap.username.validity))
        if (!keep(ProfileFieldId.NAME)) next = next.copy(name = display(snap.name.validity))
        if (!keep(ProfileFieldId.EMAIL)) next = next.copy(email = display(snap.email.validity))
        if (!keep(ProfileFieldId.AVAILABILITY)) {
            val (start, end) = dateRange(snap.availability.validity)
            next = next.copy(start = start, end = end)
        }
        _buffers.value = next
        if (!keepFocused) focusedTouched = false
    }

    private fun currentCanonicalValues(): ProfileValues? {
        val c = _canonical.value ?: return null
        val u = c.username.validity as? UsernameValidity.Valid ?: return SEED
        val n = c.name.validity as? PersonNameValidity.Valid ?: return SEED
        val e = c.email.validity as? EmailValidity.Valid ?: return SEED
        val a = c.availability.validity as? AvailabilityValidity.Valid ?: return SEED
        return ProfileValues(u.value, n.value, e.value, a.value)
    }

    private fun SavedStateHandle.serverState(): ProfileValues? =
        get<String>(SERVER_KEY)?.let(StashCodec::decodeValues)

    companion object {
        const val STASH_KEY = "bolted.draft"
        private const val STASH_JSON = "stash"
        /** Public so a process-death test can move the "server" while the app is dead. */
        const val SERVER_KEY = "bolted.server"

        /** The `Factory` `MainActivity` uses: `SavedStateHandle` comes from `CreationExtras`. */
        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer { ProfileViewModel(createSavedStateHandle()) }
        }
    }
}

// ---- outcome / helper types --------------------------------------------------------------------

sealed interface SubmitOutcome {
    data object Success : SubmitOutcome
    data class Validation(val report: ValidationReportFfi) : SubmitOutcome
    data class Conflicted(val fields: List<ProfileFieldId>) : SubmitOutcome
    data object Orphaned : SubmitOutcome
    data object AlreadySubmitted : SubmitOutcome
}

data class ConflictInfo(val base: String?, val theirs: String)

/** A foreign uniqueness checker with a small in-memory taken-set, so a `Failed` verdict is reachable. */
class DefaultChecker(
    private val taken: Set<String> = setOf("taken", "admin", "root"),
    private val latencyMs: Long = 0,
) : UniquenessChecker {
    override fun checkUnique(username: String): UniquenessVerdictFfi {
        if (latencyMs > 0) Thread.sleep(latencyMs)
        return if (username.lowercase() in taken) UniquenessVerdictFfi.TAKEN else UniquenessVerdictFfi.UNIQUE
    }
}

val SEED: ProfileValues =
    ProfileValues(
        username = "alice",
        name = "Alice Smith",
        email = "alice@example.com",
        availability = PlainDateRange(
            start = PlainDate(2026.toUShort(), 1.toUByte(), 1.toUByte()),
            end = PlainDate(2026.toUShort(), 12.toUByte(), 31.toUByte()),
        ),
    )

// ---- static projection helpers (the monomorphic per-value cost, on the Kotlin side) -------------

internal fun display(v: UsernameValidity): String = when (v) {
    is UsernameValidity.Unset -> ""
    is UsernameValidity.Valid -> v.value
    is UsernameValidity.Invalid -> v.raw
}

internal fun display(v: PersonNameValidity): String = when (v) {
    is PersonNameValidity.Unset -> ""
    is PersonNameValidity.Valid -> v.value
    is PersonNameValidity.Invalid -> v.raw
}

internal fun display(v: EmailValidity): String = when (v) {
    is EmailValidity.Unset -> ""
    is EmailValidity.Valid -> v.value
    is EmailValidity.Invalid -> v.raw
}

internal fun dateRange(v: AvailabilityValidity): Pair<PlainDate, PlainDate> = when (v) {
    is AvailabilityValidity.Valid -> v.value.start to v.value.end
    is AvailabilityValidity.Invalid -> v.raw.start to v.raw.end
    is AvailabilityValidity.Unset -> SEED.availability.start to SEED.availability.end
}

internal fun rangeText(r: PlainDateRange): String = "${dateText(r.start)} → ${dateText(r.end)}"

internal fun dateText(d: PlainDate): String =
    "%04d-%02d-%02d".format(d.year.toInt(), d.month.toInt(), d.day.toInt())

internal fun validityError(field: ProfileFieldId, snap: ProfileSnapshot): ErrorData? = when (field) {
    ProfileFieldId.USERNAME -> (snap.username.validity as? UsernameValidity.Invalid)?.error
    ProfileFieldId.NAME -> (snap.name.validity as? PersonNameValidity.Invalid)?.error
    ProfileFieldId.EMAIL -> (snap.email.validity as? EmailValidity.Invalid)?.error
    ProfileFieldId.AVAILABILITY -> (snap.availability.validity as? AvailabilityValidity.Invalid)?.error
}

/** An all-unset snapshot, used only before `init` finishes. */
internal val EMPTY_SNAPSHOT: ProfileSnapshot = ProfileSnapshot(
    username = UsernameFieldState(UsernameValidity.Unset, UsernameFieldSync.InSync, false),
    name = PersonNameFieldState(PersonNameValidity.Unset, PersonNameFieldSync.InSync, false),
    email = EmailFieldState(EmailValidity.Unset, EmailFieldSync.InSync, false),
    availability = AvailabilityFieldState(AvailabilityValidity.Unset, AvailabilityFieldSync.InSync, false),
    usernameCheck = UsernameCheckFfi.Unchecked,
    anyDirty = false,
    conflicts = emptyList(),
    status = DraftStatusFfi.LIVE,
    version = 0uL,
)
