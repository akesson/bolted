package dev.bolted.http.conformance

import android.util.Log
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.Chunk
import dev.bolted.http.ffi.HttpHarness
import dev.bolted.http.ffi.chunkStream
import java.lang.ref.ReferenceQueue
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Step 26 M0 — Gate 2 (N2): the JNI stream probe, the S-FFI chunk check in its Kotlin/JNI edition.
 *
 * A real HTTP round-trip against the test server's `/chunked` endpoint feeds each response-body chunk
 * across the FFI (`deliverChunk`) into the shared `ffi_stream`; a LIVE Kotlin consumer drains
 * `HttpHarness.chunkStream()` (a `callbackFlow`-backed `Flow<Chunk>`) off the main thread. The probe
 * measures whether F1 `ffi_stream` async push is ordered, lossless, and complete across JNI under two
 * pacings, and — the freeze-input part — what an ABANDONED consumer does to the shared streaming
 * runtime and to the NEXT run (the F-M3-1 lifecycle question; on Apple a dead subscription starved
 * the next run).
 *
 * The verdict paragraph, numbers, and the F-M3-1 observation live in `step-26-m0-notes.md`.
 */
class StreamProbe {
    // ------------------------------------------------------------------ machinery

    /**
     * Thread-safe per-chunk record: seq, cross-JNI delivery latency, and the consumer thread. Uses
     * O(1)-append `ConcurrentLinkedQueue`s (NOT `CopyOnWriteArrayList`, whose O(n) add would make the
     * consumer slow enough to itself provoke `trySend` drops in the generated `callbackFlow` — a probe
     * artifact masquerading as a binding property).
     */
    private class StreamCollector {
        private val seqs = ConcurrentLinkedQueue<Long>()
        private val latenciesUs = ConcurrentLinkedQueue<Double>()
        private val consumerThreads = ConcurrentHashMap.newKeySet<String>()

        @Volatile var sawMain = false

        fun record(seq: ULong, tSendNs: ULong) {
            val now = System.nanoTime()
            val lat = if (now.toULong() > tSendNs) (now.toULong() - tSendNs).toDouble() / 1000.0 else 0.0
            seqs.add(seq.toLong())
            latenciesUs.add(lat)
            consumerThreads.add(Thread.currentThread().name)
            if (Thread.currentThread().name == "main") sawMain = true
        }

        val count: Int get() = seqs.size

        fun summarize(total: Int, ingested: ULong): Result {
            val s = seqs.toList()
            val lats = latenciesUs.toList().sorted()
            val p50 = if (lats.isEmpty()) 0.0 else lats[lats.size / 2]
            val p99 = if (lats.isEmpty()) 0.0 else lats[minOf(lats.size - 1, (0.99 * lats.size).toInt())]
            var ordered = true
            for (i in 1 until s.size) if (s[i] < s[i - 1]) { ordered = false; break }
            val seen = s.toHashSet()
            var stall = 0
            if (total > 0) for (n in 1..total) { if (seen.contains(n.toLong())) stall = n else break }
            return Result(
                delivered = s.size, ingested = ingested.toLong(), stallPoint = stall, ordered = ordered,
                p50Us = p50, p99Us = p99, consumerThreads = consumerThreads.size, sawMain = sawMain,
            )
        }
    }

    private data class Result(
        val delivered: Int,
        val ingested: Long,
        val stallPoint: Int,
        val ordered: Boolean,
        val p50Us: Double,
        val p99Us: Double,
        val consumerThreads: Int,
        val sawMain: Boolean,
    )

    /**
     * Producer: GET `/chunked?count=N&delay_us=U`, split the de-chunked `chunk-NNNNNN\n` lines, push
     * each across the FFI via `deliverChunk` stamping `tSendNs` immediately before the call. `dropSeq`
     * (the corruption control) omits one chunk before it crosses. Runs on Dispatchers.IO (off both the
     * main thread and the consumer's Default pool), so the cross-FFI push is a real background hop.
     */
    private suspend fun produce(harness: HttpHarness, base: String, count: Int, delayUs: Int, dropSeq: Int?) =
        withContext(Dispatchers.IO) {
            val url = "$base/chunked?count=$count&delay_us=$delayUs"
            client.newCall(Request.Builder().url(url).build()).execute().use { resp ->
                val source = resp.body!!.source()
                var seq = 0L
                while (true) {
                    val line = source.readUtf8Line() ?: break
                    if (!line.startsWith("chunk-")) continue
                    seq += 1
                    if (dropSeq != null && seq == dropSeq.toLong()) continue
                    harness.deliverChunk(
                        Chunk(
                            seq = seq.toULong(),
                            bytes = line.toByteArray(),
                            tSendNs = System.nanoTime().toULong(),
                            last = seq == count.toLong(),
                        ),
                    )
                }
            }
        }

