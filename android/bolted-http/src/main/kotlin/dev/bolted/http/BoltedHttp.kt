package dev.bolted.http

import dev.bolted.http.ffi.FfiHeader
import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.FfiHttpVersion
import dev.bolted.http.ffi.FfiRequest
import dev.bolted.http.ffi.FfiResponse
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.ProtocolException
import java.net.UnknownHostException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import javax.net.ssl.SSLException
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

/**
 * `BoltedHttp` — the hand-written Android HTTP adapter (architecture.md §1, layer 3), OkHttp edition.
 *
 * **M1 status: the base adapter.** On top of M0's walking skeleton (dispatch, one-shot memory-sink
 * completion) M1 adds the full base surface:
 *
 * - **The C2 error taxonomy classified by CAUSE, never by exception text** ([classify]): a caller
 *   cancel is recorded as a per-request cause *before* the OkHttp `Call.cancel()`, so an
 *   `IOException("Canceled")` maps to [FfiHttpError.Cancelled] while a `callTimeout` expiry
 *   (an `InterruptedIOException`) maps to [FfiHttpError.Timeout] — the two are told apart by the
 *   recorded cause + the exception *type*, not by parsing the message (rule 9 / N6). DNS →
 *   [FfiHttpError.NameResolution], refused/unreachable → [FfiHttpError.Connect], any
 *   `SSLException` → [FfiHttpError.Tls], a post-connection failure → [FfiHttpError.Transport].
 * - **The total deadline is OkHttp `callTimeout`** (the default [DeadlineMode.Total]), which bounds
 *   the *whole* call — DNS, connect, TLS, request, response, redirects — as a wall-clock budget, not
 *   a per-idle idle-timer. The `/drip` trickle row proves this: a per-idle timeout keeps resetting on
 *   every dribbled byte and never fires, but `callTimeout` still fires at the deadline. [DeadlineMode]
 *   exists ONLY so the conformance tier can flip the mechanism to [DeadlineMode.PerIdle] (a bare
 *   `readTimeout`) and watch the total-deadline row go red first — one flag, the adapter is not forked.
 * - **Caller cancellation** (rule 9): [cancel] records the cause and cancels the in-flight `Call` from
 *   the (non-call) bridge thread; the resulting completion is [FfiHttpError.Cancelled].
 * - **Real negotiated version** from `Response.protocol` ([mapVersion]) — the M0 `HTTP1_1` placeholder
 *   is gone (row 11).
 * - **The redirect hop trace** from the `priorResponse` chain ([redirectHops]) — first hop first,
 *   excluding the final URL (the redirect-trace row).
 * - **Upload progress** (rule 11, N4): a request-body wrapper ([ProgressBody]) reports the monotone
 *   cumulative bytes actually flushed to the socket via [HttpHarness.reportProgress]; a terminal sample
 *   is emitted on success if the flush stopped short, so the final figure is consistent with completion.
 * - **`retryOnConnectionFailure(false)`** (rule 8): no hidden request-level retry.
 *
 * The M2 syntheses stay out: server-trust anchoring, SPKI pinning (rule 10 / pin-mismatch), the
 * https→http refusal (rule 4 / insecure-redirect), the file sink (row 15 / the `Io` control), and the
 * `PermissionDenied` control. Those rows stay red under M1 and land in M2.
 *
 * Completions arrive on an OkHttp dispatcher thread (off the caller) — the async re-entry the Rust
 * bridge expects. No constraint literals: the total deadline comes from [FfiRequest.deadlineMs]; the
 * adapter invents no timeouts, limits, or keys.
 */
class BoltedHttp(private val deadlineMode: DeadlineMode = DeadlineMode.Total) : HttpAdapter {
    /**
     * How the request's total deadline is enforced. [Total] (the shipped default) uses OkHttp
     * `callTimeout` — a wall-clock budget over the whole call. [PerIdle] uses a bare `readTimeout` and
     * is a **conformance-only** mode: it exists so the tier can prove `callTimeout` is a *total*
     * deadline by watching the `/drip` trickle row go red under a per-idle timer first (which a trickle
     * keeps resetting so it never fires). The adapter is not forked — one construction flag.
     */
    enum class DeadlineMode { Total, PerIdle }

    /**
     * The back-reference the completions re-enter through. Set by the composition root AFTER the
     * harness is built over this adapter (adapter first, harness second, then this assignment).
     */
    var harness: HttpHarness? = null

    // The engine. Contract-shaped: no cookie jar and no response cache are OkHttp defaults; redirects
    // are auto-followed (so the `priorResponse` chain carries the hop trace) and connection-failure
    // retry is OFF (rule 8 — no hidden retry). A per-`Call` deadline is layered on in `execute`.
    private val client =
        OkHttpClient.Builder()
            .followRedirects(true)
            .followSslRedirects(true)
            .retryOnConnectionFailure(false)
            .build()

    /** In-flight requests keyed by the FFI token, so [cancel] reaches the right `Call` and cause. */
    private val inFlight = ConcurrentHashMap<Long, Ctx>()

