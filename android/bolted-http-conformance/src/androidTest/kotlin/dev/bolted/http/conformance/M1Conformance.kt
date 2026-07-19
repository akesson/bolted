package dev.bolted.http.conformance

import android.util.Log
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import dev.bolted.http.ffi.RowReport
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Step 26 M1 — the base adapter, over the real `bolted-http` conformance suite behind the JNI
 * `HttpHarness`. Three assertions, mirroring the step-25 Apple M1 gate:
 *
 *  - [theM1RowsAreGreenExceptTheM2Syntheses] — the full C1 + C2 + extra-rows suite on the real
 *    [BoltedHttp]: every M1 row green, every M2-synthesis row red (pinning, https→http refusal, file
 *    sink / `Io`);
 *  - [theWatchedRedBaseline] — every M1-green row shown RED first under a broken adapter (each newly
 *    passing row must be watched red);
 *  - [theTotalDeadlineIsCallTimeoutNotPerIdle] — the sharp deadline red-watch: the `/drip` trickle row
 *    goes RED when the adapter's deadline is a per-idle `readTimeout`, and GREEN when it is the total
 *    `callTimeout` — proving `callTimeout` is the honest TOTAL deadline (no timer synthesis needed).
 */
class M1Conformance {
    // The M1-green rows (needles into the row ids). Everything the base adapter must honour.
    private val m1Green =
        listOf(
            "rule-01", "rule-02", "rule-03", "rule-05", "rule-06", "rule-07", "rule-08",
            "rule-09", "rule-11",
            "key-timeout", "key-cancelled", "key-tls", "key-name-resolution", "key-connect",
            "key-transport", "key-too-many-redirects",
            "row-redirect-trace", "row-negotiated-version", "row-deadline-total",
        )

    // The M2-synthesis rows — RED under M1 (pinning + trust anchor, https→http refusal, file sink/Io).
    private val m2Red =
        listOf("rule-04", "rule-10", "key-pin-mismatch", "key-insecure-redirect", "key-io", "row-15")

    // The two rows that EXPECT a Transport error, so an always-Transport break cannot red them — the
    // always-200 break does (rule-08: a 200 reads as a hidden retry; key-transport: a 200 where a
    // Transport error was required).
    private val transportExpecting = setOf("rule-08", "key-transport")

    private fun harnessFor(adapter: HttpAdapter): HttpHarness {
        val harness = HttpHarness(adapter)
        // Composition-root dance: adapter first, harness second, then the back-reference.
        when (adapter) {
            is BoltedHttp -> adapter.harness = harness
            is BrokenHttp -> adapter.harness = harness
            is AlwaysOkHttp -> adapter.harness = harness
        }
        return harness
    }

    private fun row(rows: List<RowReport>, needle: String): RowReport {
        val r = rows.firstOrNull { it.id.contains(needle) }
        assertNotNull("no row id contains '$needle' — got ${rows.map { it.id }}", r)
        return r!!
    }

    /** Record a line to BOTH logcat and stdout (stdout is captured in the JUnit XML `<system-out>`). */
    private fun record(line: String) {
        Log.i(TAG, line)
        println(line)
    }

    @Test
    fun theM1RowsAreGreenExceptTheM2Syntheses() {
        val harness = harnessFor(BoltedHttp())
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
        try {
            val rows = harness.runC1() + harness.runC2() + harness.runExtraRows()
            for (r in rows) {
                record("M1 row ${r.id}: passed=${r.passed} skipped=${r.skipped} msg='${r.message}'")
            }
            for (needle in m1Green) {
                val r = row(rows, needle)
                assertTrue("M1 row '$needle' must be GREEN — ${r.id}: ${r.message}", r.passed)
                assertFalse("M1 row '$needle' must run, not skip — ${r.id}", r.skipped)
            }
            for (needle in m2Red) {
                val r = row(rows, needle)
                assertFalse("M2-synthesis row '$needle' must be RED under M1 — ${r.id}: ${r.message}", r.passed)
            }
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    @Test
    fun theWatchedRedBaseline() {
        // BrokenHttp (always Transport) reds every M1-green row except the two that EXPECT Transport.
        val brokenHarness = harnessFor(BrokenHttp())
        brokenHarness.startServer()
        try {
            val rows = brokenHarness.runC1() + brokenHarness.runC2() + brokenHarness.runExtraRows()
            for (needle in m1Green.filter { it !in transportExpecting }) {
                val r = row(rows, needle)
                record("watched-red (BrokenHttp) ${r.id}: passed=${r.passed} msg='${r.message}'")
                assertFalse("watched-red: '$needle' must be RED under BrokenHttp — ${r.id}: ${r.message}", r.passed)
                assertFalse("watched-red: red row '$needle' must carry a legible message — ${r.id}", r.message.isEmpty())
            }
        } finally {
            brokenHarness.stopServer()
            brokenHarness.close()
        }

        // AlwaysOkHttp (always 200) reds the two Transport-expecting rows BrokenHttp cannot.
        val okHarness = harnessFor(AlwaysOkHttp())
        okHarness.startServer()
        try {
            val rows = okHarness.runC1() + okHarness.runC2()
            for (needle in transportExpecting) {
                val r = row(rows, needle)
                record("watched-red (AlwaysOkHttp) ${r.id}: passed=${r.passed} msg='${r.message}'")
                assertFalse("watched-red: '$needle' must be RED under AlwaysOkHttp — ${r.id}: ${r.message}", r.passed)
            }
        } finally {
            okHarness.stopServer()
            okHarness.close()
        }
    }

    @Test
    fun theTotalDeadlineIsCallTimeoutNotPerIdle() {
        // PER-IDLE: a bare readTimeout. `/drip` dribbles a byte every 50 ms, so the idle-timer is
        // continually reset and never fires; the trickle runs to completion → a 200 where the total
        // deadline required Timeout → the row catches it RED. This is the deadline red-watch.
        val perIdle = harnessFor(BoltedHttp(BoltedHttp.DeadlineMode.PerIdle))
        perIdle.startServer()
        try {
            val r = row(perIdle.runExtraRows(), "row-deadline-total")
            record("M1 deadline RED-WATCH (PerIdle/readTimeout): passed=${r.passed} msg='${r.message}'")
            assertFalse("PerIdle (readTimeout) must FAIL the total-deadline row — ${r.message}", r.passed)
        } finally {
            perIdle.stopServer()
            perIdle.close()
        }

        // TOTAL: callTimeout is a wall-clock budget over the whole call; it fires mid-trickle → GREEN.
        val total = harnessFor(BoltedHttp(BoltedHttp.DeadlineMode.Total))
        total.startServer()
        try {
            val r = row(total.runExtraRows(), "row-deadline-total")
            record("M1 deadline HONEST (Total/callTimeout): passed=${r.passed} msg='${r.message}'")
            assertTrue("Total (callTimeout) must PASS the total-deadline row — ${r.message}", r.passed)
        } finally {
            total.stopServer()
            total.close()
        }
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
    }
}