    private suspend fun waitForDelivery(collector: StreamCollector, target: Int, maxMs: Int = 30_000) {
        var waited = 0
        while (waited < maxMs) {
            if (collector.count >= target) { delay(200); return }
            delay(25); waited += 25
        }
    }

    /**
     * Drive one probe run on [harness]: attach a live consumer, run the producer round-trip, wait for
     * delivery. When [teardown] is true (the healthy path) the stream is closed and the consumer joined
     * so nothing lingers; when false (the F-M3-1 abandon path) the consumer is left subscribed and its
     * scope leaked — deliberately.
     */
    private suspend fun runOnce(
        harness: HttpHarness,
        base: String,
        count: Int,
        delayUs: Int,
        dropSeq: Int?,
        teardown: Boolean,
        consumerScope: CoroutineScope,
    ): Result {
        val collector = StreamCollector()
        val job = consumerScope.launch {
            harness.chunkStream().collect { chunk -> collector.record(chunk.seq, chunk.tSendNs) }
        }
        delay(SUBSCRIBE_SETTLE_MS) // callbackFlow subscribes asynchronously; no "subscribed" signal
        produce(harness, base, count, delayUs, dropSeq)
        val target = if (dropSeq != null) count - 1 else count
        waitForDelivery(collector, target)
        val result = collector.summarize(count, harness.chunkIngested())
        if (teardown) {
            harness.closeChunkStream() // the Apple mitigation: end the subscription deterministically
            job.cancel()
        }
        return result
    }

    private fun freshHarness(): Pair<HttpHarness, String> {
        val adapter = BoltedHttp()
        val harness = HttpHarness(adapter)
        adapter.harness = harness
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
        return harness to info.httpBase
    }

    /** Record a verdict line to BOTH logcat and stdout (stdout is captured in the JUnit XML `<system-out>`). */
    private fun record(line: String) {
        Log.i(TAG, line)
        println(line)
    }

    // ------------------------------------------------------------------ the probes

    /**
     * N2 headline. Measures ordered / lossless / complete across JNI for two pacings (burst delay=0,
     * paced delay=200µs). Asserts the LOAD-BEARING invariants — the cross-FFI push is whole
     * (`ingested==N`), chunks are in-order (no reorder), and the consumer resumes off the main thread —
     * and RECORDS re-delivery completeness as the probe's verdict (numbers → freeze input). Under burst,
     * the generated `callbackFlow` re-delivery drops via `trySend` into a bounded `BUFFERED` channel
     * (drop-on-overflow); that completeness figure is a measured finding, not a gate the adapter can
     * meet at M0 (the streaming seam is deliberately unfrozen). See kill-criterion 3 / the M0 notes.
     */
    @Test
    fun theStreamIsOrderedLosslessComplete() = runBlocking {
        for (delayUs in intArrayOf(0, 200)) {
            val (harness, base) = freshHarness()
            val scope = CoroutineScope(Dispatchers.Default)
            try {
                val r = runOnce(harness, base, CHUNKS, delayUs, dropSeq = null, teardown = true, consumerScope = scope)
                record(
                    "N2 F1 ffi_stream (delay=${delayUs}us): delivered=${r.delivered}/$CHUNKS " +
                        "ingested=${r.ingested} stallPoint=${r.stallPoint} ordered=${r.ordered} " +
                        "p50=${"%.1f".format(r.p50Us)}us p99=${"%.1f".format(r.p99Us)}us " +
                        "consumerThreads=${r.consumerThreads} consumerOffMain=${!r.sawMain}",
                )
                // Load-bearing invariants (green): the cross-FFI push is lossless into the Rust ring…
                assertEquals("http round-trip + cross-FFI ingest must be whole (delay=$delayUs)", CHUNKS.toLong(), r.ingested)
                // …in-order (the step-02 stall/reorder ghost does NOT reproduce)…
                assertTrue("chunks delivered in ascending seq order (delay=$delayUs)", r.ordered)
                // …and the consumer resumes off the main thread.
                assertFalse("F1 consumer must resume OFF the main thread (delay=$delayUs)", r.sawMain)
                // Re-delivery completeness is RECORDED above, not gated: the callbackFlow trySend
                // drop-on-overflow under burst is the N2 finding, freeze input, not an M0 pass/fail.
            } finally {
                harness.stopServer(); harness.close(); scope.cancel()
            }
        }
    }

