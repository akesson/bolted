package dev.bolted.http.conformance

import android.net.http.HttpEngine
import android.net.http.HttpException
import android.net.http.UrlRequest
import android.net.http.UrlResponseInfo
import android.os.Build
import android.util.Log
import androidx.annotation.RequiresApi
import androidx.test.platform.app.InstrumentationRegistry
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.HttpHarness
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Step 26 M3 — N5: `android.net.http.HttpEngine` feature detection (probe-grade, time-boxed).
 *
 * This decides whether the adapter's engine matrix (OkHttp / HttpEngine) is **spike-real or paper** on
 * the gating ART tier. It does NOT build a second engine path — the shipped `BoltedHttp` is OkHttp-only
 * and untouched. The probe answers, in order and cheaply:
 *
 *  1. **Present?** Is `android.net.http.HttpEngine` (added in API 34, the platform Cronet) on the
 *     classpath of the `aosp_atd` android-34 GMD? (`Class.forName`, so an absent class is a clean
 *     ABSENT rather than a `NoClassDefFoundError` at verify time.)
 *  2. **Constructible?** Does `HttpEngine.Builder(context).build()` succeed, or does the aosp_atd image
 *     lack the Cronet mainline module the framework stub delegates to?
 *  3. **h3/h2 negotiable against the conformance TestServer?** The server is a hand-rolled **HTTP/1.1**
 *     listener (rustls for TLS but raw `HTTP/1.1` on the wire, no ALPN, no QUIC) — so h2 (needs ALPN)
 *     and h3 (needs QUIC) are **impossible against it regardless of the engine**. The best a working
 *     HttpEngine can show here is a cleartext `GET /ok` at HTTP/1.1 — which the probe drives (one
 *     rule-01-shaped row through an HttpEngine-backed execute path, androidTest-only) when the engine
 *     is constructible.
 *
 * The verdict sentence lands in `step-26-m3-notes.md`; the observed facts are recorded to logcat
 * (the GMD JUnit XML has no `<system-out>` — F-M1-6).
 */
class HttpEngineProbe {
    private fun record(line: String) {
        Log.i(TAG, line)
        println(line)
    }

    @Test
    fun httpEngineFeatureDetection() {
        val api = Build.VERSION.SDK_INT
        record("N5 API facts: SDK_INT=$api RELEASE=${Build.VERSION.RELEASE} device=${Build.DEVICE} (dev34 GMD, aosp_atd android-34 arm64)")
        // We are on the dev34 GMD; the whole engine-matrix question is an API-34-tier question.
        assertTrue("the N5 probe must run on API 34+ (the dev34 GMD)", api >= 34)

        // (1) Presence via reflection: an absent class is a clean ABSENT, and isolating the direct
        // `android.net.http.*` references in `probeEngine` keeps ART's method verification lazy (no
        // NoClassDefFoundError at load time if the module is stripped).
        val cls = try {
            Class.forName("android.net.http.HttpEngine")
        } catch (t: Throwable) {
            null
        }
        record("N5 presence: android.net.http.HttpEngine ${if (cls != null) "PRESENT" else "ABSENT"} (loader=${cls?.classLoader})")

        if (cls == null) {
            record(
                "N5 VERDICT: PAPER — HttpEngine class absent on the aosp_atd android-34 GMD; " +
                    "the OkHttp/HttpEngine engine matrix is PAPER on this tier.",
            )
            return
        }

        probeEngine()
    }

