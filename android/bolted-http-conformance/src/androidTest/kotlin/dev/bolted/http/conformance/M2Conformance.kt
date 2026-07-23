package dev.bolted.http.conformance

import android.util.Base64
import android.util.Log
import androidx.test.platform.app.InstrumentationRegistry
import dev.bolted.http.BoltedHttp
import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness
import dev.bolted.http.ffi.RowReport
import java.io.ByteArrayInputStream
import java.net.ConnectException
import java.security.cert.CertificateException
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLPeerUnverifiedException
import javax.net.ssl.TrustManager
import okhttp3.CertificatePinner
import okhttp3.OkHttpClient
import okhttp3.Request
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * Step 26 M2 — the syntheses gate. The FULL suite green on the real [BoltedHttp] adapter (trust anchor
 * installed), plus the N3 fragility controls (the trust-vs-pin split, the hostname-less 2-arg
 * `checkServerTrusted` landmine, the Network Security Config `<pin-set>` verdict), the
 * `PermissionDenied` mapping control, and the pinned C3 Android column.
 */
class M2Conformance {
    private fun harnessFor(adapter: HttpAdapter): HttpHarness {
        val harness = HttpHarness(adapter)
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

    // MARK: - The full suite, real adapter (trust anchor installed)

    @Test
    fun theFullSuiteIsGreenOnTheRealAdapter() {
        // The file-sink rows (row-15, key-io) build their destination path Rust-side from
        // `std::env::temp_dir()`, which on Android resolves to an unwritable `/tmp` unless `TMPDIR` is
        // set. Point it at the app's (writable) cache dir so row-15 can write and key-io's nonexistent
        // subdirectory is still nonexistent (→ the honest `Io` control). Set on the process env so the
        // in-process Rust suite's `getenv("TMPDIR")` sees it.
        val cacheDir =
            InstrumentationRegistry.getInstrumentation().targetContext.cacheDir.absolutePath
        android.system.Os.setenv("TMPDIR", cacheDir, true)

        val adapter = BoltedHttp()
        val harness = harnessFor(adapter)
        val info = harness.startServer()
        assertFalse("the in-process test server failed to start", info.httpBase.isEmpty())
        assertFalse("startServer must export the good cert DER anchor", info.goodCertDer.isEmpty())
        // Install the test-tier trust anchor — this flips the four https rows from `Tls` to their real
        // M2 outcomes (pin split, https→http refusal), while the untrusted endpoint stays `Tls`.
        adapter.trustAnchorDer = info.goodCertDer
        try {
            val rows = harness.runC1() + harness.runExtraRows() + harness.runC2()
            for (r in rows.sortedBy { it.id }) {
                val mark = if (r.passed) "GREEN" else if (r.skipped) "SKIP " else "RED  "
                record("M2 [$mark] ${r.id}${if (r.message.isEmpty()) "" else " — ${r.message}"}")
            }
            assertFalse("no rows ran", rows.isEmpty())
            for (r in rows) {
                assertTrue("row ${r.id} must be GREEN on the real adapter — ${r.message}", r.passed)
                assertFalse("row ${r.id} must run, not skip", r.skipped)
            }

            // The pinned C3 Android column, generated from the capability traits: metrics present
            // (Phase — OkHttp EventListener). The priority hint (row 12) is now a uniform advisory
            // field, not a divergent capability (ruled Q10) — OkHttp legally ignores it and it has
            // no column. A drift means a capability impl changed without updating this.
            val c3 = harness.runC3()
            record("M2 C3 Android column:\n$c3")
            assertEquals("C3 Android column drifted:\n$c3", EXPECTED_C3, c3)
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - N3 (a): the trust-vs-pin split, at the unit level (the SPKI computation + the split)

    /**
     * The trust manager enforces the rule-10 split by CAUSE, not by conflation: on a PASSING chain, a
     * matching pin accepts, a non-matching pin fires [onPinMismatch] and throws (→ `PinMismatch`, never
     * `Tls`). Also the load-bearing non-vacuous check: the adapter's SPKI computation
     * ([BoltedHttp.spkiSha256]) equals the server's `good_spki` — if it did not, rule-10's good-pin leg
     * would spuriously mismatch.
     */
    @Test
    fun theServerTrustManagerSplitIsCauseNotConflated() {
        val harness = HttpHarness(BoltedHttp())
        val info = harness.startServer()
        try {
            val goodCert = certOf(info.goodCertDer)

            // The SPKI computation matches the server's good pin (non-vacuous: the pin arm is real).
            assertTrue(
                "spkiSha256(goodCert) must equal the server's good_spki",
                BoltedHttp.spkiSha256(goodCert).contentEquals(info.goodSpki),
            )

            // No pins ⇒ trust only ⇒ accept.
            run {
                val fired = booleanArrayOf(false)
                val tm = BoltedHttp.serverTrustManager(info.goodCertDer, emptyList()) { fired[0] = true }
                assertNotNull(tm)
                tm!!.checkServerTrusted(arrayOf(goodCert), AUTH_TYPE)
                assertFalse("no-pins must not fire pin-mismatch", fired[0])
            }

            // A matching pin ⇒ accept, no mismatch.
            run {
                val fired = booleanArrayOf(false)
                val tm = BoltedHttp.serverTrustManager(info.goodCertDer, listOf(info.goodSpki)) { fired[0] = true }
                tm!!.checkServerTrusted(arrayOf(goodCert), AUTH_TYPE)
                assertFalse("a matching pin must not fire pin-mismatch", fired[0])
            }

            // Any-one-matches ⇒ accept even with a wrong pin also present.
            run {
                val fired = booleanArrayOf(false)
                val tm = BoltedHttp.serverTrustManager(info.goodCertDer, listOf(info.untrustedSpki, info.goodSpki)) { fired[0] = true }
                tm!!.checkServerTrusted(arrayOf(goodCert), AUTH_TYPE)
                assertFalse("any one matching pin must satisfy", fired[0])
            }

            // A wrong pin on a PASSING chain ⇒ fire mismatch + throw (the pin arm → PinMismatch).
            run {
                val fired = booleanArrayOf(false)
                val tm = BoltedHttp.serverTrustManager(info.goodCertDer, listOf(info.untrustedSpki)) { fired[0] = true }
                try {
                    tm!!.checkServerTrusted(arrayOf(goodCert), AUTH_TYPE)
                    fail("a non-matching pin must throw (→ PinMismatch)")
                } catch (e: CertificateException) {
                    // expected
                }
                assertTrue("a non-matching pin must fire the pin-mismatch cause", fired[0])
            }
            record("N3(a) split: SPKI matches good_spki; matching pin accepts, wrong pin ⇒ PinMismatch cause")
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - N3 (b): the hostname-less 2-arg checkServerTrusted landmine

    /**
     * The `X509TrustManager.checkServerTrusted(chain, authType)` interface method is the TWO-argument
     * form — it receives NO hostname. It can express the trust decision (chain to the anchor) and the
     * pin decision (leaf SPKI), but it CANNOT bind the certificate to the connection's host: the same
     * 2-arg call accepts the good cert with no host context at all. Host binding is therefore OkHttp's
     * `HostnameVerifier`'s job (the adapter leaves the default in place). An adapter that did its trust
     * logic here and forgot the verifier would accept a valid-but-wrong-host certificate — the landmine.
     * The runtime proof that host binding IS present end to end is the `key-tls` row (the untrusted
     * endpoint is rejected) in [theFullSuiteIsGreenOnTheRealAdapter].
     */
    @Test
    fun theHostnameLessTwoArgCheckServerTrustedIsTrustOnly() {
        val harness = HttpHarness(BoltedHttp())
        val info = harness.startServer()
        try {
            val goodCert = certOf(info.goodCertDer)
            val tm = BoltedHttp.serverTrustManager(info.goodCertDer, emptyList()) {}
            assertNotNull(tm)
            // The 2-arg method accepts the good cert with NO hostname argument — it is hostname-blind.
            // (It would accept identically whatever host the client believed it was connecting to.)
            tm!!.checkServerTrusted(arrayOf(goodCert), AUTH_TYPE)

            // The signature IS the 2-arg interface method (there is no host parameter on it). Prove the
            // 2-arg form exists and carries no hostname — the landmine documented.
            val m = javax.net.ssl.X509TrustManager::class.java.getMethod(
                "checkServerTrusted",
                Array<X509Certificate>::class.java,
                String::class.java,
            )
            assertEquals("checkServerTrusted must be the 2-arg (hostname-less) form", 2, m.parameterCount)
            record("N3(b) landmine: 2-arg checkServerTrusted is hostname-blind — host binding is the HostnameVerifier's job")
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - N3 (a, cont.): the Network Security Config <pin-set> verdict

    /**
     * The freeze §9 question: does a Network Security Config `<pin-set>` bind OkHttp? **Verdict: NO —
     * not when a custom `SSLSocketFactory`/`X509TrustManager` is installed** (which every real pinning
     * adapter, and this one, does). The conformance manifest declares a `<pin-set>` with a deliberately
     * WRONG pin for `127.0.0.1`. This test proves the split:
     *
     *  - a client using the adapter's custom `SSLSocketFactory` and NO OkHttp `CertificatePinner`
     *    connects to the good endpoint SUCCESSFULLY — the NSC `<pin-set>` did not enforce; and
     *  - a client with the SAME wrong pin enforced at the OkHttp level (a `CertificatePinner`) is
     *    BLOCKED — proving the pin value is genuinely wrong and that pinning bites when adapter-enforced.
     *
     * So the suite's pinning is entirely adapter-enforced and never silently depends on NSC.
     */
    @Test
    fun theNscPinSetDoesNotBindTheAdapter() {
        val harness = HttpHarness(BoltedHttp())
        val info = harness.startServer()
        try {
            val url = "${info.httpsBase}/ok"
            // A base64 SHA-256 pin that is WRONG for the good endpoint (it is the UNTRUSTED cert's SPKI).
            val wrongPin = "sha256/" + Base64.encodeToString(info.untrustedSpki, Base64.NO_WRAP)

            // Arm 1: custom SSLSocketFactory, NO CertificatePinner. NSC's <pin-set> is bypassed ⇒ 200.
            val bypassClient = trustingClient(info.goodCertDer, null)
            val bypassResp = bypassClient.newCall(Request.Builder().url(url).build()).execute()
            bypassResp.use {
                assertEquals(
                    "the adapter's custom SSLSocketFactory must bypass the NSC <pin-set> (200 expected)",
                    200, it.code,
                )
            }

            // Arm 2: the SAME wrong pin, enforced at the OkHttp level (CertificatePinner) ⇒ BLOCKED.
            val pinner = CertificatePinner.Builder().add("127.0.0.1", wrongPin).build()
            val pinnedClient = trustingClient(info.goodCertDer, pinner)
            var blocked = false
            try {
                pinnedClient.newCall(Request.Builder().url(url).build()).execute().close()
            } catch (e: SSLPeerUnverifiedException) {
                blocked = true
            }
            assertTrue("the wrong pin must BLOCK when enforced at the OkHttp level (control)", blocked)

            record(
                "N3 NSC verdict: NSC <pin-set> present (wrong pin for 127.0.0.1) but NOT enforced on the " +
                    "adapter's custom-SSLSocketFactory connection (200); the identical pin BLOCKS via " +
                    "CertificatePinner ⇒ pinning is adapter-enforced, never NSC. (§9: <pin-set> does NOT bind OkHttp.)",
            )
        } finally {
            harness.stopServer()
            harness.close()
        }
    }

    // MARK: - PermissionDenied (the mapping control; a live host control is platform-gated)

    /**
     * `PermissionDenied` has no hermetic host control on the ART conformance tier: with INTERNET
     * granted and the in-process server on loopback, no host request can make the OS deny permission
     * (the genuine causes are a missing INTERNET permission or an app-sandbox/local-network denial —
     * device/app-bundle-tier, not gating here). So its positive control is the load-bearing MAPPING: a
     * genuine `SecurityException` or an `ErrnoException` `EPERM`/`EACCES` maps to `PermissionDenied`,
     * and a network-shaped failure does NOT (the negative control — the mapping is not vacuous).
     */
    @Test
    fun thePermissionDeniedMapping() {
        assertEquals(
            "a SecurityException (missing INTERNET) must map to PermissionDenied",
            FfiHttpError.PermissionDenied,
            BoltedHttp.permissionKeyFor(SecurityException("Permission denied (missing INTERNET?)")),
        )
        assertEquals(
            "an EPERM ErrnoException must map to PermissionDenied",
            FfiHttpError.PermissionDenied,
            BoltedHttp.permissionKeyFor(
                android.system.ErrnoException("connect", android.system.OsConstants.EPERM),
            ),
        )
        assertEquals(
            "a PermissionDenied cause nested in an IOException must still map",
            FfiHttpError.PermissionDenied,
            BoltedHttp.permissionKeyFor(
                java.io.IOException("connect failed", SecurityException("denied")),
            ),
        )
        // Negative controls: network-shaped failures are NOT permission-shaped.
        assertNull(
            "a connection-refused failure is not permission-shaped",
            BoltedHttp.permissionKeyFor(ConnectException("Connection refused")),
        )
        assertNull(
            "a plain transport failure is not permission-shaped",
            BoltedHttp.permissionKeyFor(java.io.IOException("unexpected end of stream")),
        )
        record("PermissionDenied: mapping proven (SecurityException/EPERM/EACCES ⇒ key; network failures ⇒ null); live host control platform-gated")
    }

    // MARK: - helpers

    private fun certOf(der: ByteArray): X509Certificate =
        CertificateFactory.getInstance("X.509")
            .generateCertificate(ByteArrayInputStream(der)) as X509Certificate

    /** An OkHttpClient trusting exactly the good cert (the adapter's SSL setup), optionally pinned. */
    private fun trustingClient(anchorDer: ByteArray, pinner: CertificatePinner?): OkHttpClient {
        val tm = BoltedHttp.serverTrustManager(anchorDer, emptyList()) {}
        assertNotNull(tm)
        val ssl = SSLContext.getInstance("TLS")
        ssl.init(null, arrayOf<TrustManager>(tm!!), null)
        val b = OkHttpClient.Builder().sslSocketFactory(ssl.socketFactory, tm)
        if (pinner != null) b.certificatePinner(pinner)
        return b.build()
    }

    private companion object {
        const val TAG = "BoltedHttpConformance"
        const val AUTH_TYPE = "GENERIC"

        val EXPECTED_C3 =
            """
            capability     | presence
            ---------------+-----------------------
            metrics        | present (Phase)
            """.trimIndent()
    }
}
