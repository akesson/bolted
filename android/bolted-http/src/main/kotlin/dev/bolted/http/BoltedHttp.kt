package dev.bolted.http

import dev.bolted.http.ffi.FfiHeader
import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.FfiHttpVersion
import dev.bolted.http.ffi.FfiRequest
import dev.bolted.http.ffi.FfiResponse
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response

/**
 * `BoltedHttp` — the hand-written Android HTTP adapter (architecture.md §1, layer 3), OkHttp edition.
 *
 * **M0 status: walking skeleton.** It honours exactly ONE C1 row (rule 1 — GET `/ok`, deterministic
 * `200`): dispatch an OkHttp `Call.enqueue`, on success re-enter [`HttpHarness.completeOk`] with the
 * status/headers/body as a `Memory` outcome; on any failure re-enter [`HttpHarness.completeErr`] with
 * a blanket [`FfiHttpError.Transport`]. The full C2 taxonomy, total-deadline synthesis semantics,
 * pinning, redirect trace, file sink, real negotiated version, and upload progress are M1/M2 — every
 * other conformance row is expected to report RED against this skeleton. The point of M0 is that the
 * JNI bridge can carry a green AND be proven able to carry a red.
 *
 * The completion arrives on an OkHttp dispatcher thread (off the caller) — the async re-entry the
 * Rust bridge expects. No constraint literals: the total deadline comes from the request
 * ([`FfiRequest.deadlineMs`]) via OkHttp `callTimeout`; the adapter invents no timeouts or limits.
 */
class BoltedHttp : HttpAdapter {
    /**
     * The back-reference the completions re-enter through. Set by the composition root AFTER the
     * harness is built over this adapter (adapter first, harness second, then this assignment).
     */
    var harness: HttpHarness? = null

    // The engine. OkHttp defaults are already contract-shaped for the skeleton: no cookie jar
    // (`CookieJar.NO_COOKIES`) and no response cache. A per-Call `callTimeout` carries the request's
    // total deadline, so the shared client needs no default timeout.
    private val client = OkHttpClient.Builder().build()

    /** In-flight calls, keyed by the FFI token, so [`cancel`] can reach the right `Call` (rule 9). */
    private val inFlight = ConcurrentHashMap<Long, Call>()

    override fun execute(request: FfiRequest) {
        val token = request.token.toLong()

        val builder = Request.Builder().url(request.url)
        val bodyBytes = request.body
        val requestBody: RequestBody? =
            if (bodyBytes.isNotEmpty()) {
                val contentType =
                    request.headers
                        .firstOrNull { it.name.equals("content-type", ignoreCase = true) }
                        ?.value
                        ?.toMediaTypeOrNull()
                bodyBytes.toRequestBody(contentType)
            } else {
                null
            }
        builder.method(request.method, requestBody)
        for (header in request.headers) {
            builder.addHeader(header.name, header.value)
        }

        // The total deadline the request carries (no magic number). `callTimeout` bounds the whole
        // call — DNS, connect, TLS, request, response, redirects — matching the contract's TOTAL
        // deadline semantics (verified honest for M1; here it just carries the request's figure).
        val perCallClient =
            client
                .newBuilder()
                .callTimeout(request.deadlineMs.toLong(), TimeUnit.MILLISECONDS)
                .build()

        val call = perCallClient.newCall(builder.build())
        inFlight[token] = call
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    inFlight.remove(token)
                    // M0 skeleton: every native failure is a blanket Transport. The typed C2
                    // taxonomy (Timeout / Cancelled / Connect / Tls / …) is M1's mapping.
                    harness?.completeErr(
                        request.token,
                        FfiHttpError.Transport(e.message ?: e.javaClass.simpleName),
                    )
                }

                override fun onResponse(call: Call, response: Response) {
                    inFlight.remove(token)
                    response.use { r ->
                        val body = r.body?.bytes() ?: ByteArray(0)
                        val headers = r.headers.map { (n, v) -> FfiHeader(n, v) }
                        harness?.completeOk(
                            FfiResponse(
                                token = request.token,
                                status = r.code.toUShort(),
                                headers = headers,
                                body = body,
                                finalUrl = r.request.url.toString(),
                                // M0 placeholder: the real negotiated version (r.protocol) is M1.
                                httpVersion = FfiHttpVersion.HTTP1_1,
                                hops = emptyList(),
                                sinkPath = "",
                            ),
                        )
                    }
                }
            },
        )
    }

    override fun cancel(token: ULong) {
        // Forward a caller cancellation to the in-flight Call (rule 9). Its callback fires an
        // IOException the skeleton maps to Transport; the typed Cancelled key is M1.
        inFlight.remove(token.toLong())?.cancel()
    }
}
