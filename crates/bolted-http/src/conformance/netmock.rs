//! The socket mock (feature `conformance`): a real, minimal, blocking HTTP/1.1 + TLS client that
//! drives the [`super::server::TestServer`]. It is *the* conformance vehicle — the eleven §7 rules
//! target adapter behaviour (deadline synthesis, https→http refusal, gzip normalization, SPKI
//! pinning), and a purely-scripted mock could only tautologically "pass" them. So the mock grows
//! real-socket ability (spike-plan S-CONF: "expected and fine"). It stays runtime-free: one std
//! worker thread per request, plus a watchdog thread that enforces the deadline / cancellation by
//! shutting the socket down (never mid-record read timeouts, which would corrupt TLS).
//!
//! Correctness lives in [`MockBehavior`]. The **correct** mock passes every row; each red-twin is
//! the same mock with exactly one flag flipped, so every row is watched red against a break that
//! targets it (the M0 fail-correctly pattern, generalised).
//!
//! `Instant::now` is disallowed workspace-wide (replay determinism, clippy.toml). An adapter
//! enforcing a real wall-clock deadline is exactly the sanctioned exception — the ban is for the
//! sans-io core, not for a device-tier executor. The uses are annotated locally.

use std::io;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use super::AdapterFactory;
use super::wire::{self, ReadWrite};
use crate::capability::{CancelToken, CompletionSink, Http, Metrics, MetricsTier, RequestHandle};
use crate::error::{HttpError, TlsErrorKind};
use crate::request::{HttpRequest, RequestBody, SpkiPin, Url};
use crate::response::{BodyOutcome, HttpResponse, HttpVersion, StatusCode};

/// Global nonce source for the non-deterministic red-twin (rule 1). Every read is unique.
static NONCE: AtomicU64 = AtomicU64::new(0);

/// What the mock does — every flag is `true`/correct in [`MockBehavior::correct`]; a red-twin
/// flips exactly one.
#[derive(Clone, Copy, Debug)]
pub struct MockBehavior {
    /// Enforce the request deadline (rule 3). Off ⇒ a stalled body hangs (NoCompletion).
    pub arm_deadline: bool,
    /// Complete a cancelled request with a terminal outcome (rule 9). Off ⇒ silence.
    pub honor_cancel: bool,
    /// Classify a cancellation as `Cancelled`, distinct from a deadline's `Timeout` (rule 2).
    /// Off ⇒ cancel is mis-reported as `Timeout` (the conflation break).
    pub classify_cancel: bool,
    /// Refuse to follow an https→http redirect (rule 4). Off ⇒ follows it (the leak).
    pub refuse_insecure_redirect: bool,
    /// Actually transmit the request's permitted headers (rule 6). Off ⇒ silently drops them.
    pub send_headers: bool,
    /// Decode a gzip response body (rule 7). Off ⇒ surfaces raw compressed bytes.
    pub decode_gzip: bool,
    /// Enforce SPKI pins (rule 10). Off ⇒ pinning is bypassed (the pin-bypass break).
    pub check_pins: bool,
    /// Produce a deterministic outcome (rule 1). Off ⇒ injects a per-call nonce into the body.
    pub deterministic: bool,
    /// Re-send a request that failed mid-flight (rule 8 — a *break* when true).
    pub retry_on_transport: bool,
    /// The redirect-follow ceiling (TooManyRedirects beyond it).
    pub redirect_limit: u32,
}

impl MockBehavior {
    /// The correct mock: passes every row.
    #[must_use]
    pub fn correct() -> Self {
        MockBehavior {
            arm_deadline: true,
            honor_cancel: true,
            classify_cancel: true,
            refuse_insecure_redirect: true,
            send_headers: true,
            decode_gzip: true,
            check_pins: true,
            deterministic: true,
            retry_on_transport: false,
            redirect_limit: 10,
        }
    }
}

/// A source of socket mocks. `trusted_spki` is the SPKI the non-pinned TLS path trusts (the good
/// cert); everything else is an untrusted root.
#[derive(Clone)]
pub struct SocketMockFactory {
    trusted_spki: [u8; 32],
    behavior: MockBehavior,
}

