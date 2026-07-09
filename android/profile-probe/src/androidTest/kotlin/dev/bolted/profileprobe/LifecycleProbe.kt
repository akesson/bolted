package dev.bolted.profileprobe

import com.example.spike_profile_ffi.ProfileDraftFfi
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.UniquenessChecker
import com.example.spike_profile_ffi.UniquenessVerdictFfi
import java.lang.ref.ReferenceQueue
import java.lang.ref.WeakReference
import java.util.concurrent.atomic.AtomicReference
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Probe matrix B — **ARCHITECTURE §9, bullet one**: *"Draft handle lifecycle in GC languages
 * (`close()`? `use`-block?) — pending Step 5."*
 *
 * Step 02 found that on Apple/ARC, letting the Swift handle leave scope runs `deinit` → the BoltFFI
 * release shim → Rust `Drop` → the wrapper prunes the draft, and `liveDraftCount()` falls. No manual
 * `close()` needed.
 *
 * The generated Kotlin handle class is `AutoCloseable` whose `close()` is the only call site of the
 * release shim — no `Cleaner`, no `finalize()`, no `PhantomReference`. Hypothesis H1 says ART's GC
 * therefore never frees the Rust object. These tests decide it on a running ART instance.
 */
class LifecycleProbe {
    private lateinit var store: ProfileStoreFfi

    @Before
    fun setUp() {
        store = seededStore()
    }

    @After
    fun tearDown() {
        store.close()
    }

    /**
     * Harness control. If this fails, no GC-based conclusion in this file means anything: the
     * instrumented APK is `debuggable`, and ART keeps every dex register of a live frame as a GC
     * root in that mode. Establish that a plain object *can* be collected before believing that a
     * draft *was not*.
     */
    @Test
    fun gc_control_aPlainObjectIsCollectable() {
        val abandoned = abandonOnAnotherThread { ByteArray(1024) }
        val collected = awaitCollection(abandoned)
        record("gc_control.plain_bytearray_collected", collected.toString())
        assertTrue(
            "this harness cannot collect even an abandoned ByteArray — every GC assertion here is " +
                "meaningless until that is fixed",
            collected,
        )
    }

    @Test
    fun checkoutRaisesTheLiveDraftCount() {
        assertEquals(0u, store.liveDraftCount())
        val draft = store.checkout()
        assertEquals(1u, store.liveDraftCount())
        draft.close()
        assertEquals(0u, store.liveDraftCount())
    }

    /**
     * **H1, the headline.** Abandon the only Kotlin reference to a draft and collect it.
     *
     * The `WeakReference` assertion is the load-bearing control: it proves ART really did collect
     * the Kotlin object, so a surviving `liveDraftCount()` cannot be explained away with "the GC
     * never ran". The Kotlin wrapper dies; the Rust draft does not.
     */
    @Test
    fun h1_artCollectsTheKotlinHandleButRustNeverFreesTheDraft() {
        val abandoned = checkoutAndAbandon(store)
        assertEquals("checkout should have registered a draft", 1u, store.liveDraftCount())

        val collected = awaitCollection(abandoned)

        record("h1.kotlin_handle_collected", collected.toString())
        record("h1.live_draft_count_after_gc", store.liveDraftCount().toString())

        assertTrue(
            "control failed: ART never collected the abandoned Kotlin handle, so this test cannot " +
                "say anything about whether Rust frees. (Increase the GC pressure.)",
            collected,
        )
        assertNull(abandoned.ref.get())
        assertEquals(
            "H1: the Kotlin handle was collected, yet the Rust draft is still registered. " +
                "Dropping the last reference does NOT run Rust Drop on Android — the opposite of " +
                "Apple/ARC. close() is mandatory. (ARCHITECTURE §9)",
            1u,
            store.liveDraftCount(),
        )
    }

    /**
     * The consequence H1 buys, made concrete: a leaked draft is unreachable from Kotlin (so it can
     * never be closed) yet stays registered, and the store keeps rebasing it on every canonical
     * change. This is the "apply_canonical rebases zombies forever" hazard step 02 anticipated.
     */
    @Test
    fun h1b_aLeakedDraftIsAnUnreachableZombieThatStillRebases() {
        val abandoned = checkoutAndAbandon(store)
        val collected = awaitCollection(abandoned)
        record("h1b.kotlin_handle_collected", collected.toString())
        assertTrue("control failed: the abandoned Kotlin handle was never collected", collected)
        assertEquals(1u, store.liveDraftCount())

        // A background canonical change. The zombie is still in the registry and gets rebased.
        store.applyCanonical(SEED.copy(name = "Server Renamed"))

        assertEquals(
            "the leaked draft is still live and still being rebased",
            1u,
            store.liveDraftCount(),
        )
        assertNull(
            "and it is unreachable from Kotlin, so it can never be closed",
            abandoned.ref.get(),
        )
    }

    @Test
    fun closeFreesTheRustDraft() {
        val draft = store.checkout()
        assertEquals(1u, store.liveDraftCount())
        draft.close()
        assertEquals(0u, store.liveDraftCount())
    }

    /** The idiomatic shape a Compose ViewModel would use. */
    @Test
    fun useBlockFreesAtScopeExit() {
        store.checkout().use { assertEquals(1u, store.liveDraftCount()) }
        assertEquals(0u, store.liveDraftCount())
    }