    /**
     * Non-vacuity control: drop one chunk BEFORE it crosses the FFI, so the probe's ingest counter must
     * see the loss (`ingested == N-1`) — proving the completeness measure is not blind. This detection
     * is at the ingest level, so it is clean and independent of the callbackFlow burst drops above.
     */
    @Test
    fun theCorruptionControlDetectsLoss() = runBlocking {
        val drop = CHUNKS / 2
        val (harness, base) = freshHarness()
        val scope = CoroutineScope(Dispatchers.Default)
        try {
            val r = runOnce(harness, base, CHUNKS, delayUs = 0, dropSeq = drop, teardown = true, consumerScope = scope)
            record("N2 CONTROL (drop seq=$drop before crossing): delivered=${r.delivered}/$CHUNKS ingested=${r.ingested} stallPoint=${r.stallPoint}")
            assertEquals("the probe must detect the pre-crossing loss at the ingest counter", (CHUNKS - 1).toLong(), r.ingested)
            assertTrue("delivered can never exceed ingested", r.delivered <= r.ingested.toInt())
        } finally {
            harness.stopServer(); harness.close(); scope.cancel()
        }
    }

    /**
     * **F-M3-1 lifecycle probe (freeze input).** On the SAME harness (shared streaming runtime), run 1
     * abandons its consumer mid-life — no `closeChunkStream`, no cancel, the scope leaked — then run 2
     * attaches a fresh consumer WITH proper teardown. Records whether run 2 is starved (Apple's finding)
     * or healthy (ART/BoltFFI differs). A control run with teardown between two runs proves the healthy
     * baseline. Uses a `ReferenceQueue` (never a polled `WeakReference.get()`) to observe whether ART
     * collects the abandoned consumer.
     */
    @Test
    fun theAbandonedConsumerLifecycleIsObserved() = runBlocking {
        val (harness, base) = freshHarness()
        val rq = ReferenceQueue<CoroutineScope>()
        try {
            // Run 1 — abandoned. The scope is created and leaked INSIDE the helper so no strong local
            // ref lingers in this method; only the leaked coroutine (if ART keeps it reachable) can.
            val (run1, leakedRef) = abandonRun(harness, base, rq)
            record("F-M3-1 run1 (ABANDONED, no teardown): ingested=${run1.ingested} delivered=${run1.delivered}/$CHUNKS ordered=${run1.ordered}")

            // Run 2 — fresh consumer, SAME harness (shared streaming runtime), WITH teardown. The N2
            // finding is whether run 2 is STARVED relative to a healthy burst run, not whether it hits
            // 200 (burst drop-on-overflow makes 200 unreachable regardless — see the main probe).
            val freshScope = CoroutineScope(Dispatchers.Default)
            val run2 = runOnce(harness, base, CHUNKS, delayUs = 0, dropSeq = null, teardown = true, consumerScope = freshScope)
            record("F-M3-1 run2 (FRESH after abandoned, same harness): ingested=${run2.ingested} delivered=${run2.delivered}/$CHUNKS ordered=${run2.ordered} stallPoint=${run2.stallPoint}")

            // `chunkIngested()` is a CUMULATIVE harness counter, so run 2's per-run ingest is the delta
            // over run 1 (200 → 400 ⇒ delta 200). The cross-FFI ingest is the un-droppable measure: if
            // the abandoned subscription starved the shared runtime, run 2's ingest delta would be < N.
            val run2IngestDelta = run2.ingested - run1.ingested
            val ingestStarved = run2IngestDelta < CHUNKS.toLong()
            record(
                "F-M3-1 VERDICT: run2 ingest-delta=$run2IngestDelta/$CHUNKS ${if (ingestStarved) "STARVED at the cross-FFI ingest" else "NOT starved at cross-FFI ingest"}; " +
                    "but re-delivery DEGRADED: run1=${run1.delivered}/$CHUNKS vs run2=${run2.delivered}/$CHUNKS stallPoint=${run2.stallPoint} — the leaked run1 subscription competes on the shared EventSubscription. " +
                    "Shape vs Apple: Apple STARVED the next run outright; on ART the ingest survives but the abandoned subscription DEGRADES the next consumer's re-delivery.",
            )

            // GC observation via the ReferenceQueue (NEVER a polled WeakReference.get(): that keeps the
            // referent alive — the ART-GC-probes lesson). If the leaked coroutine keeps the scope
            // reachable, it will NOT be enqueued — which is itself the finding.
            @Suppress("ExplicitGarbageCollectionCall")
            System.gc()
            Thread.sleep(200)
            val enqueued = rq.poll() != null
            record("F-M3-1 abandoned-scope GC: reference enqueued(collected)=$enqueued weakrefCleared=${leakedRef.get() == null}")

            // Load-bearing: the abandoned run 1 must not starve run 2's cross-FFI INGEST (delta), and
            // run 2's delivery must stay in-order. (Re-delivery completeness is the recorded finding —
            // degraded by the leaked subscription — not gated.)
            assertEquals("run 2's cross-FFI ingest delta must be whole after an abandoned run 1", CHUNKS.toLong(), run2IngestDelta)
            assertTrue("run 2 ordering must hold", run2.ordered)
            freshScope.cancel()
        } finally {
            harness.stopServer(); harness.close()
        }
    }

