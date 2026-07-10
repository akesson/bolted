package dev.bolted.profileprobe

import com.example.spike_profile_ffi.ProfileDraftFfi
import com.example.spike_profile_ffi.ProfileSnapshot
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.UsernameValidity
import com.example.spike_profile_ffi.snapshots
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Probe matrix C — the `observe` verb on ART. The generated `snapshots()` is an extension function
 * returning a `Flow<ProfileSnapshot>` built with `callbackFlow` over a background poll loop that
 * drains the Rust-side bounded ring (capacity 256, drop-newest).
 */
class StreamProbe {
    private lateinit var store: ProfileStoreFfi
    private lateinit var draft: ProfileDraftFfi

    @Before
    fun setUp() {
        store = seededStore()
        draft = store.checkout()
    }

    @After
    fun tearDown() {
        draft.close()
        store.close()
    }

    private fun usernameOf(snapshot: ProfileSnapshot): String? =
        (snapshot.username.validity as? UsernameValidity.Valid)?.value

    /** End to end: a mutation produces a snapshot the Kotlin collector receives, with the new value. */
    @Test
    fun aMutationIsDeliveredToAFlowCollector() = runBlocking {
        val scope = CoroutineScope(Dispatchers.Default)
        val firstSnapshot = scope.async { draft.snapshots().first() }
        delay(SUBSCRIBE_SETTLE_MS) // no signal exists for "subscription established" — see below

        draft.trySetUsername("bob_the_user")

        val snapshot = withTimeout(5_000) { firstSnapshot.await() }
        assertEquals("bob_the_user", usernameOf(snapshot))
        scope.cancel()
    }

    /** Which thread does the poll loop deliver on? Recorded, not asserted — it is an observation. */
    @Test
    fun theDeliveryThreadIsRecorded() = runBlocking {
        val threadName = AtomicReference<String>()
        val scope = CoroutineScope(Dispatchers.Default)
        val done =
            scope.async {
                draft.snapshots().first().also { threadName.set(Thread.currentThread().name) }
            }
        delay(SUBSCRIBE_SETTLE_MS)
        draft.trySetUsername("bob_the_user")
        withTimeout(5_000) { done.await() }

        record("stream.delivery_thread", threadName.get())
        assertNotNull(threadName.get())
        scope.cancel()
    }

    /**
     * The Compose binding shape: collect on the **main Looper** while mutating off it. Apple's
     * `@MainActor` consumer worked; this is the Android equivalent.
     */
    @Test
    fun theFlowCanBeCollectedOnTheMainLooper() = runBlocking {
        val onMain = AtomicReference(false)
        val received = AtomicReference<String>()
        val scope = CoroutineScope(Dispatchers.Default)

        val collector =
            scope.launch(Dispatchers.Main) {
                val snapshot = draft.snapshots().first()
                onMain.set(Thread.currentThread().name == "main")
                received.set(usernameOf(snapshot))
            }
        delay(SUBSCRIBE_SETTLE_MS)
        draft.trySetUsername("bob_the_user")
        withTimeout(5_000) { collector.join() }

        record("stream.collected_on_main_looper", onMain.get().toString())
        assertEquals("bob_the_user", received.get())
        assertTrue("a Compose consumer must be able to collect on the main Looper", onMain.get())
        scope.cancel()
    }

    /**
     * **The subscribe race.** Step 02 found a fresh Swift subscription replays *nothing* — it
     * delivers only future events, so a get-current-then-subscribe sequence can miss an event in the
     * gap. Its mitigation: every snapshot carries a `version` stamp, so the caller can reconcile
     * `snapshot()` against the first streamed event. Confirm both halves on Kotlin.
     *
     * Future-only: confirmed. The version mitigation: **it does not work for a draft's own stream.**
     * See [theVersionStampCountsCanonicalChangesNotDraftEdits].
     */
    @Test
    fun aFreshSubscriptionIsFutureOnly() = runBlocking {
        draft.trySetUsername("before_sub")
        val versionAtSubscribe = draft.snapshot().version

        val scope = CoroutineScope(Dispatchers.Default)
        val firstSnapshot = scope.async { draft.snapshots().first() }
        delay(SUBSCRIBE_SETTLE_MS)

        draft.trySetUsername("after_sub")
        val snapshot = withTimeout(5_000) { firstSnapshot.await() }

        assertEquals(
            "a fresh subscription replayed the pre-subscribe state instead of being future-only",
            "after_sub",
            usernameOf(snapshot),
        )
        record(
            "stream.fresh_subscription",
            "future-only; version ${versionAtSubscribe} -> ${snapshot.version}",
        )
        scope.cancel()
    }