    /** Per-request mutable state guarded by its own atomics; one instance handles all requests. */
    private class Ctx(val token: ULong) {
        var call: Call? = null

        /** The upload body length when there is a body to send (rule 11), else `null`. */
        var uploadTotal: Long? = null

        /** Cumulative bytes flushed to the socket (rule 11) — the monotone progress source. */
        val sentSoFar = AtomicLong(0)

        /**
         * The recorded caller-cancellation CAUSE. Set by [cancel] before `Call.cancel()` so the
         * completion classifies as [FfiHttpError.Cancelled] independently of the opaque `IOException`
         * a cancelled call throws — never by matching the exception message (rule 9 / N6).
         */
        @Volatile var callerCancelled = false
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

        // The total deadline the request carries (no magic number — it comes from `deadlineMs`).
        // `Total` bounds the WHOLE call incl. redirects (a wall-clock budget); `PerIdle` sets only a
        // read idle-timer (the conformance-only red-watch mechanism — a trickle resets it forever).
        val ms = request.deadlineMs.toLong()
        val perCallClient =
            client
                .newBuilder()
                .apply {
                    when (deadlineMode) {
                        DeadlineMode.Total -> callTimeout(ms, TimeUnit.MILLISECONDS)
                        DeadlineMode.PerIdle -> readTimeout(ms, TimeUnit.MILLISECONDS)
                    }
                }
                .build()

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
                        val body: ByteArray =
                            try {
                                r.body?.bytes() ?: ByteArray(0)
                            } catch (e: IOException) {
                                // A post-headers body-read failure (e.g. `/truncate`'s premature EOF):
                                // classify by cause — never a hidden success (Transport, or Cancelled
                                // if the caller cancelled mid-body).
                                inFlight.remove(token)
                                harness?.completeErr(request.token, classify(ctx, e))
                                return
                            }
                        inFlight.remove(token)

                        // Terminal upload-progress consistency (rule 11): if a body was sent but the
                        // flush stopped short of the total, emit the terminal sample now — monotone,
                        // and honest (the body WAS handed off on success). No-op when already terminal.
                        val total = ctx.uploadTotal
                        if (total != null && total > 0 && ctx.sentSoFar.get() < total) {
                            harness?.reportProgress(request.token, total.toULong(), total.toULong())
                        }

                        val headers = r.headers.map { (n, v) -> FfiHeader(n, v) }
                        harness?.completeOk(
                            FfiResponse(
                                token = request.token,
                                status = r.code.toUShort(),
                                headers = headers,
                                body = body,
                                finalUrl = r.request.url.toString(),
                                // Row 11: the REAL negotiated version, not the M0 placeholder.
                                httpVersion = mapVersion(r.protocol),
                                // The redirect hop trace (first hop first, excluding the final URL).
                                hops = redirectHops(r),
                                // Memory sink only in M1 — the file sink is M2.
                                sinkPath = "",
                            ),
                        )
                    }
                }
            },
        )
    }

    override fun cancel(token: ULong) {
        // Record the caller-cancel CAUSE before cancelling, so the completion is classified as
        // `Cancelled` by cause, not by the exception the cancelled `Call` happens to throw (rule 9).
        val ctx = inFlight[token.toLong()] ?: return
        ctx.callerCancelled = true
        ctx.call?.cancel()
    }

    /**
     * Native failure → typed error key, by CAUSE (rule 9 / N6), never by exception text. A recorded
     * caller cancel wins over the exception shape; otherwise the exception TYPE decides. The sole
     * message inspection is the too-many-redirects prefix, which OkHttp exposes only as a
     * `ProtocolException` message — flagged as friction (there is no typed signal for it).
     */
    private fun classify(ctx: Ctx, e: IOException): FfiHttpError {
        if (ctx.callerCancelled) return FfiHttpError.Cancelled
        return when (e) {
            // `callTimeout` throws `InterruptedIOException("timeout")`; a socket read/connect idle
            // timeout throws `SocketTimeoutException` (also an `InterruptedIOException`). Both → Timeout.
            is InterruptedIOException -> FfiHttpError.Timeout
            is UnknownHostException -> FfiHttpError.NameResolution
            is SSLException -> FfiHttpError.Tls
            is ConnectException -> FfiHttpError.Connect
            is ProtocolException ->
                if (e.message?.startsWith(TOO_MANY_REDIRECTS_PREFIX) == true) {
                    // OkHttp's own follow-up cap fired. The request carries no redirect limit and the
                    // delegate-driven policy is M2, so `0` is the "adapter-internal cap" sentinel — no
                    // conformance row inspects it, only the key (mirrors the Apple adapter).
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
     * the content length up front — is what avoids the buffer-jump-to-100% failure mode: progress
     * tracks the bytes that genuinely crossed to the OS, and the terminal figure equals the body length
     * because that is how many bytes were flushed.
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

    private companion object {
        /** OkHttp signals a redirect-cap breach ONLY via this `ProtocolException` message prefix. */
        const val TOO_MANY_REDIRECTS_PREFIX = "Too many follow-up requests"
    }
}