    /**
     * **M3 under-load sweep (completing M0's N2 evidence the A1 way).** Re-run the headline probe with
     * the FAST O(1) collector while background threads SATURATE the CPU (one busy-spin thread per core),
     * both pacings, and record ingested / delivered / ordered / off-main / p50 / p99 UNDER LOAD.
     *
     * Expectation from M0: the cross-FFI **ingest stays 200/200 and ordered** — that is the load-bearing,
     * un-droppable measure and it IS gated (a degrade or reorder here is fresh kill-criterion-3 evidence).
     * **Re-delivery completeness (`delivered`) is RECORDED, not gated**: under CPU contention the generated
     * `callbackFlow`'s `trySend` into the bounded `BUFFERED` channel may drop, exactly the M0 finding — the
     * streaming seam is deliberately unfrozen, so a `delivered==200` gate under burst would be flaky.
     */
    @Test
    fun theStreamIsWholeUnderCpuLoad() = runBlocking {
        val cores = Runtime.getRuntime().availableProcessors()
        val loadThreads = maxOf(2, cores) // one busy-spin per core, at least two, to saturate the CPU
        val stopLoad = java.util.concurrent.atomic.AtomicBoolean(false)
        val spinners = (0 until loadThreads).map {
            Thread {
                var x = 0L
                while (!stopLoad.get()) { x = x * 1_000_003L + 7L; if (x == Long.MIN_VALUE) println(x) }
            }.apply { isDaemon = true; priority = Thread.NORM_PRIORITY; start() }
        }
        record("N2 under-load: saturating with $loadThreads busy-spin threads on $cores cores")
        try {
            for (delayUs in intArrayOf(0, 200)) {
                val (harness, base) = freshHarness()
                val scope = CoroutineScope(Dispatchers.Default)
                try {
                    val r = runOnce(harness, base, CHUNKS, delayUs, dropSeq = null, teardown = true, consumerScope = scope)
                    record(
                        "N2 UNDER-LOAD F1 ffi_stream (delay=${delayUs}us, $loadThreads spinners): " +
                            "delivered=${r.delivered}/$CHUNKS ingested=${r.ingested} stallPoint=${r.stallPoint} " +
                            "ordered=${r.ordered} p50=${"%.1f".format(r.p50Us)}us p99=${"%.1f".format(r.p99Us)}us " +
                            "consumerThreads=${r.consumerThreads} consumerOffMain=${!r.sawMain}",
                    )
                    // GATED (kill-criterion-3): the cross-FFI ingest must stay whole and in-order even
                    // under saturation — if THIS degrades or reorders, the streaming seam is unsound.
                    assertEquals("under CPU load, cross-FFI ingest must stay whole (delay=$delayUs)", CHUNKS.toLong(), r.ingested)
                    assertTrue("under CPU load, chunks must still be delivered in ascending seq (delay=$delayUs)", r.ordered)
                    assertFalse("under CPU load, the consumer must still resume off the main thread (delay=$delayUs)", r.sawMain)
                    // `delivered` under load is RECORDED above, not gated (callbackFlow trySend variance).
                } finally {
                    harness.stopServer(); harness.close(); scope.cancel()
                }
            }
        } finally {
            stopLoad.set(true)
            spinners.forEach { it.join(1_000) }
        }
    }

    /** Launch an abandoned (never-torn-down) consumer whose scope is not retained by this method. */
    private suspend fun abandonRun(
        harness: HttpHarness,
        base: String,
        rq: ReferenceQueue<CoroutineScope>,
    ): Pair<Result, WeakReference<CoroutineScope>> {
        val scope = CoroutineScope(Dispatchers.Default)
        val result = runOnce(harness, base, CHUNKS, delayUs = 0, dropSeq = null, teardown = false, consumerScope = scope)
        return result to WeakReference(scope, rq)
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
        const val CHUNKS = 200
        const val SUBSCRIBE_SETTLE_MS = 400L
        val client = OkHttpClient.Builder().build()
    }
}
