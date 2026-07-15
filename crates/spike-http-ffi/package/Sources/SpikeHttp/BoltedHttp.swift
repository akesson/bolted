import Foundation

/// Hand-written shipped adapter — the spike stand-in for bolted-http's
/// `BoltedHttp.swift` (architecture.md §1, layer 3). Lives in the SAME SwiftPM
/// target as the generated bindings (bundled layout), so no import is needed.
///
/// Wiring: the adapter is constructed first, the core second (it takes the
/// adapter), then `core` is set on the adapter so completions can re-enter.
/// The composition root (driver) owns this three-line dance.
public final class BoltedHttpAdapter: HttpAdapter {
    /// Weak: the test/app owns the core; the Rust side owns this adapter via
    /// the FFI bridge, so a strong reference here would cycle across the FFI.
    public weak var core: SpikeCore?

    private let session: URLSession
    private let stateLock = NSLock()
    private var _lastCompletionThread = ""

    /// Which thread the URLSession completion (and thus the core re-entry) ran on.
    public var lastCompletionThread: String {
        stateLock.lock()
        defer { stateLock.unlock() }
        return _lastCompletionThread
    }

    public init() {
        // Contract defaults: cookie-less, cache-less (architecture.md §2).
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.session = URLSession(configuration: config)
    }

    public func execute(request: HttpRequest) {
        guard let url = URL(string: request.url) else {
            core?.completeErr(
                token: request.token,
                error: .transport(code: -1, message: "invalid url")
            )
            return
        }
        var urlRequest = URLRequest(url: url)
        urlRequest.httpMethod = request.method
        // One total deadline is the whole timeout story (portable core).
        urlRequest.timeoutInterval = TimeInterval(request.deadlineMs) / 1000.0
        for header in request.headers {
            urlRequest.setValue(header.value, forHTTPHeaderField: header.name)
        }
        if !request.body.isEmpty {
            urlRequest.httpBody = request.body
        }

        let token = request.token
        let deadlineMs = request.deadlineMs
        let host = url.host ?? ""
        let task = session.dataTask(with: urlRequest) { [weak self] data, response, error in
            guard let self else { return }
            self.stateLock.lock()
            self._lastCompletionThread = Thread.isMainThread
                ? "main"
                : "background(\(Thread.current))"
            self.stateLock.unlock()

            if let error {
                self.core?.completeErr(
                    token: token,
                    error: Self.mapError(error, deadlineMs: deadlineMs, host: host)
                )
                return
            }
            guard let http = response as? HTTPURLResponse else {
                self.core?.completeErr(
                    token: token,
                    error: .transport(code: -2, message: "non-HTTP response")
                )
                return
            }
            var headers: [HttpHeader] = []
            for (name, value) in http.allHeaderFields {
                headers.append(HttpHeader(
                    name: String(describing: name),
                    value: String(describing: value)
                ))
            }
            self.core?.completeOk(response: HttpResponse(
                token: token,
                status: UInt16(http.statusCode),
                headers: headers,
                body: data ?? Data(),
                finalUrl: http.url?.absoluteString ?? request.url
            ))
        }
        task.resume()
    }

    public func ping(n: UInt64) -> UInt64 {
        n
    }

    /// Native failure → typed error key. These three mappings are the first
    /// rows of the http conformance suite.
    static func mapError(_ error: Error, deadlineMs: UInt64, host: String) -> HttpError {
        guard let urlError = error as? URLError else {
            return .transport(code: Int64((error as NSError).code),
                              message: error.localizedDescription)
        }
        switch urlError.code {
        case .timedOut:
            return .timeout(deadlineMs: deadlineMs)
        case .cannotFindHost, .dnsLookupFailed:
            return .dnsFailure(host: host)
        case .secureConnectionFailed,
             .serverCertificateUntrusted,
             .serverCertificateHasBadDate,
             .serverCertificateHasUnknownRoot,
             .serverCertificateNotYetValid,
             .clientCertificateRejected,
             .clientCertificateRequired,
             .appTransportSecurityRequiresSecureConnection:
            return .tlsFailure(reason: "URLError.\(urlError.code.rawValue)")
        default:
            return .transport(code: Int64(urlError.errorCode),
                              message: urlError.localizedDescription)
        }
    }
}
