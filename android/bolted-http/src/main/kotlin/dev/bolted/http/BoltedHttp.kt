package dev.bolted.http

import android.system.ErrnoException
import android.system.OsConstants
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

        /**
         * Recorded by the custom trust manager when a declarative SPKI pin did not match a leaf on a
         * PASSING chain (rule 10). It makes the resulting `SSLHandshakeException` classify as
         * [FfiHttpError.PinMismatch] rather than [FfiHttpError.Tls] — the trust-vs-pin split, by CAUSE.
         */
        @Volatile var pinMismatch = false
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
                    // Install the test-tier trust anchor + the request's SPKI pins (rule 10). Only
                    // when an anchor is configured — the shipped adapter leaves OkHttp's default trust.
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

    override fun cancel(token: ULong) {
        // Record the caller-cancel CAUSE before cancelling, so the completion is classified as
        // `Cancelled` by cause, not by the exception the cancelled `Call` happens to throw (rule 9).
        val ctx = inFlight[token.toLong()] ?: return
        ctx.callerCancelled = true
        ctx.call?.cancel()
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
     * denial; otherwise the exception TYPE decides. The sole message inspection is the
     * too-many-redirects prefix, which OkHttp exposes only as a `ProtocolException` message.
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
                if (e.message?.startsWith(TOO_MANY_REDIRECTS_PREFIX) == true) {
                    // OkHttp's own follow-up cap fired. The request carries no redirect limit and the
                    // delegate-driven policy is out of scope, so `0` is the "adapter-internal cap"
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
        /** OkHttp signals a redirect-cap breach ONLY via this `ProtocolException` message prefix. */
        private const val TOO_MANY_REDIRECTS_PREFIX = "Too many follow-up requests"

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