impl SocketMockFactory {
    /// A correct factory trusting `trusted_spki`.
    #[must_use]
    pub fn correct(trusted_spki: [u8; 32]) -> Self {
        SocketMockFactory {
            trusted_spki,
            behavior: MockBehavior::correct(),
        }
    }

    /// The same factory with a mutated behaviour (a red-twin).
    #[must_use]
    pub fn with_behavior(mut self, mutate: impl FnOnce(&mut MockBehavior)) -> Self {
        mutate(&mut self.behavior);
        self
    }
}

impl AdapterFactory for SocketMockFactory {
    fn new_adapter(&self) -> Box<dyn Http> {
        Box::new(SocketMock {
            trusted_spki: self.trusted_spki,
            behavior: self.behavior,
        })
    }

    fn metrics(&self) -> Option<Box<dyn Metrics>> {
        // The socket mock honestly reports whole-request metrics — present, tier B.
        Some(Box::new(SocketMock {
            trusted_spki: self.trusted_spki,
            behavior: self.behavior,
        }))
    }
}

/// A real-socket [`Http`] adapter.
#[derive(Clone)]
pub struct SocketMock {
    trusted_spki: [u8; 32],
    behavior: MockBehavior,
}

impl Metrics for SocketMock {
    fn tier(&self) -> MetricsTier {
        MetricsTier::WholeRequest
    }
}

impl Http for SocketMock {
    fn send(&self, request: HttpRequest, completion: Box<dyn CompletionSink>) -> RequestHandle {
        let token = CancelToken::new();
        let worker_token = token.clone();
        let me = self.clone();
        // One worker thread per request: it may block on I/O, and the caller needs the handle back
        // immediately to be able to cancel (rule 9). Not an async runtime — one thread, one effect.
        thread::spawn(move || {
            let outcome = me.perform(&request, &worker_token);
            completion.complete(outcome);
        });
        RequestHandle::for_token(token)
    }
}

/// Why the watchdog stopped the exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Deadline,
    Cancel,
}

/// Why the TLS verifier rejected the server cert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TlsReject {
    PinMismatch,
    Untrusted,
}

impl SocketMock {
    #[allow(clippy::disallowed_methods)] // an executor enforcing a wall-clock deadline; see module note
    fn perform(
        &self,
        request: &HttpRequest,
        token: &CancelToken,
    ) -> Result<HttpResponse, HttpError> {
        let start = Instant::now();
        let deadline_at = self
            .behavior
            .arm_deadline
            .then(|| start + request.deadline());

        let mut current = request.url().clone();
        let mut hops: Vec<Url> = Vec::new();

        loop {
            let parsed = parse_url(current.as_str()).ok_or(HttpError::Connect)?;
            let exchange = self.exchange(request, &current, &parsed, deadline_at, token);
            let head = exchange?;

            if let Some(location) = redirect_location(head.status, &head.headers_raw) {
                let next = resolve_location(&parsed, &location).ok_or(HttpError::Transport)?;
                let next_parsed = parse_url(&next).ok_or(HttpError::Transport)?;
                // rule 4: an https→http redirect is refused, never followed. (A broken twin sets
                // `refuse_insecure_redirect = false` and falls through to follow it.)
                if parsed.scheme == Scheme::Https
                    && next_parsed.scheme == Scheme::Http
                    && self.behavior.refuse_insecure_redirect
                {
                    let to = Url::cleartext_dev(&next).map_err(|_| HttpError::Transport)?;
                    return Err(HttpError::InsecureRedirect { to });
                }
                if hops.len() as u32 >= self.behavior.redirect_limit {
                    return Err(HttpError::TooManyRedirects {
                        limit: self.behavior.redirect_limit,
                    });
                }
                hops.push(current.clone());
                current = build_url(&next_parsed).ok_or(HttpError::Transport)?;
                continue;
            }

            // Terminal response.
            let mut body = head.body;
            if self.behavior.decode_gzip
                && header_has(&head.headers_raw, "content-encoding", "gzip")
            {
                body = wire::gunzip(&body).map_err(|_| HttpError::Transport)?;
            }
            if !self.behavior.deterministic {
                // The nondeterminism break: two identical requests now differ (rule 1).
                let n = NONCE.fetch_add(1, Ordering::SeqCst);
                body.extend_from_slice(format!("#{n}").as_bytes());
            }

            let mut resp = HttpResponse::builder(
                StatusCode::new(head.status),
                current.clone(),
                HttpVersion::Http1_1,
                BodyOutcome::Memory(body),
            );
            for h in hops {
                resp = resp.hop(h);
            }
            let mut headers = crate::header::Headers::new();
            for (name, value) in &head.headers_raw {
                if let (Ok(n), Ok(v)) = (
                    crate::header::HeaderName::parse(name),
                    crate::header::HeaderValue::from_bytes(value.clone()),
                ) {
                    headers.append(n, v);
                }
            }
            return Ok(resp.headers(headers).build());
        }
    }