    /** All direct `android.net.http.*` use lives here, reached only after the presence check passes. */
    @RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
    private fun probeEngine() {
        val ctx = InstrumentationRegistry.getInstrumentation().targetContext

        // (2) Constructibility. On a stripped ATD image the framework stub can be present while the
        // Cronet mainline module it delegates to is not — the build() then throws.
        val engine =
            try {
                HttpEngine.Builder(ctx).build()
            } catch (t: Throwable) {
                record("N5 constructible: NO — ${t.javaClass.name}: ${t.message}")
                record(
                    "N5 VERDICT: PAPER — HttpEngine is present in the framework stubs but NOT constructible " +
                        "on the aosp_atd android-34 GMD (the Cronet mainline module the stub delegates to is " +
                        "absent on this ATD image); the OkHttp/HttpEngine engine matrix is PAPER on this tier.",
                )
                return
            }
        val version = try {
            HttpEngine.getVersionString()
        } catch (t: Throwable) {
            "?"
        }
        record("N5 constructible: YES — HttpEngine version=$version")

        // (3) Drive ONE rule-01-shaped row (GET /ok, 200) through an HttpEngine-backed execute path,
        // over the cleartext loopback base. h2/h3 cannot be shown against this TestServer (raw HTTP/1.1,
        // no ALPN, no QUIC), so we record what protocol HttpEngine negotiates and stop the h3/h2 leg.
        val harness = HttpHarness(BoltedHttp())
        try {
            val info = harness.startServer()
            val httpResult = getViaHttpEngine(engine, "${info.httpBase}/ok")
            record(
                "N5 HttpEngine GET ${info.httpBase}/ok (cleartext): status=${httpResult.status} " +
                    "negotiatedProtocol='${httpResult.protocol}' bytes=${httpResult.bytes} " +
                    "error=${httpResult.error}",
            )
            val oneRowDriven = httpResult.status == 200 && httpResult.error == null

            // h3/h2 leg: attempt the good HTTPS base. Cronet validates the chain against the system
            // trust store; the TestServer's self-signed anchor is not installed there, and Cronet has
            // no cheap public API to add a test anchor (unlike OkHttp's sslSocketFactory). AND the
            // server speaks no ALPN/QUIC, so even a trusted engine could only reach HTTP/1.1. So the
            // h3/h2 negotiation is not cheaply testable here — attempt it once to record the barrier.
            val httpsResult = getViaHttpEngine(engine, "${info.httpsBase}/ok")
            record(
                "N5 HttpEngine GET ${info.httpsBase}/ok (self-signed TLS): status=${httpsResult.status} " +
                    "negotiatedProtocol='${httpsResult.protocol}' error=${httpsResult.error} " +
                    "(expected to fail — Cronet won't trust the self-signed loopback anchor; and the server " +
                    "speaks only HTTP/1.1, so h2/h3 is unreachable against it regardless)",
            )

            if (oneRowDriven) {
                record(
                    "N5 VERDICT: SPIKE-REAL — HttpEngine is present, constructible, and drove a live " +
                        "GET /ok (200) end-to-end on the dev34 GMD; BUT h2/h3 could NOT be negotiated against " +
                        "the conformance TestServer (it is a raw HTTP/1.1 listener with no ALPN/QUIC) and " +
                        "Cronet has no cheap anchor-install for the self-signed loopback, so an h3 conformance " +
                        "row is NOT cheap here — the engine matrix is real on ART but the h3 leg needs an " +
                        "h3-capable test server (not this one). Second engine path stays out of scope (step doc).",
                )
            } else {
                record(
                    "N5 VERDICT: PAPER-ISH — HttpEngine is present and constructible but did not complete a " +
                        "cleartext GET /ok on this tier (status=${httpResult.status} error=${httpResult.error}); " +
                        "the engine matrix is not demonstrably real on the aosp_atd GMD. Recorded, not gated.",
                )
            }
        } finally {
            harness.stopServer()
            harness.close()
            engine.shutdown()
        }
    }

    private data class EngineResult(
        val status: Int,
        val protocol: String,
        val bytes: Int,
        val error: String?,
    )

    /** One-shot GET through HttpEngine, draining the body; blocks up to [TIMEOUT_S]. */
    @RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
    private fun getViaHttpEngine(engine: HttpEngine, url: String): EngineResult {
        val executor = Executors.newSingleThreadExecutor()
        val latch = CountDownLatch(1)
        // A single holder array so the callback (on the executor thread) publishes to the caller after
        // the latch; the CountDownLatch await/countDown provides the happens-before edge.
        val slot = arrayOfNulls<Any>(4) // [status, protocol, bytes, error]
        val callback = object : UrlRequest.Callback {
            override fun onRedirectReceived(request: UrlRequest, info: UrlResponseInfo, newLocationUrl: String) {
                request.followRedirect()
            }

            override fun onResponseStarted(request: UrlRequest, info: UrlResponseInfo) {
                slot[0] = info.httpStatusCode
                slot[1] = info.negotiatedProtocol ?: ""
                request.read(ByteBuffer.allocateDirect(READ_BUF))
            }

            override fun onReadCompleted(request: UrlRequest, info: UrlResponseInfo, byteBuffer: ByteBuffer) {
                slot[2] = (slot[2] as? Int ?: 0) + byteBuffer.position()
                byteBuffer.clear()
                request.read(byteBuffer)
            }

            override fun onSucceeded(request: UrlRequest, info: UrlResponseInfo) {
                latch.countDown()
            }

            override fun onFailed(request: UrlRequest, info: UrlResponseInfo?, e: HttpException) {
                slot[3] = "${e.javaClass.simpleName}: ${e.message}"
                latch.countDown()
            }

            override fun onCanceled(request: UrlRequest, info: UrlResponseInfo?) {
                slot[3] = "canceled"
                latch.countDown()
            }
        }
        return try {
            engine.newUrlRequestBuilder(url, executor, callback).build().start()
            val finished = latch.await(TIMEOUT_S, TimeUnit.SECONDS)
            EngineResult(
                status = (slot[0] as? Int) ?: -1,
                protocol = (slot[1] as? String) ?: "",
                bytes = (slot[2] as? Int) ?: 0,
                error = (slot[3] as? String) ?: if (finished) null else "timeout after ${TIMEOUT_S}s",
            )
        } catch (t: Throwable) {
            EngineResult(-1, "", 0, "${t.javaClass.simpleName}: ${t.message}")
        } finally {
            executor.shutdownNow()
        }
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
        const val TIMEOUT_S = 20L
        const val READ_BUF = 32 * 1024
    }
}
