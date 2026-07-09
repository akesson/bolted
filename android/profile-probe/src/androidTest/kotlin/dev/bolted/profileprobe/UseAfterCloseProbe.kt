package dev.bolted.profileprobe

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * **H2 — use-after-close is undefined behaviour, not a typed error.** Probe matrix B, the isolated row.
 *
 * The generated handle class frees the Rust object in `close()`:
 * ```kotlin
 * override fun close() {
 *     if (__boltffi_closed.compareAndSet(false, true)) Native.boltffi_release_class_…(handle)
 * }
 * ```
 * but every instance method reads `this.handle` **without consulting `__boltffi_closed`**, and a
 * handle is a raw pointer (`__BoltffiHandle::new(value) as usize as u64`). So a call after `close()`
 * hands a dangling pointer to JNI. On Apple this was impossible: ARC kept the object alive, and the
 * wrapper's post-submit tombstone made stale calls inert no-ops.
 *
 * This class **asserts nothing about safety**. It records what actually happens, because the point is
 * that the failure mode is *silent*: freed memory is usually not yet reused, so the read returns the
 * right answer and the bug ships. A crash here is a finding; a pass here is a scarier finding.
 *
 * Excluded from the default suite — see [HazardProbe] and `mise run test:android:hazard`.
 */
@HazardProbe
class UseAfterCloseProbe {

    @Test
    fun readingAFieldAfterCloseIsUnsoundEvenWhenItAppearsToWork() {
        val store = seededStore()
        val draft = store.checkout()
        val idWhileLive = draft.id()
        draft.close()
        assertEquals("close() must deregister the draft", 0u, store.liveDraftCount())

        // UB from here on: `id()` dereferences the freed handle.
        val idAfterClose = runCatching { draft.id() }
        record("h2.id_while_live", idWhileLive.toString())
        record("h2.id_after_close", idAfterClose.map { it.toString() }.getOrElse { "threw: $it" })

        // Encourage the allocator to reuse the freed block, then read the dangling handle again.
        val churn = List(64) { store.checkout() }
        val idAfterChurn = runCatching { draft.id() }
        record("h2.id_after_churn", idAfterChurn.map { it.toString() }.getOrElse { "threw: $it" })

        // Two distinct facts, both bad, neither of which announces itself:
        record(
            "h2.read_after_close_returned_stale_value_silently",
            (idAfterClose.getOrNull() == idWhileLive).toString(),
        )
        record(
            "h2.after_churn_handle_aliases_another_object",
            (idAfterChurn.getOrNull()?.let { it != idWhileLive } ?: false).toString(),
        )
        churn.forEach { it.close() }
        store.close()
    }

    /**
     * The nastier shape: `trySetUsername` dereferences the freed object's `Arc<Mutex<StoreCore>>`
     * field, i.e. it follows a pointer *out of* freed memory and then takes a lock through it.
     */
    @Test
    fun mutatingAfterCloseFollowsAPointerOutOfFreedMemory() {
        val store = seededStore()
        val draft = store.checkout()
        draft.close()

        val outcome = runCatching { draft.trySetUsername("after_close") }
        record(
            "h2.try_set_after_close",
            outcome.fold({ "returned normally (no error, no crash)" }, { "threw: $it" }),
        )
        record("h2.live_draft_count_after", runCatching { store.liveDraftCount().toString() }.getOrElse { "threw: $it" })
        store.close()
    }

    /** Double close is guarded by the generated `AtomicBoolean`; this must NOT double-free. */
    @Test
    fun doubleCloseDoesNotDoubleFree() {
        val store = seededStore()
        val draft = store.checkout()
        draft.close()
        draft.close()
        assertEquals(0u, store.liveDraftCount())
        store.close()
    }
}