    /// One request/response exchange against `parsed`, under the deadline/cancel watchdog. On a
    /// retrying twin, a mid-flight `Transport` failure is retried once (the rule-8 break).
    fn exchange(
        &self,
        request: &HttpRequest,
        url: &Url,
        parsed: &ParsedUrl,
        deadline_at: Option<Instant>,
        token: &CancelToken,
    ) -> Result<Head, HttpError> {
        let mut last = self.exchange_once(request, url, parsed, deadline_at, token);
        if self.behavior.retry_on_transport && matches!(last, Err(HttpError::Transport)) {
            last = self.exchange_once(request, url, parsed, deadline_at, token);
        }
        last
    }

    fn exchange_once(
        &self,
        request: &HttpRequest,
        url: &Url,
        parsed: &ParsedUrl,
        deadline_at: Option<Instant>,
        token: &CancelToken,
    ) -> Result<Head, HttpError> {
        // Resolve first: a name that will not resolve is NameResolution, distinct from a refused
        // connection (Connect).
        let addrs = (parsed.host.as_str(), parsed.port)
            .to_socket_addrs()
            .map_err(|_| HttpError::NameResolution)?;
        let addrs: Vec<_> = addrs.collect();
        if addrs.is_empty() {
            return Err(HttpError::NameResolution);
        }
        let tcp = connect_any(&addrs).map_err(|_| HttpError::Connect)?;
        let _ = tcp.set_nodelay(true);
        let shutdown_handle = tcp.try_clone().map_err(|_| HttpError::Connect)?;

        // The watchdog: it stops the exchange by shutting the socket down.
        let stop: Arc<Mutex<Option<Stop>>> = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));
        let watchdog = spawn_watchdog(
            shutdown_handle,
            deadline_at,
            self.behavior.honor_cancel.then(|| token.clone()),
            stop.clone(),
            done.clone(),
        );

        let tls_reject: Arc<Mutex<Option<TlsReject>>> = Arc::new(Mutex::new(None));
        let result = self.exchange_io(request, url, parsed, &tls_reject, &tcp);

        done.store(true, Ordering::SeqCst);
        let _ = watchdog.join();

        result.map_err(|io_err| {
            let stop_reason = stop.lock().ok().and_then(|g| *g);
            let reject = tls_reject.lock().ok().and_then(|g| *g);
            map_io_error(stop_reason, reject, &io_err, self.behavior.classify_cancel)
        })
    }

    fn exchange_io(
        &self,
        request: &HttpRequest,
        url: &Url,
        parsed: &ParsedUrl,
        tls_reject: &Arc<Mutex<Option<TlsReject>>>,
        tcp: &TcpStream,
    ) -> io::Result<Head> {
        let mut transport: Box<dyn ReadWrite> = match parsed.scheme {
            Scheme::Http => Box::new(tcp.try_clone()?),
            Scheme::Https => {
                let verifier = Arc::new(PinTrustVerifier::new(
                    self.trusted_spki,
                    request.pins(),
                    self.behavior.check_pins,
                    tls_reject.clone(),
                ));
                let cfg = wire::client_config(verifier);
                let server_name = ServerName::try_from(parsed.host.clone())
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
                let conn = rustls::ClientConnection::new(cfg, server_name)
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                Box::new(rustls::StreamOwned::new(conn, tcp.try_clone()?))
            }
        };
        let t: &mut dyn ReadWrite = &mut *transport;

        // Request head.
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(
            format!("{} {} HTTP/1.1\r\n", method_str(request), parsed.path).as_bytes(),
        );
        out.extend_from_slice(format!("host: {}\r\n", parsed.host).as_bytes());
        if self.behavior.send_headers {
            for (name, value) in request.headers().iter() {
                out.extend_from_slice(name.as_str().as_bytes());
                out.extend_from_slice(b": ");
                out.extend_from_slice(value.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
        let body_bytes: &[u8] = match request.body() {
            RequestBody::Bytes(b) => b,
            _ => &[],
        };
        out.extend_from_slice(format!("content-length: {}\r\n", body_bytes.len()).as_bytes());
        out.extend_from_slice(b"connection: close\r\n\r\n");
        out.extend_from_slice(body_bytes);
        t.write_all(&out)?;
        t.flush()?;

        // Response head + body.
        let (head_bytes, leftover) = wire::read_head(&mut *t)?;
        let mut hbuf = [httparse::EMPTY_HEADER; 64];
        let mut resp = httparse::Response::new(&mut hbuf);
        resp.parse(&head_bytes)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let status = resp
            .code
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let headers_raw: Vec<(String, Vec<u8>)> = resp
            .headers
            .iter()
            .filter(|h| !h.name.is_empty())
            .map(|h| (h.name.to_ascii_lowercase(), h.value.to_vec()))
            .collect();

        let _ = url; // (kept for symmetry; the response records `current`, not the request url)
        let body = match content_length(&headers_raw) {
            Some(len) => wire::read_body_exact(&mut *t, leftover, len)?,
            None => wire::read_body_to_end(&mut *t, leftover)?,
        };
        Ok(Head {
            status,
            headers_raw,
            body,
        })
    }
}

/// A parsed response head + body.
struct Head {
    status: u16,
    headers_raw: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

fn connect_any(addrs: &[std::net::SocketAddr]) -> io::Result<TcpStream> {
    let mut last = io::Error::from(io::ErrorKind::AddrNotAvailable);
    for addr in addrs {
        match TcpStream::connect_timeout(addr, Duration::from_secs(5)) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[allow(clippy::disallowed_methods)] // watchdog compares against a wall-clock deadline; see module note
fn spawn_watchdog(
    socket: TcpStream,
    deadline_at: Option<Instant>,
    cancel: Option<CancelToken>,
    stop: Arc<Mutex<Option<Stop>>>,
    done: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            if done.load(Ordering::SeqCst) {
                return;
            }
            if let Some(at) = deadline_at
                && Instant::now() >= at
            {
                if let Ok(mut g) = stop.lock() {
                    *g = Some(Stop::Deadline);
                }
                let _ = socket.shutdown(Shutdown::Both);
                return;
            }
            if let Some(token) = &cancel
                && token.is_cancelled()
            {
                if let Ok(mut g) = stop.lock() {
                    *g = Some(Stop::Cancel);
                }
                let _ = socket.shutdown(Shutdown::Both);
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    })
}

fn map_io_error(
    stop: Option<Stop>,
    tls_reject: Option<TlsReject>,
    io_err: &io::Error,
    classify_cancel: bool,
) -> HttpError {
    if let Some(s) = stop {
        return match s {
            Stop::Deadline => HttpError::Timeout,
            // The conflation break reports a cancel as a timeout (rule 2).
            Stop::Cancel if classify_cancel => HttpError::Cancelled,
            Stop::Cancel => HttpError::Timeout,
        };
    }
    if let Some(t) = tls_reject {
        return match t {
            TlsReject::PinMismatch => HttpError::PinMismatch,
            TlsReject::Untrusted => HttpError::Tls {
                kind: TlsErrorKind::UntrustedRoot,
            },
        };
    }
    match io_err.kind() {
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::AddrNotAvailable
        | io::ErrorKind::TimedOut => HttpError::Connect,
        _ => HttpError::Transport,
    }
}

// --- URL parsing ------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

struct ParsedUrl {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Option<ParsedUrl> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        (Scheme::Https, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (Scheme::Http, r)
    } else {
        return None;
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (
            authority.to_string(),
            match scheme {
                Scheme::Https => 443,
                Scheme::Http => 80,
            },
        ),
    };
    Some(ParsedUrl {
        scheme,
        host,
        port,
        path,
    })
}

fn build_url(p: &ParsedUrl) -> Option<Url> {
    let scheme = match p.scheme {
        Scheme::Https => "https",
        Scheme::Http => "http",
    };
    let s = format!("{scheme}://{}:{}{}", p.host, p.port, p.path);
    match p.scheme {
        Scheme::Https => Url::https(&s).ok(),
        Scheme::Http => Url::cleartext_dev(&s).ok(),
    }
}

fn resolve_location(base: &ParsedUrl, location: &str) -> Option<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        Some(location.to_string())
    } else if let Some(rest) = location.strip_prefix('/') {
        let scheme = match base.scheme {
            Scheme::Https => "https",
            Scheme::Http => "http",
        };
        Some(format!("{scheme}://{}:{}/{}", base.host, base.port, rest))
    } else {
        None
    }
}

fn redirect_location(status: u16, headers: &[(String, Vec<u8>)]) -> Option<String> {
    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        for (name, value) in headers {
            if name == "location" {
                return String::from_utf8(value.clone()).ok();
            }
        }
    }
    None
}

fn content_length(headers: &[(String, Vec<u8>)]) -> Option<usize> {
    for (name, value) in headers {
        if name == "content-length" {
            return std::str::from_utf8(value).ok()?.trim().parse().ok();
        }
    }
    None
}

fn header_has(headers: &[(String, Vec<u8>)], name: &str, value: &str) -> bool {
    headers.iter().any(|(n, v)| {
        n == name
            && std::str::from_utf8(v)
                .map(|s| s.eq_ignore_ascii_case(value))
                .unwrap_or(false)
    })
}

fn method_str(request: &HttpRequest) -> &'static str {
    use crate::request::Method;
    match request.method() {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Patch => "PATCH",
        Method::Delete => "DELETE",
        Method::Options => "OPTIONS",
    }
}

