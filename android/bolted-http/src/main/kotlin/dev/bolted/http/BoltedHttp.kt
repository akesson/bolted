package dev.bolted.http

import android.system.ErrnoException
import android.system.OsConstants
import dev.bolted.http.ffi.FfiBodyChunk
import dev.bolted.http.ffi.FfiBodyEnd
import dev.bolted.http.ffi.FfiFlowSignal
import dev.bolted.http.ffi.FfiHeader
import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.FfiHttpVersion
import dev.bolted.http.ffi.FfiRequest
import dev.bolted.http.ffi.FfiResponse
import dev.bolted.http.ffi.FfiResponseSink
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import java.io.ByteArrayInputStream
import java.io.File
import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.ProtocolException
import java.net.UnknownHostException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLException
import javax.net.ssl.TrustManager
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.Response
import okio.Buffer
import okio.BufferedSink
import okio.ForwardingSink
import okio.buffer
import okio.sink

/**
 * `BoltedHttp` — the hand-written Android HTTP adapter (architecture.md §1, layer 3), OkHttp edition.
 *
 * **M2 status: the syntheses landed.** On top of M1's base surface (the C2 error taxonomy classified
 * by CAUSE, total deadline via `callTimeout`, caller cancellation, real negotiated version, redirect
 * hop trace, upload progress) M2 adds the four adapter-side syntheses that make the HTTPS + file-sink
 * rows green:
 *
 * - **Server-trust anchoring** ([trustAnchorDer]): a TEST-tier trust anchor installed from
 *   `ServerInfo.goodCertDer`. When set, the per-call client verifies the good (self-signed) test
 *   endpoint against a custom [X509TrustManager] trusting exactly that anchor; the untrusted endpoint
 *   stays rejected (the `key-tls` control). **The shipped adapter hard-codes no anchor** — this is a
 *   test-tier configuration field, mirroring Apple's `trustAnchorDER`.
 * - **SPKI pinning** (rule 10 / pin-mismatch): the custom trust manager does a real chain check
 *   against the anchor and, on a PASSING chain with pins present, compares SHA-256 over the presented
 *   leaf's SubjectPublicKeyInfo. A chain/hostname failure surfaces as [FfiHttpError.Tls]; a chain that
 *   VALIDATES but has no pin matching the leaf SPKI surfaces as [FfiHttpError.PinMismatch] — the
 *   Linux/Apple split, mirrored exactly, never conflated. Any one matching pin satisfies.
 * - **https→http refusal** (rule 4 / insecure-redirect): `followSslRedirects(false)` leaves an
 *   un-followed cross-scheme 302 as the response; [insecureDowngradeTarget] detects the https→http
 *   downgrade and refuses with [FfiHttpError.InsecureRedirect] carrying the cleartext target. Same-scheme
 *   redirects are still auto-followed, so the `priorResponse` hop trace and the too-many-redirects cap
 *   are untouched.
 * - **The file sink** (row 15 / the `Io` positive control): a [FfiResponseSink.File] request streams the
 *   decoded body to the path via Okio (never buffering the whole body), then finalizes atomically
 *   (temp + rename). A write failure surfaces as [FfiHttpError.Io].
 *
 * `PermissionDenied` has a genuine cause mapping ([permissionKeyFor]: a `SecurityException` or an
 * `ErrnoException` `EPERM`/`EACCES`); a live host control is platform-gated on the ART tier (see the
 * M2 notes), so the mapping is proven at the unit level.
 *
 * **M4 status: the streaming seam.** On top of the buffered path the adapter now implements
 * [executeStreaming] (streaming-seam §3a): the OkHttp response body **source** is read in the
 * adapter's own read loop and each read is pushed across JNI via [HttpHarness.deliverChunk]; the
 * single terminal goes through [HttpHarness.finishBody] (`Complete { total }` on clean end-of-body,
 * `Failed { error }` on a mid-body failure, mapped by CAUSE). Cancellation and back-pressure are
 * **pushed** through [signal] (the one [FfiFlowSignal] shape, three uses — the poll-watcher is gone):
 * `Cancel` cancels the `Call`; `Pause`/`Resume` pace the read loop so the core ring never overflows.
 *
 * Completions arrive on an OkHttp dispatcher thread (off the caller) — the async re-entry the Rust
 * bridge expects. No constraint literals: the total deadline comes from [FfiRequest.deadlineMs]; the
 * adapter invents no timeouts, limits, or keys.
 */