    /** The generated `AtomicBoolean` should make this safe; a double free would be serious. */
    @Test
    fun doubleCloseIsIdempotent() {
        val draft = store.checkout()
        draft.close()
        draft.close()
        draft.close()
        assertEquals(0u, store.liveDraftCount())
    }

    /**
     * Callback-object lifetime. The generated bindings keep the Kotlin `UniquenessChecker` alive in a
     * `ConcurrentHashMap<Long, UniquenessChecker>` (a strong reference) for as long as Rust holds the
     * callback handle — so abandoning the Kotlin reference is safe, and the checker is still invoked.
     */
    @Test
    fun anAbandonedCheckerSurvivesGcBecauseTheBindingsHoldItStrongly() {
        store.checkout().use { draft ->
            val abandoned = installAndAbandonChecker(draft)
            val collected = awaitCollection(abandoned)
            record("callback.kotlin_checker_collected", collected.toString())

            assertNotNull(
                "the bindings' UniquenessCheckerMap should hold the checker strongly",
                abandoned.ref.get(),
            )
            draft.trySetUsername("bob_the_user")
            assertTrue("the abandoned checker is still invoked from Rust", draft.runUsernameCheck())
        }
    }

    /**
     * **And the callback is released deterministically — unlike the handle.** Dropping the Rust
     * `Box<dyn UniquenessChecker>` (which `close()`ing the draft does) invokes the callback vtable's
     * `free(handle)`, which lands in `UniquenessCheckerCallbacks.free` → `UniquenessCheckerMap.remove`.
     * No finalizer is involved.
     *
     * The asymmetry with H1 is one of ownership direction: **Rust owns the callback** and can release
     * it across the boundary; **Kotlin owns the handle**, and BoltFFI gives Rust no hook to release
     * that — hence `close()`.
     */
    @Test
    fun closingTheDraftReleasesTheCallbackObjectWithoutAFinalizer() {
        val draft = store.checkout()
        val abandoned = installAndAbandonChecker(draft)
        assertNotNull("held strongly while the draft lives", abandoned.ref.get())

        draft.close() // drops the Rust Box<dyn UniquenessChecker> -> free(handle) -> map.remove

        val collected = awaitCollection(abandoned)
        record("callback.collected_after_draft_close", collected.toString())
        assertTrue(
            "closing the draft must release the bindings' strong reference to the checker",
            collected,
        )
    }

    // -- helpers ---------------------------------------------------------------------------------

    /** A weakly-held referent plus the queue it is enqueued on when collected. */
    private class Abandoned<T : Any>(val ref: WeakReference<T>, val queue: ReferenceQueue<T>)

    /**
     * Creates an object on a **separate thread** and keeps only a weak reference to it.
     *
     * Doing this inline in the test method does not work: a dead local still pins the object in the
     * caller's dex frame (an instrumented APK is `debuggable`, so ART treats every dex register of a
     * live frame as a GC root). After `join()` the worker's stack is gone, and the only remaining
     * referents are whatever Rust and the bindings hold — which is exactly the question.
     */
    private fun <T : Any> abandonOnAnotherThread(create: () -> T): Abandoned<T> {
        val queue = ReferenceQueue<T>()
        val slot = AtomicReference<WeakReference<T>>()
        val worker = Thread { slot.set(WeakReference(create(), queue)) }
        worker.start()
        worker.join()
        return Abandoned(slot.get(), queue)
    }

    private fun checkoutAndAbandon(store: ProfileStoreFfi): Abandoned<ProfileDraftFfi> =
        abandonOnAnotherThread { store.checkout() }

    private fun installAndAbandonChecker(draft: ProfileDraftFfi): Abandoned<UniquenessChecker> =
        abandonOnAnotherThread {
            // Generated as a plain interface, not a `fun interface` — no SAM conversion.
            val checker =
                object : UniquenessChecker {
                    override fun checkUnique(username: String): UniquenessVerdictFfi =
                        if (username == "admin") UniquenessVerdictFfi.TAKEN
                        else UniquenessVerdictFfi.UNIQUE
                }
            draft.setUniquenessChecker(checker)
            checker
        }

    /**
     * Waits for collection by polling the `ReferenceQueue` — and never calling `ref.get()`.
     *
     * ART's concurrent-copying collector puts a **read barrier** on `Reference.get()`: reading the
     * referent marks it reachable for the GC cycle in progress. A loop of `get()`-then-`gc()`
     * therefore keeps the object alive indefinitely. The first version of this probe did exactly
     * that, and made even an abandoned `ByteArray` look uncollectable — see
     * `gc_control_aPlainObjectIsCollectable`, which exists to catch precisely this class of mistake.
     */
    private fun awaitCollection(abandoned: Abandoned<*>, rounds: Int = 40): Boolean {
        var sink = 0L
        Thread.sleep(50)
        repeat(rounds) {
            System.gc()
            System.runFinalization()
            repeat(16) {
                val ballast = ByteArray(512 * 1024)
                sink += ballast.size.toLong() // keep the allocation from being elided
            }
            if (abandoned.queue.poll() != null) return true
            Thread.sleep(20)
        }
        check(sink >= 0)
        return false
    }
}
