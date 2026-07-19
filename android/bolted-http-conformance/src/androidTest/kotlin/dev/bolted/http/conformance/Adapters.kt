package dev.bolted.http.conformance

import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.FfiRequest
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness

/**
 * The M0 gate's break: an adapter that never performs a request, failing every effect immediately.
 * rule-01 expects a successful GET of `/ok`, so a blanket failure makes it red with the typed reason
 * `ExpectedSuccessGotError { got: Transport }`. Isolated to the test target; the shipped `BoltedHttp`
 * adapter is untouched — the green half of the gate uses the real adapter in the same suite run.
 */
class BrokenHttp : HttpAdapter {
    var harness: HttpHarness? = null

    override fun execute(request: FfiRequest) {
        harness?.completeErr(
            request.token,
            FfiHttpError.Transport("deliberately broken adapter (gate red half)"),
        )
    }

    override fun cancel(token: ULong) {}
}
