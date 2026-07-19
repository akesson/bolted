import CryptoKit
import Foundation
import Security

/// `BoltedHttp` — the hand-written Apple HTTP adapter (architecture.md §1, layer 3).
///
/// **M2 status: the syntheses landed.** On top of M1 (dispatch, total-deadline synthesis, caller
/// cancellation, the C2 error-key mapping, upload progress, anchor-based server trust, the real
/// negotiated HTTP version) M2 adds the four adapter-side syntheses:
///
/// - **SPKI pinning** (rule 10): the trust-evaluation delegate does a real chain + hostname check
///   against the installed anchor AND, when the request carries pins, compares SHA-256 over the
///   presented leaf's SubjectPublicKeyInfo. A pin mismatch is `PinMismatch`; a trust/hostname
///   failure is `Tls` — the Linux verifier's split, mirrored exactly, never conflated.
/// - **https→http refusal + the hop trace** (rules 4/7): `willPerformHTTPRedirection` refuses a
///   downgrade with the typed `InsecureRedirect` key and records every intermediate URL so the
///   response carries the redirect trace; the synthesized total deadline already spans the chain.
/// - **The file sink** (row 15 / the `Io` positive control): a `File` response sink uses a
///   `downloadTask` and persists the temp file **synchronously inside** `didFinishDownloadingTo`
///   (the temp-file-lifetime rule) with an atomic temp-then-rename finalize; a write failure is `Io`.
///
/// `PermissionDenied` has a genuine-`EPERM` mapping (see `permissionKeyForPOSIX`); a live host
/// control is platform-gated on the macOS SwiftPM test tier (M2 notes).
///
/// It lives in the SAME SwiftPM target as the generated bindings (the `bundled` packaging layout),
/// so the generated `FfiRequest` / `FfiResponse` / `HttpHarness` types need no `import`.
public final class BoltedHttp: NSObject, HttpAdapter, URLSessionDataDelegate, URLSessionDownloadDelegate {
    /// Weak by design: the harness owns this adapter through the FFI bridge.
    public weak var harness: HttpHarness?

    /// The good endpoint's DER-encoded certificate, installed as the sole trust anchor for
    /// server-trust evaluation. Set by the composition root from `ServerInfo.goodCertDer` after
    /// `startServer()`. `nil` ⇒ default system trust evaluation, which rejects the self-signed test
    /// certificates (so the untrusted endpoint stays a `Tls` positive control).
    public var trustAnchorDER: Data?

    /// A total-deadline timer runs off this queue (concurrent — one short-lived source per request).
    private static let timerQueue = DispatchQueue(label: "bolted.http.deadline", attributes: .concurrent)

    private var session: URLSession!
    /// Guards `contexts`, `_lastTaskPriority`, and every mutation of a `RequestContext` after it is
    /// registered.
    private let lock = NSLock()
    /// In-flight requests, keyed by the FFI token (the delegate re-derives the token from
    /// `URLSessionTask.taskDescription`).
    private var contexts: [UInt64: RequestContext] = [:]

    /// A5 acceptance (step 25): the `URLSessionTask.priority` the adapter last applied, recorded so
    /// the acceptance test can assert the task CARRIED the mapped value. Acceptance-only — the
    /// RFC 9218 wire behaviour is FLAGGED lore and deliberately NOT conformance-tested.
    private var _lastTaskPriority: Float?
    public var lastTaskPriority: Float? {
        lock.lock(); defer { lock.unlock() }; return _lastTaskPriority
    }

    /// The no-argument initializer used everywhere except the A6 sweep: OS-default loading mode.
    public override convenience init() {
        self.init(classicLoading: nil)
    }

    /// A6 (step 25): the classic-loading-mode sweep. `classicLoading == nil` leaves the OS default
    /// (used by every normal test); the sweep constructs the adapter with `false` to force the new
    /// (non-classic) loading path and records whether the suite diverges. One flag on the adapter —
    /// the adapter is NOT forked.
    public init(classicLoading: Bool?) {
        super.init()
        // Contract defaults: cookie-less, cache-less (architecture.md §2).
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        if let classic = classicLoading, #available(macOS 15.4, iOS 18.4, *) {
            config.usesClassicLoadingMode = classic
        }
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

