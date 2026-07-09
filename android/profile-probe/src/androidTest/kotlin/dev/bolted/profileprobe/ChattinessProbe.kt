package dev.bolted.profileprobe

import com.example.spike_profile_ffi.ProfileDraftFfi
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.ping
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * Probe matrix A — **the kill criterion**. ROADMAP: "JNI `try_set` round-trip cost at keystroke
 * frequency — *the* chattiness kill-criterion — perceptible latency means a shell-side write buffer,
 * a design change."
 *
 * **Every number here is a LOWER BOUND.** This is an arm64 emulator on an arm64 host: ART, JNI and
 * the GC are real, but guest code runs natively on an M-series core, several times faster than the
 * low-end phone VISION names as the worst case. A kill that *fires* here is trustworthy; a kill that
 * *clears* here means "not obviously fatal", and must be re-checked on physical hardware in step 07.
 */
class ChattinessProbe {
    private lateinit var store: ProfileStoreFfi
    private lateinit var draft: ProfileDraftFfi

    /** Precomputed so the timed region measures the FFI crossing, not `String.format`. */
    private val usernames = Array(ITERATIONS) { "user%04d".format(it % 10000) }

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

    /** The timer's own cost, so a µs-scale JNI figure can be read honestly. */
    @Test
    fun a_nanoTimeOverheadBaseline() {
        var sink = 0L
        val samples = timeSorted(ITERATIONS, WARMUP) { sink += it.toLong() }
        samples.reportMs("noop.kotlin")
        assertTrue(sink >= 0)
    }

    /**
     * A no-op crossing with a String in and a String out, and no core work behind it.
     *
     * Deliberately *not* called a floor for `try_set`: `ping` allocates and UTF-8-decodes a return
     * String, which `try_set` (void on success) does not. It measures a comparable crossing, not a
     * lower bound — read the two together to split "JNI is expensive" from "our marshaling is".
     */
    @Test
    fun b_pingIsAComparableNoOpCrossing() {
        val samples = timeSorted(ITERATIONS, WARMUP) { ping(usernames[it]) }
        samples.reportMs("ping.noop_crossing")
    }

    /**
     * One keystroke's write half: encode the raw text, cross, parse + validate in the core, emit a
     * snapshot into the stream ring.
     */
    @Test
    fun c_trySetUsernameRoundTrip() {
        val samples = timeSorted(ITERATIONS, WARMUP) { draft.trySetUsername(usernames[it]) }
        samples.reportMs("try_set_username")
    }

    /** One keystroke's read half: marshal the whole `ProfileSnapshot` DTO back across. */
    @Test
    fun d_snapshotReadback() {
        val samples = timeSorted(ITERATIONS, WARMUP) { draft.snapshot() }
        samples.reportMs("snapshot_readback")
    }

    /**
     * **The bar.** A realistic keystroke is `try_set` + `snapshot()`: the shell writes the character
     * and repaints from the returned state (ARCHITECTURE §4, snapshot-per-change).
     *
     * Bar: median > 1.0 ms ⇒ the "core validates every keystroke" contract needs a shell-side write
     * buffer, which is a design change, not an optimization. Rationale: a 60 fps frame is 16.7 ms; a
     * low-end phone runs perhaps 5–10× slower than this emulator's host core, so 1.0 ms here
     * projects to 5–10 ms there — over half a frame for one keystroke, before any UI work.
     */
    @Test
    fun e_perKeystrokeRoundTripIsUnderTheKillBar() {
        val samples =
            timeSorted(ITERATIONS, WARMUP) {
                draft.trySetUsername(usernames[it])
                draft.snapshot()
            }
        val median = samples.reportMs("KEYSTROKE(try_set+snapshot)")
        assertTrue(
            "KILL CRITERION: median per-keystroke round-trip ${"%.4f".format(median)} ms " +
                "exceeds the $KILL_BAR_MS ms bar — on the emulator, which is a lower bound. " +
                "The core-validates-every-keystroke contract needs a shell-side write buffer. " +
                "Stop and report (step doc, Kill criteria #1).",
            median <= KILL_BAR_MS,
        )
    }

    /**
     * Human-scale intuition: a warm 20-character burst, the way a user types a username.
     *
     * Both halves are warmed. Warming only `try_set` (as a first draft of this test did) leaves
     * `snapshot`'s dex/JIT cost inside the timed region and inflates the per-keystroke figure ~6×.
     */
    @Test
    fun f_twentyKeystrokeBurstWallClock() {
        repeat(WARMUP) {
            draft.trySetUsername(usernames[it])
            draft.snapshot()
        }
        val start = System.nanoTime()
        for (i in 0 until 20) {
            draft.trySetUsername(usernames[i])
            draft.snapshot()
        }
        val elapsedMs = (System.nanoTime() - start) / 1_000_000.0
        record("burst.20_keystrokes_warm", "%.4f ms total".format(elapsedMs))
        assertTrue("a 20-keystroke burst took $elapsedMs ms", elapsedMs < 20 * KILL_BAR_MS)
    }

    /**
     * The cold path, measured on purpose rather than leaked into the burst figure: the very first
     * keystroke a user types after a screen opens, with nothing JIT-compiled. This is the number a
     * "feels janky on first type" complaint would be about, and it is the one an emulator flatters
     * least — record it, do not gate on it.
     */
    @Test
    fun g_firstKeystrokeIsCold() {
        val coldStore = seededStore()
        val coldDraft = coldStore.checkout()
        val start = System.nanoTime()
        coldDraft.trySetUsername("cold_start")
        coldDraft.snapshot()
        val elapsedMs = (System.nanoTime() - start) / 1_000_000.0
        coldDraft.close()
        coldStore.close()
        record("keystroke.cold_first", "%.4f ms".format(elapsedMs))
    }

    private companion object {
        const val ITERATIONS = 2000
        const val WARMUP = 200
        const val KILL_BAR_MS = 1.0
    }
}
