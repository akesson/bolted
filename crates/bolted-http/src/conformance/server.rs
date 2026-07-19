//! The local test server the harness owns (spike-plan §0). One process, three listeners
//! (cleartext + two TLS certs), all on `127.0.0.1:0`. It is deliberately hand-rolled over
//! `std::net` so the harness controls raw socket behaviour a real HTTP library would hide:
//! stalling mid-body, truncating mid-body, presenting a specific certificate, and counting the
//! connections an endpoint sees (the no-hidden-retry control, rule 8).
//!
//! Endpoints (dispatched by path; query parsed where noted):
//! - `/ok` — constant `200` (rule 1 determinism baseline).
//! - `/echo` — `200`, echoes each request header back as `x-echo-<name>` (rule 6 runtime half).
//! - `/delay?ms=N` — sleep then `200`.
//! - `/chunked?count=N&delay_us=U` — `200` with a `Transfer-Encoding: chunked` body of N
//!   application chunks (`chunk-NNNNNN\n` per HTTP chunk), each flushed with `delay_us` between
//!   them so chunk boundaries arrive incrementally on the client. Drives the A1 response-streaming
//!   probe (step 25); paced by `delay_us` (0 = burst).
//! - `/stall` — `200` with `Content-Length: 1000`, a few bytes, then holds the socket open
//!   (bounded). Drives the deadline (rule 3) and cancellation (rule 9) syntheses.
//! - `/drip?count=N&interval_ms=M` — `200` announcing an `N`-byte body, then dribbling one byte
//!   every `M` ms so the connection is **never idle** longer than `M`. Distinguishes a *total*
//!   deadline (which must still fire) from a *per-idle* timeout (which a trickle keeps resetting) —
//!   the step-25 M4 deadline blind spot: `/stall`'s single burst-then-silence cannot tell them apart.
//! - `/truncate` — `200` with `Content-Length: 1000`, 10 bytes, then closes → `Transport`.
//! - `/flaky` — attempt 1 truncates, attempt ≥2 succeeds (the no-hidden-retry control, rule 8).
//! - `/etag` — `304` when `If-None-Match: "v1"`, else `200` + `ETag: "v1"` (rule 5).
//! - `/gzip` — `200`, `Content-Encoding: gzip`, gzipped payload (rule 7).
//! - `/unauthorized` — `401` (the 401 endpoint; a typed response, not an error key).
//! - `/redirect-insecure` — `302` to the cleartext base (rule 4, https→http refusal).
//! - `/redirect-chain?n=N` — `302` chain of length N, then `200` (redirect following).
//! - `/redirect-loop` — `302` to itself (TooManyRedirects, C2).

use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::wire;

/// The gzip payload the `/gzip` endpoint serves (decompressed). Rule 7 pins the decoded bytes.
pub const GZIP_PLAINTEXT: &[u8] =
    b"the quick brown fox jumps over the lazy dog, repeatedly and compressibly.";

/// A running test server. Dropping it shuts the listeners down.
pub struct TestServer {
    http_addr: SocketAddr,
    https_addr: SocketAddr,
    https_untrusted_addr: SocketAddr,
    good_spki: [u8; 32],
    untrusted_spki: [u8; 32],
    good_cert_der: Vec<u8>,
    shutdown: Arc<AtomicBool>,
    hits: Arc<Mutex<HashMap<String, usize>>>,
    handles: Vec<JoinHandle<()>>,
}

