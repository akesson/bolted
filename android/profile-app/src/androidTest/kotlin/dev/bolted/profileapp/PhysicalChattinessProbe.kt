package dev.bolted.profileapp

import android.os.Build
import com.example.gen_profile_ffi.ProfileDraftFfi
import com.example.gen_profile_ffi.ProfileStoreFfi
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * **Kill criterion 4 — the chattiness bar, on the right CPU at last.**
 *
 * Step 05 measured a per-keystroke `try_set` + `snapshot()` round trip at 12–13 µs against a 1.0 ms
 * bar, and said so with a caveat: an arm64 emulator on an arm64 host is *the right VM and the wrong
 * CPU*. ART, JNI and the GC were real, but guest code ran natively on an M-series core, several times
 * faster than the low-end phone VISION names as the worst case. Those numbers are **lower bounds**.
 *
 * This probe therefore refuses to run anywhere but physical silicon. A benchmark that lies about its
 * hardware is worse than no benchmark: `mise run bench:android:device` rejects an `emulator-*` serial,
 * and [assertPhysicalDevice] rejects an emulator fingerprint, so that neither a stray `gradle
 * connectedAndroidTest` nor a future reader can mistake an emulator figure for a hardware one.
 *
 * Run it with:  `mise run bench:android:device`
 */
@PhysicalDevice
class PhysicalChattinessProbe {
    private lateinit var store: ProfileStoreFfi
    private lateinit var draft: ProfileDraftFfi

    /** Precomputed so the timed region measures the FFI crossing, not `String.format`. */
    private val usernames = Array(ITERATIONS) { "user%04d".format(it % 10000) }

    @Before
    fun setUp() {
        assertPhysicalDevice()
        store = ProfileStoreFfi.new().also { it.applyCanonical(SEED) }
        draft = store.checkout(null)
    }

    @After
    fun tearDown() {
        if (::draft.isInitialized) draft.close()
        if (::store.isInitialized) store.close()
    }

    /**
     * **The bar.** A realistic keystroke is `try_set` + `snapshot()`: the shell writes the character
     * and repaints from the returned state (ARCHITECTURE §4, snapshot-per-change).
     *
     * Bar: median > 1.0 ms ⇒ the "core validates every keystroke" contract needs a shell-side write
     * buffer, which is a design change, not an optimization. A 60 fps frame is 16.7 ms.
     */
    @Test
    fun perKeystrokeRoundTripIsUnderTheKillBarOnHardware() {
        val samples = timeSorted(ITERATIONS, WARMUP) {
            draft.trySetUsername(usernames[it])
            draft.snapshot()
        }
        val median = samples.reportMs("HW.KEYSTROKE(try_set+snapshot)")
        record("HW.device", "${Build.MANUFACTURER} ${Build.MODEL} (API ${Build.VERSION.SDK_INT})")
        assertTrue(
            "KILL CRITERION: median per-keystroke round-trip ${"%.4f".format(median)} ms exceeds " +
                "the $KILL_BAR_MS ms bar on ${Build.MODEL}. The core-validates-every-keystroke " +
                "contract needs a shell-side write buffer. Stop and report (step-07 kill criterion 4).",
            median <= KILL_BAR_MS,
        )
    }

    /** The timer's own cost, so a µs-scale JNI figure can be read honestly. */
    @Test
    fun nanoTimeOverheadBaseline() {
        var sink = 0L
        timeSorted(ITERATIONS, WARMUP) { sink += it.toLong() }.reportMs("HW.noop.kotlin")
        assertTrue(sink >= 0)
    }

    /** Each half separately, so "JNI is expensive" can be told apart from "our marshaling is". */
    @Test
    fun eachHalfOfTheRoundTrip() {
        timeSorted(ITERATIONS, WARMUP) { draft.trySetUsername(usernames[it]) }.reportMs("HW.try_set_username")
        timeSorted(ITERATIONS, WARMUP) { draft.snapshot() }.reportMs("HW.snapshot_readback")
    }

    /**
     * The cold path: the first keystroke after a screen opens, nothing JIT-compiled. This is the
     * number a "feels janky on first type" complaint is about, and the one an emulator flatters least.
     * Recorded, never gated on.
     */
    @Test
    fun theFirstKeystrokeIsCold() {
        val coldStore = ProfileStoreFfi.new().also { it.applyCanonical(SEED) }
        val coldDraft = coldStore.checkout(null)
        val start = System.nanoTime()
        coldDraft.trySetUsername("cold_start")
        coldDraft.snapshot()
        val elapsedMs = (System.nanoTime() - start) / 1_000_000.0
        coldDraft.close()
        coldStore.close()
        record("HW.keystroke.cold_first", "%.4f ms".format(elapsedMs))
    }

    /**
     * `ro.kernel.qemu` is gone on modern images, so the fingerprint is the tell. Belt and braces with
     * `mise run bench:android:device`, which rejects an `emulator-*` serial before Gradle is invoked.
     */
    private fun assertPhysicalDevice() {
        val emulator = Build.FINGERPRINT.startsWith("generic") ||
            Build.FINGERPRINT.startsWith("unknown") ||
            Build.FINGERPRINT.contains("generic") ||
            Build.FINGERPRINT.contains("emulator") ||
            Build.MODEL.contains("Emulator") ||
            Build.MODEL.contains("Android SDK built for") ||
            Build.MANUFACTURER.contains("Genymotion") ||
            Build.PRODUCT.contains("sdk") ||
            Build.HARDWARE.contains("goldfish") ||
            Build.HARDWARE.contains("ranchu")
        assertTrue(
            "PhysicalChattinessProbe refuses to run on an emulator (fingerprint=${Build.FINGERPRINT}). " +
                "Step 05 already measured that, and it is a lower bound, not a result. " +
                "Connect a physical device and run `mise run bench:android:device`.",
            !emulator,
        )
    }

    /** Sorted elapsed nanos for [iterations] calls of [body], after [warmup] untimed calls (ART JIT). */
    private fun timeSorted(iterations: Int, warmup: Int, body: (Int) -> Unit): LongArray {
        repeat(warmup) { body(it) }
        val samples = LongArray(iterations)
        for (i in 0 until iterations) {
            val start = System.nanoTime()
            body(i)
            samples[i] = System.nanoTime() - start
        }
        samples.sort()
        return samples
    }

    private fun LongArray.percentileMs(p: Int): Double = this[(size - 1) * p / 100] / 1_000_000.0

    private fun LongArray.reportMs(label: String): Double {
        val median = percentileMs(50)
        record(label, "p50=%.4f ms  p95=%.4f ms  n=%d".format(median, percentileMs(95), size))
        return median
    }

    private companion object {
        const val ITERATIONS = 2000
        const val WARMUP = 200
        const val KILL_BAR_MS = 1.0
    }
}