class BoltedHttp(
    private val deadlineMode: DeadlineMode = DeadlineMode.Total,
    private val streamFault: StreamFault = StreamFault.NONE,
) : HttpAdapter {
    /**
     * How the request's total deadline is enforced. [Total] (the shipped default) uses OkHttp
     * `callTimeout` — a wall-clock budget over the whole call. [PerIdle] uses a bare `readTimeout` and
     * is a **conformance-only** mode: it exists so the tier can prove `callTimeout` is a *total*
     * deadline by watching the `/drip` trickle row go red under a per-idle timer first (which a trickle
     * keeps resetting so it never fires). The adapter is not forked — one construction flag.
     */
    enum class DeadlineMode { Total, PerIdle }

    /**
     * A deliberately-injected streaming fault — the scoped per-adapter red twin for the streaming rows
     * (the Linux `StreamFault` / Apple `StreamFault` precedent, one fault at a time). [NONE] is the
     * conformant shipped adapter; the conformance red-twin tests construct the adapter with a fault.
     */
    enum class StreamFault {
        /** Conformant: deliver every read, honour the terminal. */
        NONE,

        /**
         * Skip DELIVERING the first read but still count its bytes toward the declared total — the
         * truncation the core completeness gate forbids (row 12's Android red). The ingest stays
         * gapless (no seq consumed for the dropped read); only the declared total disagrees.
         */
        DROP_CHUNK,

        /**
         * Deliver every read but never call `finishBody` — the missing-terminal break (row 13's red)
         * and the leaked-subscription case (row 14's red: the parked sink is never closed).
         */
        SKIP_TERMINAL,
    }

    /**
     * The back-reference the completions re-enter through. Set by the composition root AFTER the
     * harness is built over this adapter (adapter first, harness second, then this assignment).
     */
    var harness: HttpHarness? = null

    /**
     * The good endpoint's DER-encoded certificate, installed as the sole trust anchor for
     * server-trust evaluation. Set by the composition root from `ServerInfo.goodCertDer` after
     * `startServer()`. `null` ⇒ default Android trust evaluation, which rejects the self-signed test
     * certificates (so the good endpoint is a `Tls` failure until an anchor is installed, and the
     * untrusted endpoint stays a `Tls` positive control regardless). The SHIPPED adapter never sets
     * this — it is a test-tier configuration field.
     */
    var trustAnchorDer: ByteArray? = null

    // The engine. Contract-shaped: no cookie jar and no response cache are OkHttp defaults; same-scheme
    // redirects are auto-followed (so the `priorResponse` chain carries the hop trace) and
    // connection-failure retry is OFF (rule 8 — no hidden retry). `followSslRedirects(false)` makes
    // OkHttp DECLINE a cross-scheme redirect (leaving the un-followed 3xx as the response), which the
    // adapter inspects for the https→http downgrade (rule 4). A per-`Call` deadline + the test-tier
    // trust anchor are layered on in `execute`.
    private val client =
        OkHttpClient.Builder()
            .followRedirects(true)
            .followSslRedirects(false)
            .retryOnConnectionFailure(false)
            .build()

    /** In-flight requests keyed by the FFI token, so [signal] reaches the right `Call` and cause. */
    private val inFlight = ConcurrentHashMap<Long, Ctx>()

    /** Per-request mutable state guarded by its own atomics; one instance handles all requests. */
    private class Ctx(val token: ULong) {
        var call: Call? = null

        /** The upload body length when there is a body to send (rule 11), else `null`. */
        var uploadTotal: Long? = null

        /** Cumulative bytes flushed to the socket (rule 11) — the monotone progress source. */
        val sentSoFar = AtomicLong(0)

        /**
         * The recorded caller-cancellation CAUSE. Set by [signal] before `Call.cancel()` so the
         * completion classifies as [FfiHttpError.Cancelled] independently of the opaque `IOException`
         * a cancelled call throws — never by matching the exception message (rule 9 / N6).
         */
        @Volatile var callerCancelled = false

        /**
         * Recorded by the custom trust manager when a declarative SPKI pin did not match a leaf on a
         * PASSING chain (rule 10). It makes the resulting `SSLHandshakeException` classify as
         * [FfiHttpError.PinMismatch] rather than [FfiHttpError.Tls] — the trust-vs-pin split, by CAUSE.
         */
        @Volatile var pinMismatch = false

        /**
         * Set by the per-hop network interceptor to whether the LAST network response OkHttp saw was a
         * redirect (3xx). When OkHttp exhausts its own follow-up cap on a redirect loop, the last hop
         * it saw is a redirect, so [classify] maps the resulting `ProtocolException` to
         * [FfiHttpError.TooManyRedirects] by this recorded CAUSE — never by matching the exception text
         * (Q2; the old `TOO_MANY_REDIRECTS_PREFIX` string match is deleted).
         */
        @Volatile var lastHopWasRedirect = false

        // -- Streaming (streaming-seam §3a/§3b, M4) --

        /** Whether this is a STREAMING request: the read loop pushes chunks, the terminal is `finishBody`. */
        @Volatile var streaming = false

        /** The next `seq` a delivered body chunk carries (ascending, gapless from 0). */
        var nextSeq: ULong = 0uL

        /**
         * Set when the core raised a typed delivery failure (a `seq` violation / ring overflow): it
         * already closed the stream with that failure, so the read loop stops and must NOT finish.
         */
        @Volatile var streamClosedByCore = false

        /** Whether the first read has been dropped yet (the [StreamFault.DROP_CHUNK] one-shot). */
        var droppedOne = false

        /**
         * Back-pressure gate: set by a pushed [FfiFlowSignal.Pause], cleared by [FfiFlowSignal.Resume]
         * (or [FfiFlowSignal.Cancel]). The streaming read loop waits on [pauseLock] while this is set,
         * so the socket back-pressures the server and the core ring never overflows (the Linux
         * read-pacing precedent — OkHttp, like reqwest, has no task-level suspend).
         */
        val paused = AtomicBoolean(false)

        /** The monitor the streaming read loop waits on for resume (lost-wake-up-safe with [paused]). */
        val pauseLock = Object()
    }

    override fun execute(request: FfiRequest) {
        val token = request.token.toLong()
        val ctx = Ctx(request.token)

        val builder = Request.Builder().url(request.url)
        val bodyBytes = request.body
        val requestBody: RequestBody? =
            if (bodyBytes.isNotEmpty()) {
                val contentType: MediaType? =
                    request.headers
                        .firstOrNull { it.name.equals("content-type", ignoreCase = true) }
                        ?.value
                        ?.toMediaTypeOrNull()
                ctx.uploadTotal = bodyBytes.size.toLong()
                ProgressBody(bodyBytes, contentType, ctx)
            } else {
                null
            }
        builder.method(request.method, requestBody)
        for (header in request.headers) {
            builder.addHeader(header.name, header.value)
        }

        val perCallClient = buildPerCallClient(request, ctx)

        val call = perCallClient.newCall(builder.build())
        ctx.call = call
        inFlight[token] = ctx
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    inFlight.remove(token)
                    harness?.completeErr(request.token, classify(ctx, e))
                }

                override fun onResponse(call: Call, response: Response) {
                    response.use { r ->
                        // Rule 4: an un-followed cross-scheme redirect that downgrades https→http is
                        // refused with the typed `InsecureRedirect` carrying the cleartext target.
                        val downgrade = insecureDowngradeTarget(r)
                        if (downgrade != null) {
                            inFlight.remove(token)
                            harness?.completeErr(request.token, FfiHttpError.InsecureRedirect(downgrade))
                            return
                        }

                        when (val sink = request.sink) {
                            // Row 15 / `Io`: stream the body to the file (never buffering the whole
                            // body), atomic finalize; a write failure is `Io`.
                            is FfiResponseSink.File -> {
                                val ok =
                                    try {
                                        sinkBodyToFile(r, sink.path)
                                        true
                                    } catch (e: IOException) {
                                        false
                                    }
                                inFlight.remove(token)
                                if (!ok) {
                                    harness?.completeErr(request.token, FfiHttpError.Io)
                                    return
                                }
                                terminalUploadProgress(ctx, request.token)
                                harness?.completeOk(
                                    FfiResponse(
                                        token = request.token,
                                        status = r.code.toUShort(),
                                        headers = r.headers.map { (n, v) -> FfiHeader(n, v) },
                                        // A file sink reports the destination path with an empty body
                                        // (the bridge builds a `File` outcome from a non-empty sinkPath).
                                        body = ByteArray(0),
                                        finalUrl = r.request.url.toString(),
                                        httpVersion = mapVersion(r.protocol),
                                        hops = redirectHops(r),
                                        sinkPath = sink.path,
                                    ),
                                )
                            }
                            // Memory sink: buffer the decoded body.
                            FfiResponseSink.Memory -> {
                                val body: ByteArray =
                                    try {
                                        r.body?.bytes() ?: ByteArray(0)
                                    } catch (e: IOException) {
                                        inFlight.remove(token)
                                        harness?.completeErr(request.token, classify(ctx, e))
                                        return
                                    }
                                inFlight.remove(token)
                                terminalUploadProgress(ctx, request.token)
                                harness?.completeOk(
                                    FfiResponse(
                                        token = request.token,
                                        status = r.code.toUShort(),
                                        headers = r.headers.map { (n, v) -> FfiHeader(n, v) },
                                        body = body,
                                        finalUrl = r.request.url.toString(),
                                        httpVersion = mapVersion(r.protocol),
                                        hops = redirectHops(r),
                                        // Memory sink → empty sink path (the bridge keeps the body).
                                        sinkPath = "",
                                    ),
                                )
                            }
                        }
                    }
                }
            },
        )
    }

    /**
     * Dispatch a **streaming** request effect (streaming-seam §3a, M4). Enqueues the call; on the
     * response, the body **source** is read in this adapter's own read loop and each read is pushed
     * across JNI via [HttpHarness.deliverChunk], the single terminal via [HttpHarness.finishBody].
     * A `false` from `deliverChunk` means the core closed the stream (a typed `seq`/overflow failure)
     * — the loop stops, cancels the call, and does NOT finish. The total deadline is enforced exactly
     * as in [execute] (OkHttp `callTimeout` spans the whole call incl. the body read); cancel and
     * pause/resume arrive through [signal], not a poll-watcher.
     */
    override fun executeStreaming(request: FfiRequest) {
        val token = request.token.toLong()
        val ctx = Ctx(request.token)
        ctx.streaming = true

        val builder = Request.Builder().url(request.url)
        builder.method(request.method, null)
        for (header in request.headers) {
            builder.addHeader(header.name, header.value)
        }

        val call = buildPerCallClient(request, ctx).newCall(builder.build())
        ctx.call = call
        inFlight[token] = ctx
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    inFlight.remove(token)
                    // The core already closed the stream on a typed delivery failure — nothing to do.
                    if (ctx.streamClosedByCore) return
                    harness?.finishBody(request.token, FfiBodyEnd.Failed(classify(ctx, e)))
                }

                override fun onResponse(call: Call, response: Response) {
                    streamResponse(ctx, request.token, call, response)
                }
            },
        )
    }

    /**
     * Read the response body **source** into the driver-owned ingest, one JNI push per transport read
     * (streaming-seam §3a). Honours pushed back-pressure by read-pacing (the loop waits while
     * [Ctx.paused]); closes with the single terminal via [HttpHarness.finishBody]. Runs on the OkHttp
     * dispatcher thread (`onResponse`).
     */
    private fun streamResponse(ctx: Ctx, token: ULong, call: Call, response: Response) {
        val longToken = token.toLong()
        try {
            // Rule 4: an un-followed cross-scheme https→http redirect is refused as the terminal.
            val downgrade = insecureDowngradeTarget(response)
            if (downgrade != null) {
                inFlight.remove(longToken)
                harness?.finishBody(token, FfiBodyEnd.Failed(FfiHttpError.InsecureRedirect(downgrade)))
                return
            }
            val source = response.body?.source()
            if (source == null) {
                inFlight.remove(longToken)
                if (streamFault == StreamFault.SKIP_TERMINAL) return
                harness?.finishBody(token, FfiBodyEnd.Complete(0uL))
                return
            }
            val readBuffer = Buffer()
            var total = 0uL
            while (true) {
                // Back-pressure read-pacing: while the core has pushed Pause, stop reading (the socket
                // back-pressures the server). Guarded wait — a Resume/Cancel that clears the flag before
                // we wait is not lost (the flag is re-checked under the monitor). A synchronous Pause
                // re-entering `signal` during a `deliverChunk` below only sets the flag (no lock held),
                // so it cannot deadlock — the Linux/Apple discipline, mirrored.
                synchronized(ctx.pauseLock) {
                    while (ctx.paused.get()) {
                        if (call.isCanceled()) return
                        ctx.pauseLock.wait()
                    }
                }
                val read =
                    try {
                        source.read(readBuffer, READ_WINDOW_BYTES)
                    } catch (e: IOException) {
                        inFlight.remove(longToken)
                        if (ctx.streamClosedByCore) return
                        harness?.finishBody(token, FfiBodyEnd.Failed(classify(ctx, e)))
                        return
                    }
                if (read == -1L) break
                val bytes = readBuffer.readByteArray()
                // Every read's bytes count toward the DECLARED total (the completeness gate's
                // denominator), even a dropped one — that is what makes DROP_CHUNK observable.
                total += read.toULong()
                // DROP_CHUNK: skip DELIVERING the first read (its bytes are already counted), so the
                // declared total exceeds the ingested bytes ⇒ the core gate fires ⇒ row 12 red.
                if (streamFault == StreamFault.DROP_CHUNK && !ctx.droppedOne) {
                    ctx.droppedOne = true
                    continue
                }
                val seq = ctx.nextSeq
                ctx.nextSeq += 1uL
                val keepReading = harness?.deliverChunk(token, FfiBodyChunk(seq, bytes)) ?: false
                if (!keepReading) {
                    // The core raised a typed failure and already closed the stream — stop reading,
                    // cancel the call, and do NOT finish (the harness owns that terminal).
                    ctx.streamClosedByCore = true
                    inFlight.remove(longToken)
                    call.cancel()
                    return
                }
            }
            // Clean end of body.
            inFlight.remove(longToken)
            // SKIP_TERMINAL: deliver every read but never finish — the missing-terminal red twin
            // (row 13) and the leaked-subscription case (row 14). The parked sink stays live.
            if (streamFault == StreamFault.SKIP_TERMINAL) return
            harness?.finishBody(token, FfiBodyEnd.Complete(total))
        } catch (e: InterruptedException) {
            // A blocked pause-wait was interrupted (dispatcher teardown / cancel): treat as a cancel
            // terminal unless the core already closed the stream or the fault suppresses the terminal.
            Thread.currentThread().interrupt()
            inFlight.remove(longToken)
            if (!ctx.streamClosedByCore && streamFault != StreamFault.SKIP_TERMINAL) {
                harness?.finishBody(token, FfiBodyEnd.Failed(FfiHttpError.Cancelled))
            }
        } finally {
            response.close()
        }
    }

    /**
     * Push a mid-flight [FfiFlowSignal] to the in-flight call for `token` (streaming-seam §3b / Q4 —
     * the one signal shape, three uses; this replaces the deleted poll-watcher). `Cancel` records the
     * caller-cancel cause then cancels the `Call` (rule 9: the completion classifies as `Cancelled` by
     * cause) and wakes any paused read; `Pause`/`Resume` pace the streaming read loop for back-pressure.
     * A no-op if the token is unknown / already done.
     */
    override fun signal(token: ULong, flow: FfiFlowSignal) {
        val ctx = inFlight[token.toLong()] ?: return
        when (flow) {
            FfiFlowSignal.CANCEL -> {
                ctx.callerCancelled = true
                ctx.call?.cancel()
                synchronized(ctx.pauseLock) {
                    ctx.paused.set(false)
                    ctx.pauseLock.notifyAll()
                }
            }
            // Pause only sets the flag (safe to re-enter synchronously during a `deliverChunk` — no
            // lock is held there); Resume clears it and wakes the loop under the monitor (no lost wake).
            FfiFlowSignal.PAUSE -> ctx.paused.set(true)
            FfiFlowSignal.RESUME ->
                synchronized(ctx.pauseLock) {
                    ctx.paused.set(false)
                    ctx.pauseLock.notifyAll()
                }
        }
    }

    /**
     * Build the per-request OkHttp client: the total-deadline enforcement (no magic number — from
     * `deadlineMs`; `Total` bounds the WHOLE call incl. redirects and the streaming body read, `PerIdle`
     * only a read idle-timer — the conformance-only red-watch), the optional test-tier trust anchor +
     * SPKI pins (rule 10), and a per-hop network interceptor that records whether the last network
     * response was a redirect (so [classify] can map OkHttp's redirect-exhaustion `ProtocolException`
     * to `TooManyRedirects` structurally, never by exception text — Q2).
     */
    private fun buildPerCallClient(request: FfiRequest, ctx: Ctx): OkHttpClient {
        val ms = request.deadlineMs.toLong()
        return client
            .newBuilder()
            .apply {
                when (deadlineMode) {
                    DeadlineMode.Total -> callTimeout(ms, TimeUnit.MILLISECONDS)
                    DeadlineMode.PerIdle -> readTimeout(ms, TimeUnit.MILLISECONDS)
                }
                val anchor = trustAnchorDer
                if (anchor != null) {
                    val pins = request.pins.map { it.hash }
                    val tm = serverTrustManager(anchor, pins) { ctx.pinMismatch = true }
                    if (tm != null) {
                        val ssl = SSLContext.getInstance("TLS")
                        ssl.init(null, arrayOf<TrustManager>(tm), null)
                        sslSocketFactory(ssl.socketFactory, tm)
                    }
                }
                addNetworkInterceptor { chain ->
                    val response = chain.proceed(chain.request())
                    ctx.lastHopWasRedirect = response.isRedirect
                    response
                }
            }
            .build()
    }

    /**
     * Terminal upload-progress consistency (rule 11): if a body was sent but the flush stopped short
     * of the total, emit the terminal `(total,total)` sample — monotone, and honest (the body WAS
     * handed off on success). A no-op when already terminal.
     */
    private fun terminalUploadProgress(ctx: Ctx, token: ULong) {
        val total = ctx.uploadTotal
        if (total != null && total > 0 && ctx.sentSoFar.get() < total) {
            harness?.reportProgress(token, total.toULong(), total.toULong())
        }
    }

    /**
     * The refused cleartext target when `response` is an un-followed cross-scheme redirect that
     * downgrades an https request to http (rule 4), else `null`. `followSslRedirects(false)` leaves
     * such a redirect un-followed (the 3xx is returned as the response), so the adapter refuses it;
     * a same-scheme redirect never reaches here (OkHttp already followed it).
     */
    private fun insecureDowngradeTarget(response: Response): String? {
        if (!response.isRedirect) return null
        val from = response.request.url
        if (!from.isHttps) return null
        val location = response.header("Location") ?: return null
        val target = from.resolve(location) ?: return null
        return if (target.scheme == "http") target.toString() else null
    }

    /**
     * Stream the response body to `path` without buffering the whole body in memory, finalizing
     * atomically (a same-directory temp file, then a rename into place). Throws [IOException] on any
     * write failure — e.g. the destination's parent directory does not exist (the `Io` control).
     */
    private fun sinkBodyToFile(response: Response, path: String) {
        val dest = File(path)
        val parent = dest.parentFile
        val tmp = File(parent, ".${dest.name}.tmp.${UUID.randomUUID()}")
        val source = response.body?.source() ?: throw IOException("no response body to sink")
        try {
            // `File.sink()` opens the file for writing — it throws if `parent` is missing/unwritable
            // (the `Io` positive control). Okio streams segment-by-segment: the whole body is never
            // materialised in memory.
            tmp.sink().buffer().use { it.writeAll(source) }
            if (dest.exists() && !dest.delete()) throw IOException("cannot replace $path")
            if (!tmp.renameTo(dest)) throw IOException("cannot finalize $path")
        } catch (e: IOException) {
            tmp.delete()
            throw e
        }
    }

    /**
     * Native failure → typed error key, by CAUSE (rule 9 / N6), never by exception text. A recorded
     * caller cancel wins; then a recorded SPKI pin mismatch (rule 10); then a genuine OS permission
     * denial; otherwise the exception TYPE decides. Redirect exhaustion is read from the structural
     * [Ctx.lastHopWasRedirect] the network interceptor records — NOT from the exception message (Q2:
     * the old `TOO_MANY_REDIRECTS_PREFIX` text match is deleted).
     */
    private fun classify(ctx: Ctx, e: IOException): FfiHttpError {
        if (ctx.callerCancelled) return FfiHttpError.Cancelled
        // Rule 10: a pin mismatch on a PASSING chain surfaces the handshake failure as `PinMismatch`,
        // never `Tls` — the trust-vs-pin split, decided by the recorded cause, not the exception shape.
        if (ctx.pinMismatch) return FfiHttpError.PinMismatch
        // A genuine OS permission denial (a missing INTERNET permission, or an EPERM/EACCES sandbox
        // denial) maps to `PermissionDenied` regardless of the IOException OkHttp wrapped it in.
        permissionKeyFor(e)?.let { return it }
        return when (e) {
            // `callTimeout` throws `InterruptedIOException("timeout")`; a socket read/connect idle
            // timeout throws `SocketTimeoutException` (also an `InterruptedIOException`). Both → Timeout.
            is InterruptedIOException -> FfiHttpError.Timeout
            is UnknownHostException -> FfiHttpError.NameResolution
            is SSLException -> FfiHttpError.Tls
            is ConnectException -> FfiHttpError.Connect
            is ProtocolException ->
                if (ctx.lastHopWasRedirect) {
                    // OkHttp's own follow-up cap fired on a redirect chain: the last network response
                    // the interceptor saw was a 3xx, so this `ProtocolException` is redirect exhaustion
                    // (Q2 — read by CAUSE, not by the exception text). The request carries no redirect
                    // limit and the native cap is OkHttp's own, so `0` is the "adapter-internal cap"
                    // sentinel — no conformance row inspects it, only the key (mirrors Apple).
                    FfiHttpError.TooManyRedirects(0u)
                } else {
                    FfiHttpError.Transport(e.message ?: e.javaClass.simpleName)
                }
            else -> FfiHttpError.Transport(e.message ?: e.javaClass.simpleName)
        }
    }

    /** OkHttp `Response.protocol` → the contract version (row 11). The test server speaks HTTP/1.1. */
    private fun mapVersion(protocol: Protocol): FfiHttpVersion =
        when (protocol) {
            Protocol.HTTP_1_0 -> FfiHttpVersion.HTTP1_0
            Protocol.HTTP_1_1 -> FfiHttpVersion.HTTP1_1
            Protocol.HTTP_2, Protocol.H2_PRIOR_KNOWLEDGE -> FfiHttpVersion.HTTP2
            Protocol.QUIC -> FfiHttpVersion.HTTP3
            else -> FfiHttpVersion.HTTP1_1
        }

    /**
     * The redirect hop trace from OkHttp's `priorResponse` chain: every intermediate request URL, in
     * traversal order (first hop first), excluding the final URL (which the response reports as
     * `finalUrl`). The chain links final → prior → …; walking it yields last-hop-first, so it is
     * reversed. Empty when no redirect occurred.
     */
    private fun redirectHops(response: Response): List<String> {
        val hops = ArrayList<String>()
        var prior = response.priorResponse
        while (prior != null) {
            hops.add(prior.request.url.toString())
            prior = prior.priorResponse
        }
        hops.reverse()
        return hops
    }

    /**
     * A request body that reports upload progress (rule 11, N4). It counts the bytes actually flushed
     * to the socket (via a [ForwardingSink]) and forwards the monotone cumulative figure to the parked
     * upload-progress sink through [HttpHarness.reportProgress]. Reporting the real flushed count — not
     * the content length up front — is what avoids the buffer-jump-to-100% failure mode.
     */
    private inner class ProgressBody(
        private val bytes: ByteArray,
        private val contentType: MediaType?,
        private val ctx: Ctx,
    ) : RequestBody() {
        override fun contentType(): MediaType? = contentType

        override fun contentLength(): Long = bytes.size.toLong()

        override fun writeTo(sink: BufferedSink) {
            val total = bytes.size.toLong()
            val counting =
                object : ForwardingSink(sink) {
                    override fun write(source: Buffer, byteCount: Long) {
                        super.write(source, byteCount)
                        val sent = ctx.sentSoFar.addAndGet(byteCount)
                        harness?.reportProgress(ctx.token, sent.toULong(), total.toULong())
                    }
                }
            val buffered = counting.buffer()
            buffered.write(bytes)
            buffered.flush()
        }
    }

    companion object {
        /**
         * The transport read window for the streaming read loop — the granularity of one socket-read
         * hand-off across JNI (okio's natural segment size). This is a **transport I/O** buffer, NOT a
         * contract constraint: the completeness gate counts total bytes regardless of how the body is
         * chunked, so this value has no semantic effect (the analog of URLSession's natural `didReceive
         * data` size and reqwest's `bytes_stream` read — neither of which the adapter picks either).
         */
        private const val READ_WINDOW_BYTES = 8192L

        /**
         * Build the test-tier server-trust manager: a real chain check against `anchorDer` (the sole
         * trust anchor) with the request's declarative SPKI `pins` ANDed on top (rule 10). A chain
         * failure throws (→ `Tls`); a chain that PASSES but has no pin matching the leaf SPKI calls
         * `onPinMismatch` and throws (→ `PinMismatch`). Returns `null` if the anchor cannot be parsed
         * into a trust manager (the adapter then falls back to OkHttp's default trust). Exposed for the
         * N3 unit controls (the trust-vs-pin split and the hostname-less 2-arg landmine).
         */
        @JvmStatic
        fun serverTrustManager(
            anchorDer: ByteArray,
            pins: List<ByteArray>,
            onPinMismatch: () -> Unit,
        ): X509TrustManager? =
            try {
                val cf = CertificateFactory.getInstance("X.509")
                val anchor = cf.generateCertificate(ByteArrayInputStream(anchorDer)) as? X509Certificate
                if (anchor == null) {
                    null
                } else {
                    val ks = KeyStore.getInstance(KeyStore.getDefaultType())
                    ks.load(null, null)
                    ks.setCertificateEntry("bolted-test-anchor", anchor)
                    val tmf = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
                    tmf.init(ks)
                    val delegate = tmf.trustManagers.filterIsInstance<X509TrustManager>().firstOrNull()
                    if (delegate == null) null else PinningTrustManager(delegate, pins, onPinMismatch)
                }
            } catch (e: GeneralSecurityException) {
                null
            } catch (e: IOException) {
                null
            }

        /**
         * SHA-256 over the certificate's SubjectPublicKeyInfo — the honest SPKI pin. `PublicKey.encoded`
         * is the X.509 SubjectPublicKeyInfo DER (the same bytes `x509_parser`'s `public_key().raw` gives
         * the harness server and the Linux verifier), so no hand-rolled ASN.1 walk is needed (unlike
         * Apple, whose `SecCertificate` exposes no SPKI DER). Exposed for the N3 split unit control.
         */
        @JvmStatic
        fun spkiSha256(cert: X509Certificate): ByteArray =
            MessageDigest.getInstance("SHA-256").digest(cert.publicKey.encoded)

        /**
         * Map a genuine OS permission denial to [FfiHttpError.PermissionDenied], else `null`. Walks the
         * throwable's cause chain for a `SecurityException` (a missing INTERNET permission) or an
         * `ErrnoException` with `EPERM`/`EACCES` (a sandbox / local-network denial). A network failure
         * (connection refused, DNS, timeout) is NOT permission-shaped and returns `null` — the adapter
         * maps a genuine denial, it never invents the key. Exposed for the unit control (a live host
         * control is platform-gated on the ART tier; see the M2 notes).
         */
        @JvmStatic
        fun permissionKeyFor(error: Throwable?): FfiHttpError? {
            var cur: Throwable? = error
            while (cur != null) {
                if (cur is SecurityException) return FfiHttpError.PermissionDenied
                if (cur is ErrnoException &&
                    (cur.errno == OsConstants.EPERM || cur.errno == OsConstants.EACCES)
                ) {
                    return FfiHttpError.PermissionDenied
                }
                cur = cur.cause
            }
            return null
        }
    }

    /**
     * The custom [X509TrustManager] enforcing the trust-vs-pin split (rule 10), mirroring the Linux
     * `PinningVerifier` and the Apple trust delegate. The trust decision is DELEGATED to a real
     * anchor-based manager (chain building); the SPKI pin check is ANDed on top of a passing chain.
     *
     * **The hostname-less 2-arg landmine (N3):** [checkServerTrusted] is the two-argument
     * `(chain, authType)` interface method — it receives NO hostname, so it can express the *trust*
     * decision (chain to the anchor) and the *pin* decision (leaf SPKI) but CANNOT bind the certificate
     * to the connection's host. Hostname binding is OkHttp's `HostnameVerifier`'s job (the adapter
     * leaves the default in place); an adapter that did its trust logic here and forgot the verifier
     * would accept a valid-but-wrong-host certificate. The N3 unit test pins this.
     */
    private class PinningTrustManager(
        private val delegate: X509TrustManager,
        private val pins: List<ByteArray>,
        private val onPinMismatch: () -> Unit,
    ) : X509TrustManager {
        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) =
            delegate.checkClientTrusted(chain, authType)

        override fun getAcceptedIssuers(): Array<X509Certificate> = delegate.acceptedIssuers

        override fun checkServerTrusted(chain: Array<out X509Certificate>, authType: String) {
            // 1. The REAL trust decision: chain building against the installed anchor. A failure throws
            //    a `CertificateException` → OkHttp wraps it as `SSLHandshakeException` → `Tls`.
            delegate.checkServerTrusted(chain, authType)
            // 2. The declarative SPKI pins, ANDed on top of a PASSING chain (rule 10). Any one match
            //    satisfies; no match records the cause and throws → the adapter maps it to `PinMismatch`.
            if (pins.isNotEmpty()) {
                val leaf = chain.firstOrNull() ?: throw CertificateException("empty certificate chain")
                val leafSpki = spkiSha256(leaf)
                if (pins.none { it.contentEquals(leafSpki) }) {
                    onPinMismatch()
                    throw CertificateException("SPKI pin mismatch (rule 10)")
                }
            }
        }
    }
}