    /**
     * **C15 — the draft's version stamp tracks its rebase.** This probe found the bug in step 05 and
     * now guards the fix.
     *
     * Then: a draft snapshot's `version` was its `base_version`, written once by `from_canonical`
     * and never again, because `ProfileDraft::rebase` took no version. After canonical moved to v2
     * and the draft rebased onto it, the draft still *reported* v1 — a stale stamp. So step-02's
     * version-guarded reconcile (the mitigation for the future-only subscribe race) could never fire
     * on a draft stream; it silently worked on the entity stream only, where the store stamps the
     * live version.
     *
     * Now: `StoreDraft::rebase(entity, version)` records the canonical the draft is actually based
     * on. An *edit* still must not bump it — an edit does not move the draft onto a new canonical.
     */
    @Test
    fun theDraftVersionStampAdvancesWithTheRebaseButNotWithAnEdit() {
        val draftAtCheckout = draft.snapshot().version
        val storeAtCheckout = store.canonical()!!.version

        draft.trySetUsername("edited_once") // dirty, so the rebase below will conflict on username
        assertEquals("a draft edit must not bump `version`", draftAtCheckout, draft.snapshot().version)

        store.applyCanonical(SEED.copy(name = "Server Renamed"))

        val draftAfterCanonical = draft.snapshot().version
        val storeAfterCanonical = store.canonical()!!.version
        record(
            "stream.version_semantics",
            "draft: $draftAtCheckout -> $draftAfterCanonical   " +
                "store: $storeAtCheckout -> $storeAfterCanonical",
        )

        // The rebase demonstrably happened: the clean `name` field silently adopted theirs.
        val name = draft.snapshot().name.validity
        assertTrue(
            "precondition: the draft must actually have rebased, got $name",
            name.toString().contains("Server Renamed"),
        )

        assertTrue("the store's version stamp advances", storeAfterCanonical > storeAtCheckout)
        assertEquals(
            "the draft rebased onto the new canonical, so its stamp must name that canonical — " +
                "otherwise version-based reconcile cannot work on a draft stream",
            storeAfterCanonical,
            draftAfterCanonical,
        )
    }

    /**
     * Overflow. The Rust ring is bounded (256) and drops the *newest* when full; the Kotlin poll
     * loop drains it in batches of 16 into a `callbackFlow` channel. Stall the collector, burst far
     * more than the ring holds, and check step 02's ruling still stands: **drop-newest is not a kill
     * as long as current state is recoverable** — here, via the `snapshot()` getter.
     */
    @Test
    fun aBurstMayDropButCurrentStateStaysRecoverable() = runBlocking {
        val seen = CopyOnWriteArrayList<String>()
        val scope = CoroutineScope(Dispatchers.Default)
        val collector =
            scope.launch {
                draft.snapshots().collect { snapshot ->
                    delay(2) // a deliberately slow consumer
                    usernameOf(snapshot)?.let { seen.add(it) }
                }
            }
        delay(SUBSCRIBE_SETTLE_MS)

        val burst = 300
        repeat(burst) { draft.trySetUsername("user%04d".format(it)) }
        val finalName = "user%04d".format(burst - 1)

        delay(1_500)
        collector.cancelAndJoin()
        scope.cancel()

        record("stream.burst_delivered", "${seen.size} of $burst")
        record("stream.burst_last_delivered", seen.lastOrNull() ?: "(none)")
        assertEquals(
            "the snapshot() getter must always read current state, whatever the ring dropped",
            finalName,
            usernameOf(draft.snapshot()),
        )
    }

    private companion object {
        /**
         * `callbackFlow` starts its poll loop asynchronously and there is no "subscribed" signal to
         * await, so the test sleeps. This is itself a finding: the subscribe race is not merely
         * theoretical, it is unavoidable from the consumer's side.
         */
        const val SUBSCRIBE_SETTLE_MS = 400L
    }
}
