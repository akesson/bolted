import Foundation
import Security

/// `BoltedHttp` — the hand-written Apple HTTP adapter (architecture.md §1, layer 3).
///
/// **M1 status: the real adapter.** It implements the `HttpAdapter` callback trait over a
/// delegate-driven `URLSession`, carrying the C1/C2 behaviour the host tier can reach: total-deadline
/// synthesis, caller cancellation, the C2 error-key mapping, upload progress, anchor-based server
/// trust, and the real negotiated HTTP version. The A2/A4 syntheses (file sink, SPKI pinning, hop
/// trace, https→http refusal, 304, `PermissionDenied`) are milestone M2.
///
/// It lives in the SAME SwiftPM target as the generated bindings (the `bundled` packaging layout),
/// so the generated `FfiRequest` / `FfiResponse` / `HttpHarness` types need no `import`.
///
/// Composition-root wiring (the dance the XCTest performs): the adapter is built first, the harness
/// second (it takes the adapter), then the harness is set back on the adapter so the completion can
/// re-enter, and finally — once `startServer()` has handed over `ServerInfo.goodCertDer` — the trust
/// anchor is installed. The `harness` back-reference is `weak`: the Rust side owns the adapter across
/// the FFI, so a strong reference here would cycle.
public final class BoltedHttp: NSObject, HttpAdapter, URLSessionDataDelegate {
    /// Weak by design: the harness owns this adapter through the FFI bridge.
    public weak var harness: HttpHarness?

    /// The good endpoint's DER-encoded certificate, installed as the sole trust anchor for
    /// server-trust evaluation (anchor-only for M1 — the pinning SPLIT is M2). Set by the
    /// composition root from `ServerInfo.goodCertDer` after `startServer()`. `nil` ⇒ default system
    /// trust evaluation, which rejects the self-signed test certificates (so the untrusted endpoint
    /// stays a `Tls` positive control).
    public var trustAnchorDER: Data?

    /// A total-deadline timer runs off this queue (concurrent — one short-lived source per request).
    private static let timerQueue = DispatchQueue(label: "bolted.http.deadline", attributes: .concurrent)

    private var session: URLSession!
    /// Guards `contexts`, and every mutation of a `RequestContext` after it is registered.
    private let lock = NSLock()
    /// In-flight requests, keyed by the FFI token (the delegate re-derives the token from
    /// `URLSessionTask.taskDescription`).
    private var contexts: [UInt64: RequestContext] = [:]

    public override init() {
        super.init()
        // Contract defaults: cookie-less, cache-less (architecture.md §2).
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        // A serial delegate queue: within the conformance driver at most one request is in flight,
        // and serialising the delegate callbacks keeps progress-before-completion ordering (rule 11).
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        self.session = URLSession(configuration: config, delegate: self, delegateQueue: queue)
    }

    // MARK: - HttpAdapter

    public func execute(request: FfiRequest) {
        guard let url = URL(string: request.url) else {
            harness?.completeErr(
                token: request.token,
                error: .transport(message: "invalid url: \(request.url)")
            )
            return
        }

        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = request.method
        // Deliberately NOT deriving `timeoutInterval` from the deadline: URLSession's
        // `timeoutInterval` is a PER-IDLE timeout and must not stand in for the contract's TOTAL
        // deadline (the A3 hazard). The total budget is enforced by the synthesized timer below,
        // spanning the whole request including any redirects.
        for header in request.headers {
            urlRequest.setValue(header.value, forHTTPHeaderField: header.name)
        }
        let uploadTotal: UInt64?
        if !request.body.isEmpty {
            urlRequest.httpBody = request.body
            uploadTotal = UInt64(request.body.count)
        } else {
            uploadTotal = nil
        }

        let task = session.dataTask(with: urlRequest)
        task.taskDescription = String(request.token)

        let ctx = RequestContext(token: request.token, requestURL: request.url)
        ctx.task = task
        ctx.uploadTotal = uploadTotal

        lock.lock()
        contexts[request.token] = ctx
        lock.unlock()

        // Total-deadline synthesis (rule 3): a single timer over the whole request. On expiry we
        // cancel the task and record the cause as `.deadline`, so the resulting URLError.cancelled is
        // classified as `Timeout` — distinct from a caller cancel (rule 2), by CAUSE not error shape.
        if request.deadlineMs > 0 {
            let seconds = Double(request.deadlineMs) / 1000.0
            let timer = DispatchSource.makeTimerSource(queue: Self.timerQueue)
            timer.schedule(deadline: .now() + seconds)
            timer.setEventHandler { [weak self] in self?.deadlineFired(token: request.token) }
            ctx.deadlineTimer = timer
            timer.resume()
        }
        task.resume()
    }

