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
 * Step 26 — the watched-red baseline + the total-deadline red-watch (retained from M1, extended in M2).
 *
 *  - [theWatchedRedBaseline] — every green row shown RED first under a broken adapter. Extended in M2
 *    to cover the whole suite (C1 + extra rows + C2), so the six M2-synthesis rows (rule-04, rule-10,
 *    key-pin-mismatch, key-insecure-redirect, key-io, row-15) are watched red alongside the M1 rows;
 *  - [theTotalDeadlineIsCallTimeoutNotPerIdle] — the sharp deadline red-watch: the `/drip` trickle row
 *    goes RED under a per-idle `readTimeout` and GREEN under the total `callTimeout`, proving
 *    `callTimeout` is the honest TOTAL deadline (no timer synthesis needed).
 *
 * The full green-on-the-real-adapter gate (all rows, anchor installed) and the N3/PermissionDenied/C3
 * controls live in [M2Conformance].
 */
class M1Conformance {
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

    private fun record(line: String) {
        Log.i(TAG, line)
        println(line)
    }

    @Test
    fun theWatchedRedBaseline() {
        // BrokenHttp (always Transport) reds every row EXCEPT the two that EXPECT Transport. Sweeping
        // the WHOLE suite (C1 + extra + C2) means the six M2-synthesis rows are watched red here too.
        val brokenHarness = harnessFor(BrokenHttp())
        brokenHarness.startServer()
        try {
            val rows = brokenHarness.runC1() + brokenHarness.runC2() + brokenHarness.runExtraRows()
            for (r in rows) {
                if (transportExpecting.any { r.id.contains(it) }) continue
                record("watched-red (BrokenHttp) ${r.id}: passed=${r.passed} msg='${r.message}'")
                assertFalse("watched-red: '${r.id}' must be RED under BrokenHttp — ${r.message}", r.passed)
                assertFalse("watched-red: red row '${r.id}' must carry a legible message", r.message.isEmpty())
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
            record("deadline RED-WATCH (PerIdle/readTimeout): passed=${r.passed} msg='${r.message}'")
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
            record("deadline HONEST (Total/callTimeout): passed=${r.passed} msg='${r.message}'")
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
