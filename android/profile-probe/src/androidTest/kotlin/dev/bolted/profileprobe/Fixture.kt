package dev.bolted.profileprobe

import android.util.Log
import com.example.spike_profile_ffi.PlainDate
import com.example.spike_profile_ffi.PlainDateRange
import com.example.spike_profile_ffi.ProfileStoreFfi
import com.example.spike_profile_ffi.ProfileValues
import com.example.spike_profile_ffi.UniquenessChecker
import com.example.spike_profile_ffi.UniquenessVerdictFfi

/**
 * Shared fixture + the measurement channel.
 *
 * Numbers are emitted to logcat under [TAG]. AGP's Gradle-Managed-Device runner saves per-test
 * logcat to `build/outputs/androidTest-results/managedDevice/debug/dev34/logcat-<class>-<test>.txt`,
 * which is how the report's measurements get off the emulator. (Instrumented-test stdout is *not*
 * captured by Gradle, so `println` would vanish.)
 */
const val TAG: String = "BoltedProbe"

fun record(label: String, value: String) {
    Log.i(TAG, "$label = $value")
}

/**
 * A valid seed. Note `alice`, not `corp_alice`: the tier-2 `corporate_email` rule only fires for a
 * `corp_`-prefixed username, and we do not want it firing incidentally in unrelated probes.
 */
val SEED: ProfileValues =
    ProfileValues(
        username = "alice",
        name = "Alice Anderson",
        email = "alice@example.com",
        availability =
            PlainDateRange(
                start = PlainDate(2026.toUShort(), 1.toUByte(), 1.toUByte()),
                end = PlainDate(2026.toUShort(), 12.toUByte(), 31.toUByte()),
            ),
    )

/** A store with canonical already applied, so checked-out drafts rebase like a real one. */
fun seededStore(): ProfileStoreFfi = ProfileStoreFfi.new().also { it.applyCanonical(SEED) }

/**
 * A checker that approves everything. Since C16, a *dirty* username with an unrun check blocks
 * submit, so any probe that edits the username and then submits must run a check first — exactly as
 * a real shell must. Generated as a plain `interface`, not a `fun interface`, so no SAM conversion
 * (step-05 friction 5).
 */
fun uniqueChecker(): UniquenessChecker =
    object : UniquenessChecker {
        override fun checkUnique(username: String): UniquenessVerdictFfi = UniquenessVerdictFfi.UNIQUE
    }

/** Sorted elapsed nanos for [iterations] calls of [body], after [warmup] untimed calls (ART JIT). */
fun timeSorted(iterations: Int, warmup: Int, body: (Int) -> Unit): LongArray {
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

fun LongArray.percentileMs(p: Int): Double = this[(size - 1) * p / 100] / 1_000_000.0

/** Reports median + p95 in milliseconds and returns the median. */
fun LongArray.reportMs(label: String): Double {
    val median = percentileMs(50)
    record(label, "p50=%.4f ms  p95=%.4f ms  n=%d".format(median, percentileMs(95), size))
    return median
}
