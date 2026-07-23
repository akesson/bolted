package dev.bolted.http.conformance

import dev.bolted.http.ffi.FfiBodyEnd
import dev.bolted.http.ffi.FfiFlowSignal
import dev.bolted.http.ffi.FfiHttpError
import dev.bolted.http.ffi.FfiHttpVersion
import dev.bolted.http.ffi.FfiRequest
import dev.bolted.http.ffi.FfiResponse
import dev.bolted.http.ffi.HttpAdapter
import dev.bolted.http.ffi.HttpHarness

/**
 * The M0 gate's break: an adapter that never performs a request, failing every effect immediately.
 * rule-01 expects a successful GET of `/ok`, so a blanket failure makes it red with the typed reason
 * `ExpectedSuccessGotError { got: Transport }`. Isolated to the test target; the shipped `BoltedHttp`
 * adapter is untouched — the green half of the gate uses the real adapter in the same suite run.
 *
 * M1 reuses it as the watched-red baseline: an always-`Transport` adapter reds every M1-green row
 * whose expectation is NOT `Transport` (i.e. all but rule-08 and key-transport — those two EXPECT a
 * `Transport`, so [AlwaysOkHttp] reds them instead).
 */
class BrokenHttp : HttpAdapter {
    var harness: HttpHarness? = null

    override fun execute(request: FfiRequest) {
        harness?.completeErr(
            request.token,
            FfiHttpError.Transport("deliberately broken adapter (gate red half)"),
        )
    }

    override fun executeStreaming(request: FfiRequest) {
        harness?.finishBody(
            request.token,
            FfiBodyEnd.Failed(FfiHttpError.Transport("deliberately broken adapter (gate red half)")),
        )
    }

    override fun signal(token: ULong, flow: FfiFlowSignal) {}
}

/**
 * The other watched-red break: an adapter that always succeeds with a `200`. It reds exactly the two
 * rows [BrokenHttp] cannot — the ones that EXPECT a `Transport` error: rule-08 (a `200` is read as a
 * hidden retry) and C2/key-transport (a `200` where a `Transport` error was required). Isolated to the
 * test target; the shipped `BoltedHttp` adapter is untouched.
 */
class AlwaysOkHttp : HttpAdapter {
    var harness: HttpHarness? = null

    override fun execute(request: FfiRequest) {
        harness?.completeOk(
            FfiResponse(
                token = request.token,
                status = 200u,
                headers = emptyList(),
                body = "ok".toByteArray(),
                finalUrl = request.url,
                httpVersion = FfiHttpVersion.HTTP1_1,
                hops = emptyList(),
                sinkPath = "",
            ),
        )
    }

    override fun executeStreaming(request: FfiRequest) {
        // Not exercised by the watched-red baseline (it reds only the buffered Transport-expecting
        // rows); a trivial empty-complete keeps it a valid streaming adapter.
        harness?.finishBody(request.token, FfiBodyEnd.Complete(0uL))
    }

    override fun signal(token: ULong, flow: FfiFlowSignal) {}
}