    /// Forward a caller cancellation (rule 9): cancel the task; its completion becomes
    /// `URLError.cancelled`, classified `Cancelled` because the cause is a caller cancel.
    public func cancel(token: UInt64) {
        lock.lock()
        guard let ctx = contexts[token], !ctx.finished else {
            lock.unlock()
            return
        }
        if ctx.termination == .none { ctx.termination = .callerCancel }
        let task = ctx.task
        lock.unlock()
        task?.cancel()
    }

    // MARK: - Deadline

    private func deadlineFired(token: UInt64) {
        lock.lock()
        guard let ctx = contexts[token], !ctx.finished, ctx.termination == .none else {
            lock.unlock()
            return
        }
        ctx.termination = .deadline
        let task = ctx.task
        lock.unlock()
        task?.cancel()
    }

    // MARK: - URLSessionDelegate (server trust)

    public func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let serverTrust = challenge.protectionSpace.serverTrust else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        guard let der = trustAnchorDER,
              let anchor = SecCertificateCreateWithData(nil, der as CFData) else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        // Anchor-only evaluation against the good cert: a real chain + hostname check, but trusting
        // exactly our anchor. The good endpoint chains to it (self-signed leaf == anchor); the
        // untrusted endpoint does not, so it falls through to default handling → a real TLS rejection.
        SecTrustSetAnchorCertificates(serverTrust, [anchor] as CFArray)
        SecTrustSetAnchorCertificatesOnly(serverTrust, true)
        if SecTrustEvaluateWithError(serverTrust, nil) {
            completionHandler(.useCredential, URLCredential(trust: serverTrust))
        } else {
            completionHandler(.performDefaultHandling, nil)
        }
    }

    // MARK: - URLSessionDataDelegate

    public func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        lock.lock()
        if let ctx = context(for: dataTask), let http = response as? HTTPURLResponse {
            ctx.response = http
        }
        lock.unlock()
        completionHandler(.allow)
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        lock.lock()
        context(for: dataTask)?.buffer.append(data)
        lock.unlock()
    }

    // MARK: - URLSessionTaskDelegate

    public func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didSendBodyData bytesSent: Int64,
        totalBytesSent: Int64,
        totalBytesExpectedToSend: Int64
    ) {
        lock.lock()
        guard let ctx = context(for: task) else {
            lock.unlock()
            return
        }
        let sent = UInt64(max(0, totalBytesSent))
        ctx.sentSoFar = sent
        let total: UInt64? = totalBytesExpectedToSend > 0 ? UInt64(totalBytesExpectedToSend) : ctx.uploadTotal
        let token = ctx.token
        lock.unlock()
        // Rule 11: OS-fed, monotone (`totalBytesSent` is cumulative), forwarded to the parked sink.
        harness?.reportProgress(token: token, sent: sent, total: total)
    }

    public func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didFinishCollecting metrics: URLSessionTaskMetrics
    ) {
        let proto = metrics.transactionMetrics.last?.networkProtocolName
        lock.lock()
        context(for: task)?.protocolName = proto
        lock.unlock()
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        lock.lock()
        guard let desc = task.taskDescription, let token = UInt64(desc),
              let ctx = contexts[token], !ctx.finished else {
            lock.unlock()
            return
        }
        ctx.finished = true
        contexts[token] = nil
        ctx.deadlineTimer?.cancel()
        let termination = ctx.termination
        let response = ctx.response
        let buffer = ctx.buffer
        let protocolName = ctx.protocolName
        let uploadTotal = ctx.uploadTotal
        let sentSoFar = ctx.sentSoFar
        let requestURL = ctx.requestURL
        lock.unlock()

        if let error {
            harness?.completeErr(token: token, error: Self.mapError(error, termination: termination))
            return
        }
        guard let http = response else {
            harness?.completeErr(token: token, error: .transport(message: "no HTTP response"))
            return
        }

        // Terminal upload-progress consistency (rule 11): on success, if the body was fully sent but
        // the OS-fed `didSendBodyData` never reported the final byte count, emit the terminal sample
        // now — monotone, and honest (the body WAS handed off on success). A no-op when progress
        // already reached the total.
        if let total = uploadTotal, total > 0, sentSoFar < total {
            harness?.reportProgress(token: token, sent: total, total: total)
        }

        var headers: [FfiHeader] = []
        for (name, value) in http.allHeaderFields {
            headers.append(FfiHeader(name: String(describing: name), value: String(describing: value)))
        }
        harness?.completeOk(
            response: FfiResponse(
                token: token,
                status: UInt16(http.statusCode),
                headers: headers,
                body: buffer,
                finalUrl: http.url?.absoluteString ?? requestURL,
                httpVersion: Self.mapVersion(protocolName)
            )
        )
    }

    // MARK: - Mapping

    /// Native failure → typed error key (rule 2 — by CAUSE, not exception shape). Covers the full C2
    /// taxonomy the URLSession host tier can reach; the pin / insecure-redirect / permission / io keys
    /// are the M2 syntheses.
    static func mapError(_ error: Error, termination: RequestContext.Termination) -> FfiHttpError {
        guard let urlError = error as? URLError else {
            return .transport(message: (error as NSError).localizedDescription)
        }
        switch urlError.code {
        case .timedOut:
            return .timeout
        case .cancelled:
            switch termination {
            case .deadline:
                return .timeout
            case .callerCancel, .none:
                return .cancelled
            }
        case .cannotFindHost, .dnsLookupFailed:
            return .nameResolution
        case .cannotConnectToHost, .notConnectedToInternet:
            return .connect
        case .httpTooManyRedirects:
            // The request carries no redirect limit and the delegate-driven policy is M2, so
            // URLSession's own internal cap fired: `0` is the "adapter-internal" sentinel (§ FFI).
            return .tooManyRedirects(limit: 0)
        case .secureConnectionFailed,
             .serverCertificateUntrusted,
             .serverCertificateHasBadDate,
             .serverCertificateHasUnknownRoot,
             .serverCertificateNotYetValid,
             .clientCertificateRejected,
             .clientCertificateRequired:
            return .tls
        default:
            // Anything after the connection was established (reset, truncated mid-body, unparseable):
            // the rule-8 / key-transport target.
            return .transport(message: urlError.localizedDescription)
        }
    }

    /// `URLSessionTaskMetrics.networkProtocolName` → the contract version (row 11). Absent metrics
    /// default to HTTP/1.1 (the test server speaks 1.1; the value is the real observable, not a
    /// placeholder).
    static func mapVersion(_ name: String?) -> FfiHttpVersion {
        switch name?.lowercased() {
        case "http/1.0":
            return .http10
        case "http/1.1":
            return .http11
        case "h2", "http/2", "http/2.0":
            return .http2
        case "h3", "http/3", "http/3.0":
            return .http3
        default:
            return .http11
        }
    }

    // MARK: - Internals

    /// Look up the in-flight context for `task` via its `taskDescription`-carried token. The caller
    /// must hold `lock`.
    private func context(for task: URLSessionTask) -> RequestContext? {
        guard let desc = task.taskDescription, let token = UInt64(desc) else { return nil }
        return contexts[token]
    }
}

/// The mutable per-request state the delegate callbacks accumulate. All access is guarded by
/// `BoltedHttp.lock`.
final class RequestContext {
    /// How an in-flight request was terminated early — the cause that classifies a `URLError.cancelled`.
    enum Termination {
        case none
        case deadline
        case callerCancel
    }

    let token: UInt64
    let requestURL: String
    var task: URLSessionTask?
    var deadlineTimer: DispatchSourceTimer?
    var termination: Termination = .none
    var finished = false

    var response: HTTPURLResponse?
    var buffer = Data()
    var protocolName: String?
    var uploadTotal: UInt64?
    var sentSoFar: UInt64 = 0

    init(token: UInt64, requestURL: String) {
        self.token = token
        self.requestURL = requestURL
    }
}