// --- The TLS verifier: pinning + trust, recording the reason for rejection ---------------

#[derive(Debug)]
struct PinTrustVerifier {
    trusted_spki: [u8; 32],
    pins: Option<Vec<[u8; 32]>>,
    check_pins: bool,
    reject: Arc<Mutex<Option<TlsReject>>>,
    algs: WebPkiSupportedAlgorithms,
}

impl PinTrustVerifier {
    fn new(
        trusted_spki: [u8; 32],
        pins: Option<&crate::request::PinSet>,
        check_pins: bool,
        reject: Arc<Mutex<Option<TlsReject>>>,
    ) -> Self {
        let pins = pins.map(|set| set.pins().iter().map(|p: &SpkiPin| *p.as_bytes()).collect());
        let algs = rustls::crypto::ring::default_provider().signature_verification_algorithms;
        PinTrustVerifier {
            trusted_spki,
            pins,
            check_pins,
            reject,
            algs,
        }
    }

    fn record(&self, why: TlsReject) -> rustls::Error {
        if let Ok(mut g) = self.reject.lock() {
            *g = Some(why);
        }
        rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
    }
}

impl ServerCertVerifier for PinTrustVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let spki = wire::spki_sha256(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;

        // With pins present and enforced, the pin set is the trust anchor (rule 10).
        if self.check_pins
            && let Some(pins) = &self.pins
        {
            return if pins.contains(&spki) {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(self.record(TlsReject::PinMismatch))
            };
        }
        // Otherwise: SPKI-allowlist trust (the harness's stand-in for WebPKI; the reqwest adapter
        // does real chain verification). The good cert is trusted; anything else is untrusted.
        if spki == self.trusted_spki {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(self.record(TlsReject::Untrusted))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}
