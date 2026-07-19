import Foundation

/// `BoltedHttp` — the hand-written Apple HTTP adapter (architecture.md §1, layer 3).
///
/// **M0 status: a walking skeleton.** It implements the `HttpAdapter` callback trait over
/// `URLSession` with just enough behaviour to pass exactly one C1 conformance row (rule 1 — a
/// determinate GET of `/ok`). The full adapter — deadline synthesis, cancellation, the C2 error
/// keys, pinning, upload progress — is milestone M1+. Everything here is deliberately minimal.
///
/// It lives in the SAME SwiftPM target as the generated bindings (the `bundled` packaging layout),
/// so the generated `FfiRequest` / `FfiResponse` / `HttpHarness` types need no `import`.
///
/// Composition-root wiring (the three-line dance the XCTest performs): the adapter is built first,
/// the harness second (it takes the adapter), then the harness is set back on the adapter so the
/// completion can re-enter. The back-reference is `weak` — the Rust side owns the adapter across
/// the FFI, so a strong reference here would cycle.
public final class BoltedHttp: HttpAdapter {
    /// Weak by design: the harness owns this adapter through the FFI bridge.
    public weak var harness: HttpHarness?

    private let session: URLSession

    public init() {
        // Contract defaults: cookie-less, cache-less (architecture.md §2).
        let config = URLSessionConfiguration.ephemeral
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.session = URLSession(configuration: config)
    }

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
        // One total deadline is the whole timeout story for the portable core. (M1 replaces this
        // with a real synthesized total deadline — URLSession's `timeoutInterval` is per-idle.)
        if request.deadlineMs > 0 {
            urlRequest.timeoutInterval = TimeInterval(request.deadlineMs) / 1000.0
        }
        for header in request.headers {
            urlRequest.setValue(header.value, forHTTPHeaderField: header.name)
        }
        if !request.body.isEmpty {
            urlRequest.httpBody = request.body
        }

        let token = request.token
        let task = session.dataTask(with: urlRequest) { [weak self] data, response, error in
            guard let self else { return }
            if let error {
                self.harness?.completeErr(
                    token: token,
                    error: Self.mapError(error)
                )
                return
            }
            guard let http = response as? HTTPURLResponse else {
                self.harness?.completeErr(
                    token: token,
                    error: .transport(message: "non-HTTP response")
                )
                return
            }
            var headers: [FfiHeader] = []
            for (name, value) in http.allHeaderFields {
                headers.append(
                    FfiHeader(
                        name: String(describing: name),
                        value: String(describing: value)
                    )
                )
            }
            self.harness?.completeOk(
                response: FfiResponse(
                    token: token,
                    status: UInt16(http.statusCode),
                    headers: headers,
                    body: data ?? Data(),
                    finalUrl: http.url?.absoluteString ?? request.url
                )
            )
        }
        task.resume()
    }

    /// Native failure → typed error key. **M0-minimal**: just enough to keep the skeleton honest
    /// (an error is a typed key, never a string). The full C2 taxonomy mapping is M1.
    static func mapError(_ error: Error) -> FfiHttpError {
        guard let urlError = error as? URLError else {
            return .transport(message: (error as NSError).localizedDescription)
        }
        switch urlError.code {
        case .timedOut:
            return .timeout
        case .cancelled:
            return .cancelled
        case .cannotFindHost, .dnsLookupFailed:
            return .nameResolution
        case .cannotConnectToHost, .networkConnectionLost, .notConnectedToInternet:
            return .connect
        case .secureConnectionFailed,
             .serverCertificateUntrusted,
             .serverCertificateHasBadDate,
             .serverCertificateHasUnknownRoot,
             .serverCertificateNotYetValid:
            return .tls
        default:
            return .transport(message: urlError.localizedDescription)
        }
    }
}