impl TestServer {
    /// Start the server. Errors only on cert generation / bind failure.
    pub fn start() -> Result<Self, ServerError> {
        wire::install_crypto_provider();
        // The certs carry a `127.0.0.1` IP SAN alongside the `localhost` DNS name: the socket mock's
        // verifier ignores hostnames (SPKI allowlist), but the reqwest reference adapter
        // (`bolted-http-linux`) does *real* rustls chain + hostname verification, and the endpoints
        // are `https://127.0.0.1:PORT`. Without the IP SAN, real hostname verification would reject
        // the loopback address (harness-mechanical; SPKI/pin values are unchanged by SANs).
        let sans = || vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let good = wire::generate_cert(sans()).map_err(|_| ServerError::Cert)?;
        let untrusted = wire::generate_cert(sans()).map_err(|_| ServerError::Cert)?;
        let good_spki = good.spki_sha256;
        let untrusted_spki = untrusted.spki_sha256;
        // The good cert DER, so a real adapter can add it as a trust anchor (real chain verification
        // needs the actual root, not only its SPKI hash — the mock trusts by SPKI allowlist instead).
        let good_cert_der = good.cert_der.as_ref().to_vec();

        let shutdown = Arc::new(AtomicBool::new(false));
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));

        let http = TcpListener::bind("127.0.0.1:0").map_err(|_| ServerError::Bind)?;
        let https = TcpListener::bind("127.0.0.1:0").map_err(|_| ServerError::Bind)?;
        let https_u = TcpListener::bind("127.0.0.1:0").map_err(|_| ServerError::Bind)?;
        let http_addr = http.local_addr().map_err(|_| ServerError::Bind)?;
        let https_addr = https.local_addr().map_err(|_| ServerError::Bind)?;
        let https_untrusted_addr = https_u.local_addr().map_err(|_| ServerError::Bind)?;

        let good_cfg = wire::server_config(good.cert_der, good.key_der).ok_or(ServerError::Cert)?;
        let untrusted_cfg =
            wire::server_config(untrusted.cert_der, untrusted.key_der).ok_or(ServerError::Cert)?;

        let handles = vec![
            spawn_accept(http, None, shutdown.clone(), hits.clone()),
            spawn_accept(https, Some(good_cfg), shutdown.clone(), hits.clone()),
            spawn_accept(https_u, Some(untrusted_cfg), shutdown.clone(), hits.clone()),
        ];

        Ok(TestServer {
            http_addr,
            https_addr,
            https_untrusted_addr,
            good_spki,
            untrusted_spki,
            good_cert_der,
            shutdown,
            hits,
            handles,
        })
    }

    /// The cleartext base URL (`http://127.0.0.1:PORT`).
    #[must_use]
    pub fn http_base(&self) -> String {
        format!("http://{}", self.http_addr)
    }

    /// The TLS base URL presenting the trusted (good) cert.
    #[must_use]
    pub fn https_base(&self) -> String {
        format!("https://{}", self.https_addr)
    }

    /// The TLS base URL presenting the untrusted cert (the `Tls` positive control).
    #[must_use]
    pub fn https_untrusted_base(&self) -> String {
        format!("https://{}", self.https_untrusted_addr)
    }

    /// The SHA-256 SPKI pin of the good cert (a caller pinning this succeeds).
    #[must_use]
    pub fn good_spki(&self) -> [u8; 32] {
        self.good_spki
    }

    /// The SHA-256 SPKI pin of the untrusted cert — a *wrong* pin for the good endpoint
    /// (pinning it against the good endpoint is the rule-10 mismatch).
    #[must_use]
    pub fn untrusted_spki(&self) -> [u8; 32] {
        self.untrusted_spki
    }

    /// The good cert, DER-encoded — a trust anchor a real adapter adds so its *real* chain
    /// verification accepts the (self-signed) test endpoint. The untrusted cert is deliberately
    /// never exposed this way: it must stay untrusted (the `Tls` positive control).
    #[must_use]
    pub fn good_cert_der(&self) -> &[u8] {
        &self.good_cert_der
    }

    /// How many connections a given path has seen (the no-hidden-retry control, rule 8).
    #[must_use]
    pub fn hits(&self, path: &str) -> usize {
        self.hits
            .lock()
            .ok()
            .and_then(|h| h.get(path).copied())
            .unwrap_or(0)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Unblock each blocked `accept()` with a throwaway connection.
        for addr in [self.http_addr, self.https_addr, self.https_untrusted_addr] {
            let _ = TcpStream::connect(addr);
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// Why the server failed to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Certificate generation failed.
    Cert,
    /// A listener could not bind.
    Bind,
}

fn spawn_accept(
    listener: TcpListener,
    tls: Option<Arc<rustls::ServerConfig>>,
    shutdown: Arc<AtomicBool>,
    hits: Arc<Mutex<HashMap<String, usize>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let tls = tls.clone();
            let hits = hits.clone();
            // Handlers are detached: /stall holds its socket, so joining would stall shutdown.
            thread::spawn(move || {
                handle_conn(stream, tls, &hits);
            });
        }
    })
}

