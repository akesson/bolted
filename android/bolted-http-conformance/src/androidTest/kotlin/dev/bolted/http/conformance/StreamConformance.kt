package dev.bolted.http.conformance

import android.util.Log
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import dev.bolted.http.ffi.RowReport
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Step 27 M4 — the streaming seam on the instrumented tier. N2's `StreamProbe` machinery (the
 * pre-0.28.0 `ffi_stream` `Chunk`/`chunkStream`/`callbackFlow` probe) has **graduated** into the
 * shipped contract path: the OkHttp adapter reads the response body **source** in its own read loop
 * and pushes each read across JNI via `HttpHarness.deliverChunk`, closing with the single terminal via
 * `HttpHarness.finishBody`. This drives the real streaming rows against the real [BoltedHttp] adapter:
 *
 *  - **Rows 12/13** (`runStreamRows`): slow-consumer completeness + terminal-exactly-once, GREEN on the
 *    real adapter, each watched RED first via a scoped [BoltedHttp.StreamFault] twin (mirroring the
 *    Apple/Linux red-twin discipline).
 *  - **Row 14** (subscription hygiene, streaming-seam §3d): `liveStreams()` returns to baseline (0)
 *    after conformant streamed responses (the 0 baseline asserted first — the positive control), and a
 *    never-finished (`SKIP_TERMINAL`) stream leaves `liveStreams() > 0` (the F-M3-1 red case), detected
 *    by the exact Rust registry count, never a GC/weak-reference poll.
 *
 * Counts and reds are read from the JUnit XML by the `test:android:http` gate, never the exit code.
 */
class StreamConformance {
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

    private fun streamHarness(adapter: HttpAdapter): HttpHarness {
        val harness = harnessFor(adapter)
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
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

    // MARK: - Rows 12/13 GREEN on the real adapter

    /**
     * Rows 12 (slow-consumer completeness) and 13 (terminal-exactly-once) GREEN on the real adapter:
     * the OkHttp body source is read in the adapter's read loop, each read pushed across JNI into the
     * driver-owned completeness gate, and the single terminal delivered on clean end-of-body. Also the
     * positive-control baseline for row 14: no live subscription remains after conformant streams.
     */
    @Test
    fun theStreamingRowsAreGreenOnTheRealAdapter() {
        val harness = streamHarness(BoltedHttp())
        try {
            assertEquals("baseline: no streams before driving any", 0uL, harness.liveStreams())
            val reports = harness.runStreamRows()
            assertFalse("no streaming rows ran", reports.isEmpty())
            for (r in reports.sortedBy { it.id }) {
                record("M4 STREAM [${if (r.passed) "GREEN" else "RED  "}] ${r.id}${if (r.message.isEmpty()) "" else " — ${r.message}"}")
                assertTrue("streaming row ${r.id} must be GREEN on the real adapter — ${r.message}", r.passed)
                assertFalse("streaming row ${r.id} must run, not skip", r.skipped)
            }
            assertEquals(
                "row 14: conformant streamed responses must leave no live subscription",
                0uL,
                harness.liveStreams(),
            )
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - Row 12 RED (dropped chunk ⇒ completeness gate fires)

    /**
     * Watched-red (row 12): [BoltedHttp.StreamFault.DROP_CHUNK] drops the first transport read but counts
     * its bytes toward the declared total, so the core completeness gate fires (declared > ingested) and
     * row 12 goes RED — the truncation the gate forbids, on the REAL Android adapter.
     */
    @Test
    fun theStreamingRow12IsRedOnADroppedChunk() {
        val harness = streamHarness(BoltedHttp(streamFault = BoltedHttp.StreamFault.DROP_CHUNK))
        try {
            val r = row(harness.runStreamRows(), "row-12")
            record("M4 RED row-12 (DROP_CHUNK): passed=${r.passed} msg='${r.message}'")
            assertFalse("a dropped chunk must red row 12 (completeness gate) — ${r.message}", r.passed)
            assertFalse("a red row must carry a legible, typed failure message", r.message.isEmpty())
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - Row 13 RED (missing terminal)

    /**
     * Watched-red (row 13): [BoltedHttp.StreamFault.SKIP_TERMINAL] delivers every read but never calls
     * `finishBody`, so no terminal arrives and row 13 goes RED — the missing-terminal break, on the
     * REAL Android adapter. (Double-terminal is impossible by construction — the sink consumes on
     * finish — so the reachable red is the *missing* terminal.)
     */
    @Test
    fun theStreamingRow13IsRedOnAMissingTerminal() {
        val harness = streamHarness(BoltedHttp(streamFault = BoltedHttp.StreamFault.SKIP_TERMINAL))
        try {
            val r = row(harness.runStreamRows(), "row-13")
            record("M4 RED row-13 (SKIP_TERMINAL): passed=${r.passed} msg='${r.message}'")
            assertFalse("a missing terminal must red row 13 — ${r.message}", r.passed)
            assertFalse("a red row must carry a legible, typed failure message", r.message.isEmpty())
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - Row 14 RED (leaked subscription — the F-M3-1 case, made deterministic)

    /**
     * Row 14 RED — the F-M3-1 leak made deterministic: a [BoltedHttp.StreamFault.SKIP_TERMINAL] adapter
     * delivers every read but never sends the terminal, so the driver-owned subscription is never closed
     * and the live-count stays above baseline. Detected by the exact Rust registry count
     * (`liveStreams()`), never by waiting on a weak reference (the step's ART-GC caution).
     */
    @Test
    fun theStreamingRow14IsRedOnALeakedSubscription() {
        val harness = streamHarness(BoltedHttp(streamFault = BoltedHttp.StreamFault.SKIP_TERMINAL))
        try {
            assertEquals("baseline: no streams before driving any", 0uL, harness.liveStreams())
            harness.runStreamRows() // both streams never finish → their subscriptions leak
            val live = harness.liveStreams()
            record("M4 RED row-14 (SKIP_TERMINAL): liveStreams=$live")
            assertTrue(
                "row 14: a never-finished stream must leave a live subscription — the F-M3-1 red case",
                live > 0uL,
            )
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
    }
}
