package dev.bolted.http.conformance

import android.util.Log
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import dev.bolted.http.ffi.RowReport
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * Step 26 M0 — Gate 1: the JNI harness bridge can carry a green AND be proven able to carry a red.
 *
 * The whole conformance suite runs behind the JNI `HttpHarness`: [`HttpHarness.runC1`] drives the
 * real `bolted-http` C1 rows against a registered Kotlin adapter over an in-process Rust `TestServer`
 * and returns structured [`RowReport`]s. The walking-skeleton [`BoltedHttp`] honours exactly rule-01
 * (GET `/ok`); the other rows are expected red (that is M1's work). This gate asserts:
 *
 *  - **green half** ([theC1Rule01IsGreenOnTheRealAdapter]): rule-01 passes on the real adapter;
 *  - **red half** ([theC1Rule01IsRedWithABrokenAdapter]): a deliberately-broken adapter drives the
 *    same row red with a legible, typed failure message — the bridge itself can fail.
 */
class ConformanceProbe {
    private fun makeHarness(adapter: HttpAdapter): HttpHarness {
        val harness = HttpHarness(adapter)
        // The composition-root dance: adapter first, harness second, then the back-reference so the
        // adapter's completions re-enter this harness.
        when (adapter) {
            is BoltedHttp -> adapter.harness = harness
            is BrokenHttp -> adapter.harness = harness
        }
        return harness
    }

    private fun rowOf(reports: List<RowReport>, needle: String): RowReport? =
        reports.firstOrNull { it.id.contains(needle) }

    @Test
    fun theC1Rule01IsGreenOnTheRealAdapter() {
        val harness = makeHarness(BoltedHttp())
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
        try {
            val reports = harness.runC1()
            val r = rowOf(reports, "rule-01")
                ?: return fail("rule-01 not among reported rows: ${reports.map { it.id }}")
            Log.i(TAG, "M0 GREEN-HALF rule-01 passed=${r.passed} skipped=${r.skipped} msg='${r.message}'")
            assertTrue("C1 rule-01 must be green on the real adapter — message: ${r.message}", r.passed)
            assertFalse("rule-01 must run, not skip", r.skipped)
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    @Test
    fun theC1Rule01IsRedWithABrokenAdapter() {
        val harness = makeHarness(BrokenHttp())
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
        try {
            val reports = harness.runC1()
            val r = rowOf(reports, "rule-01")
                ?: return fail("rule-01 not among reported rows: ${reports.map { it.id }}")
            Log.i(TAG, "M0 RED-HALF rule-01 message: '${r.message}'")
            assertFalse("a broken adapter must drive rule-01 red — the bridge must be able to fail", r.passed)
            assertFalse("a red row must carry a legible, typed failure message", r.message.isEmpty())
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
    }
}