fn handle_conn(
    tcp: TcpStream,
    tls: Option<Arc<rustls::ServerConfig>>,
    hits: &Mutex<HashMap<String, usize>>,
) {
    let _ = tcp.set_nodelay(true);
    match tls {
        None => serve(tcp, hits),
        Some(cfg) => {
            let Ok(conn) = rustls::ServerConnection::new(cfg) else {
                return;
            };
            let stream = rustls::StreamOwned::new(conn, tcp);
            serve(stream, hits);
        }
    }
}

fn serve(mut stream: impl std::io::Read + Write, hits: &Mutex<HashMap<String, usize>>) {
    let Ok((head, _leftover)) = wire::read_head(&mut stream) else {
        return;
    };
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    if req.parse(&head).is_err() {
        return;
    }
    let target = req.path.unwrap_or("/").to_string();
    let (path, query) = split_query(&target);

    if let Ok(mut map) = hits.lock() {
        *map.entry(path.to_string()).or_insert(0) += 1;
    }
    let attempt = hits
        .lock()
        .ok()
        .and_then(|m| m.get(path).copied())
        .unwrap_or(1);

    dispatch(&mut stream, path, &query, &req, attempt);
}

fn dispatch(
    stream: &mut impl Write,
    path: &str,
    query: &str,
    req: &httparse::Request,
    attempt: usize,
) {
    match path {
        "/ok" => write_response(stream, 200, "OK", &[], b"ok"),
        "/echo" => {
            let echoed: Vec<(String, Vec<u8>)> = req
                .headers
                .iter()
                .filter(|h| !h.name.is_empty())
                .map(|h| {
                    (
                        format!("x-echo-{}", h.name.to_ascii_lowercase()),
                        h.value.to_vec(),
                    )
                })
                .collect();
            let refs: Vec<(&str, &[u8])> = echoed
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_slice()))
                .collect();
            write_response(stream, 200, "OK", &refs, b"echo");
        }
        "/delay" => {
            let ms = query_int(query, "ms").unwrap_or(0);
            thread::sleep(Duration::from_millis(ms));
            write_response(stream, 200, "OK", &[], b"ok");
        }
        "/chunked" => {
            let count = query_int(query, "count").unwrap_or(0);
            let delay_us = query_int(query, "delay_us").unwrap_or(0);
            chunked(stream, count, delay_us);
        }
        "/stall" => stall(stream),
        "/drip" => {
            let count = query_int(query, "count").unwrap_or(0);
            let interval_ms = query_int(query, "interval_ms").unwrap_or(0);
            drip(stream, count, interval_ms);
        }
        "/truncate" => truncate(stream),
        "/flaky" => {
            if attempt <= 1 {
                truncate(stream);
            } else {
                write_response(stream, 200, "OK", &[], b"ok");
            }
        }
        "/etag" => {
            let matches = header_value(req, "if-none-match")
                .map(|v| v == b"\"v1\"")
                .unwrap_or(false);
            if matches {
                write_response(stream, 304, "Not Modified", &[("etag", b"\"v1\"")], b"");
            } else {
                write_response(stream, 200, "OK", &[("etag", b"\"v1\"")], b"etag-body");
            }
        }
        "/gzip" => match wire::gzip(GZIP_PLAINTEXT) {
            Ok(gz) => write_response(stream, 200, "OK", &[("content-encoding", b"gzip")], &gz),
            Err(_) => write_response(stream, 500, "Internal Server Error", &[], b""),
        },
        "/unauthorized" => write_response(
            stream,
            401,
            "Unauthorized",
            &[("www-authenticate", b"Basic realm=\"test\"")],
            b"",
        ),
        "/redirect-insecure" => {
            // Absolute cleartext target: the mock must refuse to follow https→http.
            let loc = query.strip_prefix("to=").unwrap_or("");
            write_response(stream, 302, "Found", &[("location", loc.as_bytes())], b"");
        }
        "/redirect-chain" => {
            let n = query_int(query, "n").unwrap_or(0);
            if n == 0 {
                write_response(stream, 200, "OK", &[], b"ok");
            } else {
                let loc = format!("/redirect-chain?n={}", n - 1);
                write_response(stream, 302, "Found", &[("location", loc.as_bytes())], b"");
            }
        }
        "/redirect-loop" => {
            write_response(
                stream,
                302,
                "Found",
                &[("location", b"/redirect-loop")],
                b"",
            );
        }
        _ => write_response(stream, 404, "Not Found", &[], b"not found"),
    }
}