        // Row 15: a `File` sink downloads to a temp file (so the body is never buffered) and is
        // persisted inside `didFinishDownloadingTo`; a `Memory` sink buffers via `didReceive data`.
        let sinkPath: String?
        let task: URLSessionTask
        switch request.sink {
        case .file(let path):
            sinkPath = path
            task = session.downloadTask(with: urlRequest)
        case .memory:
            sinkPath = nil
            task = session.dataTask(with: urlRequest)
        }
        task.taskDescription = String(request.token)
        // A5 (row 12, CAP): honour the request's priority hint by mapping it to the task priority.
        // Acceptance-only; the RFC 9218 wire behaviour is FLAGGED lore, not conformance-tested.
        task.priority = Self.taskPriority(for: request.priority)

        let ctx = RequestContext(token: request.token, requestURL: request.url)
        ctx.task = task
        ctx.uploadTotal = uploadTotal
        ctx.sinkPath = sinkPath
        // Rule 10: the request's SPKI pins, enforced in the trust delegate (empty ⇒ no pinning).
        ctx.pins = request.pins.map { Data($0.hash) }

        lock.lock()
        contexts[request.token] = ctx
        // Record the applied priority (read back off the task) for the A5 acceptance assertion.
        _lastTaskPriority = task.priority
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
        if case .none = ctx.termination { ctx.termination = .callerCancel }
        let task = ctx.task
        lock.unlock()
        task?.cancel()
    }

    // MARK: - Deadline

    private func deadlineFired(token: UInt64) {
        lock.lock()
        guard let ctx = contexts[token], !ctx.finished, case .none = ctx.termination else {
            lock.unlock()
            return
        }
        ctx.termination = .deadline
        let task = ctx.task
        lock.unlock()
        task?.cancel()
    }

    // MARK: - URLSessionTaskDelegate (server trust — real chain + hostname AND SPKI pinning)

    /// The trust-evaluation delegate at the TASK level so it can read the request's pins. Mirrors the
    /// Linux `PinningVerifier` split exactly: (1) a real chain + hostname evaluation against the
    /// installed anchor; then (2) — only when pins are present and the chain PASSED — the declarative
    /// SPKI pin check on the leaf. A chain/hostname failure falls through to default handling and
    /// surfaces as `Tls`; a pin mismatch cancels the challenge and surfaces as `PinMismatch`.
    public func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
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
        // 1. The REAL trust decision: chain building + hostname matching, trusting exactly our anchor.
        SecTrustSetAnchorCertificates(serverTrust, [anchor] as CFArray)
        SecTrustSetAnchorCertificatesOnly(serverTrust, true)
        guard SecTrustEvaluateWithError(serverTrust, nil) else {
            // Trust/hostname failure ⇒ default handling ⇒ the system rejects ⇒ `Tls` (the split's
            // trust arm). The untrusted endpoint lands here.
            completionHandler(.performDefaultHandling, nil)
            return
        }

        // 2. The declarative SPKI pins, ANDed on top of a PASSING chain (rule 10). Absent ⇒ accept.
        lock.lock()
        let pins = context(for: task)?.pins ?? []
        lock.unlock()
        if !pins.isEmpty {
            guard let leafPin = Self.leafSpkiSha256(serverTrust), pins.contains(leafPin) else {
                // Chain passed, pin did not ⇒ `PinMismatch`, never `Tls`. Fail closed if we cannot
                // compute the leaf SPKI (a pin was requested and could not be satisfied).
                markPinMismatch(task)
                completionHandler(.cancelAuthenticationChallenge, nil)
                return
            }
        }
        completionHandler(.useCredential, URLCredential(trust: serverTrust))
    }

    /// Record the pin-mismatch cause so `didCompleteWithError` maps the resulting failure to
    /// `PinMismatch` regardless of the opaque URLError the cancelled challenge produces.
    private func markPinMismatch(_ task: URLSessionTask) {
        lock.lock()
        if let ctx = context(for: task), case .none = ctx.termination {
            ctx.termination = .pinMismatch
        }
        lock.unlock()
    }

    // MARK: - URLSessionTaskDelegate (redirects — refusal + hop trace)

    /// Rule 4 + row 7: refuse an `https → http` downgrade with `InsecureRedirect`, and record every
    /// intermediate URL as a hop for the redirect trace. A permitted redirect is followed (URLSession
    /// still enforces its own chain cap → `httpTooManyRedirects` → `TooManyRedirects`).
    public func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        let fromScheme = response.url?.scheme?.lowercased()
        let toScheme = request.url?.scheme?.lowercased()
        if fromScheme == "https", toScheme == "http" {
            // Refuse the downgrade. Setting the cause + not following makes the completion terminal;
            // `didCompleteWithError` reads the cause first, so the ignored 302 never leaks as success.
            lock.lock()
            if let ctx = context(for: task), case .none = ctx.termination {
                ctx.termination = .insecureRedirect(to: request.url?.absoluteString ?? "")
            }
            lock.unlock()
            completionHandler(nil)
            return
        }
        // A permitted redirect: record the URL that issued it (the hop), then follow.
        lock.lock()
        if let ctx = context(for: task), let hop = response.url?.absoluteString {
            ctx.hops.append(hop)
        }
        lock.unlock()
        completionHandler(request)
    }

    // MARK: - URLSessionDataDelegate (memory sink)

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

    // MARK: - URLSessionDownloadDelegate (file sink)

    /// Row 15 / the `Io` positive control: persist the downloaded body to the requested destination
    /// **synchronously** — URLSession deletes `location` the moment this returns (the temp-file
    /// lifetime rule). Atomic finalize: move the download into the destination directory (so the
    /// rename is same-filesystem), then rename it into place. Any failure (e.g. the destination's
    /// parent directory does not exist — the `Io` control) records `.ioFailure`, mapped to `Io`.
    public func urlSession(
        _ session: URLSession,
        downloadTask: URLSessionDownloadTask,
        didFinishDownloadingTo location: URL
    ) {
        lock.lock()
        let dest = context(for: downloadTask)?.sinkPath
        if let http = downloadTask.response as? HTTPURLResponse {
            context(for: downloadTask)?.response = http
        }
        lock.unlock()
        guard let dest else { return }

        let destURL = URL(fileURLWithPath: dest)
        let dir = destURL.deletingLastPathComponent()
        let tmp = dir.appendingPathComponent(".\(destURL.lastPathComponent).tmp.\(UUID().uuidString)")
        let fm = FileManager.default
        do {
            // Cross-filesystem move into the destination directory (fails if the dir is missing/
            // unwritable — the `Io` control), then an atomic same-directory rename into place.
            try fm.moveItem(at: location, to: tmp)
            if fm.fileExists(atPath: destURL.path) { try fm.removeItem(at: destURL) }
            try fm.moveItem(at: tmp, to: destURL)
        } catch {
            try? fm.removeItem(at: tmp)
            lock.lock()
            if let ctx = context(for: downloadTask), case .none = ctx.termination {
                ctx.termination = .ioFailure
            }
            lock.unlock()
        }
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
        let response = ctx.response ?? (task.response as? HTTPURLResponse)
        let buffer = ctx.buffer
        let protocolName = ctx.protocolName
        let uploadTotal = ctx.uploadTotal
        let sentSoFar = ctx.sentSoFar
        let requestURL = ctx.requestURL
        let hops = ctx.hops
        let sinkPath = ctx.sinkPath
        lock.unlock()

        // Synthesized terminal causes take precedence over the raw URLError shape (and even over an
        // OS-reported success, for the file-sink write failure): the adapter classifies by CAUSE.
        switch termination {
        case .pinMismatch:
            harness?.completeErr(token: token, error: .pinMismatch)
            return
        case .insecureRedirect(let to):
            harness?.completeErr(token: token, error: .insecureRedirect(to: to))
            return
        case .ioFailure:
            harness?.completeErr(token: token, error: .io)
            return
        case .deadline, .callerCancel, .none:
            break
        }

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
        // A file sink reports the destination path (an empty body) so the core builds a `File`
        // outcome; a memory sink reports the buffered body with an empty `sinkPath`.
        let outcomePath = sinkPath ?? ""
        harness?.completeOk(
            response: FfiResponse(
                token: token,
                status: UInt16(http.statusCode),
                headers: headers,
                body: outcomePath.isEmpty ? buffer : Data(),
                finalUrl: http.url?.absoluteString ?? requestURL,
                httpVersion: Self.mapVersion(protocolName),
                hops: hops,
                sinkPath: outcomePath
            )
        )
    }

    // MARK: - Mapping

    /// Native failure → typed error key (rule 2 — by CAUSE, not exception shape). Covers the full C2
    /// taxonomy the URLSession host tier can reach. The pin / insecure-redirect / io causes are
    /// synthesized and handled before this in `didCompleteWithError`; `PermissionDenied` is mapped
    /// here from a genuine POSIX `EPERM` (a sandbox / local-network denial), never invented.
    static func mapError(_ error: Error, termination: RequestContext.Termination) -> FfiHttpError {
        guard let urlError = error as? URLError else {
            return .transport(message: (error as NSError).localizedDescription)
        }
        // A genuine OS permission denial (EPERM) surfaces as PermissionDenied regardless of the
        // URLError code URLSession chose to wrap it in.
        if let permission = permissionKeyForURLError(urlError) {
            return permission
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
            // The synthesized causes below are handled before `mapError` is reached; fall back to
            // `cancelled` (their URLError is a cancellation) rather than leaking a network key.
            case .pinMismatch, .insecureRedirect, .ioFailure:
                return .cancelled
            }
        case .cannotFindHost, .dnsLookupFailed:
            return .nameResolution
        case .cannotConnectToHost, .notConnectedToInternet:
            return .connect
        case .httpTooManyRedirects:
            // The request carries no redirect limit and URLSession's own internal cap fired: `0` is
            // the "adapter-internal cap" sentinel (§ FFI). No row inspects it, only the key.
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

    /// Map a genuine POSIX errno to a permission key. Only `EPERM` — the sandbox / local-network
    /// denial signal — maps to `PermissionDenied`; everything else is `nil` (not permission-shaped).
    /// This is the load-bearing, unit-testable core of the mapping; see the M2 notes on why a live
    /// host control is platform-gated on the macOS SwiftPM test tier.
    public static func permissionKeyForPOSIX(_ code: Int32) -> FfiHttpError? {
        code == EPERM ? .permissionDenied : nil
    }

    /// Inspect a `URLError`'s underlying error chain for a POSIX `EPERM`, mapping it to
    /// `PermissionDenied`. Returns `nil` when no permission-shaped cause is present.
    static func permissionKeyForURLError(_ urlError: URLError) -> FfiHttpError? {
        var current: NSError? = urlError as NSError
        while let err = current {
            if err.domain == NSPOSIXErrorDomain, let key = permissionKeyForPOSIX(Int32(err.code)) {
                return key
            }
            current = err.userInfo[NSUnderlyingErrorKey] as? NSError
        }
        return nil
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

    // MARK: - Priority (A5, row 12 CAP — acceptance-only)

    /// Map the request's priority hint to `URLSessionTask.priority`. The five contract levels fold
    /// onto URLSession's three named buckets (the platform constants — no magic priority numbers):
    /// `Throttled`/`Low` → low, `Normal` → default, `High`/`Critical` → high. Acceptance-only: the
    /// adapter honours the hint by carrying it on the task; the RFC 9218 wire effect is FLAGGED lore.
    public static func taskPriority(for priority: FfiPriority) -> Float {
        switch priority {
        case .throttled, .low:
            return URLSessionTask.lowPriority
        case .normal:
            return URLSessionTask.defaultPriority
        case .high, .critical:
            return URLSessionTask.highPriority
        }
    }

    // MARK: - SPKI pinning (leaf SubjectPublicKeyInfo SHA-256)

    /// SHA-256 over the presented leaf certificate's SubjectPublicKeyInfo — the same computation the
    /// harness server and Linux verifier use (`x509_parser`'s `public_key().raw`). Extracted by a
    /// minimal structural DER walk of the certificate rather than reconstructed from a `SecKey` (which
    /// omits the SPKI's `AlgorithmIdentifier` wrapper and is key-type specific).
    static func leafSpkiSha256(_ trust: SecTrust) -> Data? {
        let leaf: SecCertificate?
        if #available(macOS 12.0, iOS 15.0, *) {
            leaf = (SecTrustCopyCertificateChain(trust) as? [SecCertificate])?.first
        } else {
            leaf = SecTrustGetCertificateAtIndex(trust, 0)
        }
        guard let cert = leaf else { return nil }
        let certDER = SecCertificateCopyData(cert) as Data
        guard let spki = subjectPublicKeyInfoDER(fromCertificate: certDER) else { return nil }
        return Data(SHA256.hash(data: Data(spki)))
    }

    /// Extract the full SubjectPublicKeyInfo TLV from a DER X.509 certificate. Structural, not
    /// algorithm-specific: `Certificate → tbsCertificate → [optional [0] version] serialNumber,
    /// signature, issuer, validity, subject, subjectPublicKeyInfo` (the 6th field after any version).
    static func subjectPublicKeyInfoDER(fromCertificate certDER: Data) -> [UInt8]? {
        let bytes = [UInt8](certDER)
        // Certificate ::= SEQUENCE { tbsCertificate SEQUENCE { ... }, ... }
        guard let cert = derRead(bytes, 0), cert.element.tag == 0x30,
              let tbs = derRead(cert.element.value, 0), tbs.element.tag == 0x30 else {
            return nil
        }
        var children = derChildren(tbs.element.value)
        // Drop an optional EXPLICIT [0] version (context tag 0xA0).
        if let first = children.first, first.tag == 0xA0 { children.removeFirst() }
        // serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo → index 5.
        guard children.count >= 6 else { return nil }
        return children[5].full
    }

    /// One DER TLV: its tag, the full TLV bytes (header + value), and the value bytes.
    struct DERElement {
        let tag: UInt8
        let full: [UInt8]
        let value: [UInt8]
    }

    /// Read one TLV from `bytes` at `offset`, returning it and the next offset. `nil` on malformed
    /// or truncated input (definite-length DER only, which X.509 certificates always use).
    static func derRead(_ bytes: [UInt8], _ offset: Int) -> (element: DERElement, next: Int)? {
        guard offset < bytes.count else { return nil }
        let tag = bytes[offset]
        var idx = offset + 1
        guard idx < bytes.count else { return nil }
        let lenByte = bytes[idx]
        idx += 1
        var length = 0
        if lenByte & 0x80 == 0 {
            length = Int(lenByte)
        } else {
            let count = Int(lenByte & 0x7F)
            guard count > 0, count <= 4, idx + count <= bytes.count else { return nil }
            for _ in 0..<count {
                length = (length << 8) | Int(bytes[idx])
                idx += 1
            }
        }
        guard length >= 0, idx + length <= bytes.count else { return nil }
        let full = Array(bytes[offset..<(idx + length)])
        let value = Array(bytes[idx..<(idx + length)])
        return (DERElement(tag: tag, full: full, value: value), idx + length)
    }

    /// Read every TLV within a value blob, in order.
    static func derChildren(_ value: [UInt8]) -> [DERElement] {
        var out: [DERElement] = []
        var offset = 0
        while offset < value.count, let (element, next) = derRead(value, offset) {
            out.append(element)
            offset = next
        }
        return out
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
    /// How an in-flight request was terminated early — the cause that classifies the outcome
    /// (independently of the opaque URLError shape).
    enum Termination: Equatable {
        case none
        case deadline
        case callerCancel
        /// A declarative SPKI pin did not match (rule 10).
        case pinMismatch
        /// An `https → http` redirect was refused (rule 4); carries the refused cleartext target.
        case insecureRedirect(to: String)
        /// A file-sink write failed (row 15 / the `Io` control).
        case ioFailure
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
    /// The request's SPKI pins (empty ⇒ no pinning); enforced in the trust delegate (rule 10).
    var pins: [Data] = []
    /// The file-sink destination, or `nil` for a memory sink (row 15).
    var sinkPath: String?
    /// The redirect hop trace — every intermediate URL, in order (row 7).
    var hops: [String] = []

    init(token: UInt64, requestURL: String) {
        self.token = token
        self.requestURL = requestURL
    }
}