/// Write a complete `Connection: close` response with a `Content-Length` body.
fn write_response(
    stream: &mut impl Write,
    code: u16,
    reason: &str,
    headers: &[(&str, &[u8])],
    body: &[u8],
) {
    let mut out = Vec::with_capacity(128 + body.len());
    out.extend_from_slice(format!("HTTP/1.1 {code} {reason}\r\n").as_bytes());
    for (name, value) in headers {
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    out.extend_from_slice(b"connection: close\r\n\r\n");
    out.extend_from_slice(body);
    let _ = stream.write_all(&out);
    let _ = stream.flush();
}

/// Stream `count` application chunks as a `Transfer-Encoding: chunked` body — one line
/// `chunk-NNNNNN\n` per HTTP chunk, flushed with `delay_us` between them so chunk boundaries
/// arrive incrementally on the client (the A1 response-streaming probe, step 25). `delay_us == 0`
/// writes them back-to-back (burst; the max drain-loop stress). Mirrors the step-24 S-FFI probe's
/// chunk server, now hosted in the harness-owned test server.
fn chunked(stream: &mut impl Write, count: u64, delay_us: u64) {
    let head = b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\n\
                 transfer-encoding: chunked\r\nconnection: close\r\n\r\n";
    if stream.write_all(head).is_err() {
        return;
    }
    let _ = stream.flush();
    for n in 1..=count {
        let line = format!("chunk-{n:06}\n");
        // One HTTP chunk = <hexlen>\r\n<data>\r\n.
        let framed = format!("{:X}\r\n{}\r\n", line.len(), line);
        if stream.write_all(framed.as_bytes()).is_err() {
            return;
        }
        let _ = stream.flush();
        if delay_us > 0 {
            thread::sleep(Duration::from_micros(delay_us));
        }
    }
    // The terminating zero-length chunk.
    let _ = stream.write_all(b"0\r\n\r\n");
    let _ = stream.flush();
}

/// Announce a 1000-byte body, send a few, then hold the socket open (bounded). The mock's watchdog
/// fires the deadline (or cancel) before this returns.
fn stall(stream: &mut impl Write) {
    let head = b"HTTP/1.1 200 OK\r\ncontent-length: 1000\r\nconnection: close\r\n\r\nstart";
    let _ = stream.write_all(head);
    let _ = stream.flush();
    // Bounded so a leaked handler thread cannot outlive the test process for long.
    thread::sleep(Duration::from_secs(30));
}

/// Announce an `count`-byte body, then dribble one byte every `interval_ms` (so the connection is
/// never idle for more than `interval_ms`). A *total* deadline must still fire mid-drip; a *per-idle*
/// timeout keeps getting reset by each byte and never fires — the two are indistinguishable on
/// `/stall` (one burst then silence), which is the step-25 M4 deadline blind spot. Bounded by
/// `count`; a client that cancels/times out closes the socket and the write loop exits.
fn drip(stream: &mut impl Write, count: u64, interval_ms: u64) {
    let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {count}\r\nconnection: close\r\n\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();
    for _ in 0..count {
        thread::sleep(Duration::from_millis(interval_ms));
        if stream.write_all(b"x").is_err() {
            return; // client gone (cancelled / deadline fired) — stop dribbling.
        }
        let _ = stream.flush();
    }
}

/// Announce a 1000-byte body, send 10, then close early → the mock sees `UnexpectedEof` → Transport.
fn truncate(stream: &mut impl Write) {
    let head = b"HTTP/1.1 200 OK\r\ncontent-length: 1000\r\nconnection: close\r\n\r\n0123456789";
    let _ = stream.write_all(head);
    let _ = stream.flush();
    // Return → socket drops → EOF at the client mid-body.
}

fn split_query(target: &str) -> (&str, String) {
    match target.split_once('?') {
        Some((p, q)) => (p, q.to_string()),
        None => (target, String::new()),
    }
}

fn query_int(query: &str, key: &str) -> Option<u64> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == key
        {
            return v.parse().ok();
        }
    }
    None
}

fn header_value<'a>(req: &'a httparse::Request, name: &str) -> Option<&'a [u8]> {
    req.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value)
}
