//! spike-http-ffi — step-02 third-cluster probe (capability adapter packaging).
//!
//! Probes, per `crates/bolted-http/docs/architecture.md` §4:
//! 1. packaging: generated bindings + hand-written `BoltedHttp.swift` in ONE Swift package
//! 2. the capability round-trip: core → callback trait → URLSession → typed input back in
//! 3. measurements: callback overhead, payload cost, completion thread
//! 4. error taxonomy: timeout / DNS / TLS → typed keys
//!
//! This is spike code, not bolted-core: `std::time` and locking are allowed here.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use boltffi::*;

/// A typed HTTP request effect. The `token` is the single-flight identity: the
/// completion must come back carrying the same token to be accepted.
#[data]
pub struct HttpRequest {
    pub token: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<HttpHeader>,
    pub body: Vec<u8>,
    /// One total deadline in milliseconds — the only timeout in the portable contract.
    pub deadline_ms: u64,
}

#[data]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// A typed HTTP response, re-entering the core as an input.
#[data]
pub struct HttpResponse {
    pub token: u64,
    pub status: u16,
    pub headers: Vec<HttpHeader>,
    pub body: Vec<u8>,
    /// Redirects are followed by the stack; the final URL is reported.
    pub final_url: String,
}

/// Typed error keys + params — never strings. Payload-carrying #[data] enum:
/// whether this crosses cleanly is itself a probe finding.
#[data]
#[derive(Clone)]
pub enum HttpError {
    Timeout { deadline_ms: u64 },
    DnsFailure { host: String },
    TlsFailure { reason: String },
    Transport { code: i64, message: String },
}

/// The outcome of a request as recorded by the core after the completion input.
#[data]
#[derive(Clone)]
pub enum HttpOutcome {
    Pending,
    Succeeded { status: u16, body_len: u64, final_url: String },
    Failed { error: HttpError },
}

/// The capability trait: implemented in hand-written Swift (BoltedHttp.swift) over
/// URLSession. The core calls `execute`; the adapter later delivers the completion
/// by calling back into `SpikeCore::complete_ok` / `complete_err`.
#[export]
pub trait HttpAdapter: Send + Sync {
    fn execute(&self, request: HttpRequest);
    /// No-op used to measure raw callback-trait call overhead (Rust → Swift).
    fn ping(&self, n: u64) -> u64;
}

struct Flight {
    token: u64,
    outcome: HttpOutcome,
}

pub struct SpikeCore {
    adapter: Arc<dyn HttpAdapter>,
    next_token: AtomicU64,
    flights: Mutex<Vec<Flight>>,
}

#[export]
impl SpikeCore {
    pub fn new(adapter: Arc<dyn HttpAdapter>) -> Self {
        SpikeCore {
            adapter,
            next_token: AtomicU64::new(1),
            flights: Mutex::new(Vec::new()),
        }
    }

    /// Emit a typed HttpRequest effect to the adapter. Returns the flight token.
    pub fn fetch(&self, url: String, deadline_ms: u64) -> u64 {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut flights) = self.flights.lock() {
            flights.push(Flight { token, outcome: HttpOutcome::Pending });
        }
        self.adapter.execute(HttpRequest {
            token,
            method: "GET".to_owned(),
            url,
            headers: vec![HttpHeader { name: "accept".to_owned(), value: "*/*".to_owned() }],
            body: Vec::new(),
            deadline_ms,
        });
        token
    }

    /// Completion re-entering the core as a typed input (success path).
    /// A stale/unknown token is rejected — the single-flight guarantee.
    pub fn complete_ok(&self, response: HttpResponse) {
        let Ok(mut flights) = self.flights.lock() else { return };
        match flights.iter_mut().find(|f| f.token == response.token) {
            Some(flight) if matches!(flight.outcome, HttpOutcome::Pending) => {
                flight.outcome = HttpOutcome::Succeeded {
                    status: response.status,
                    body_len: response.body.len() as u64,
                    final_url: response.final_url,
                };
            }
            // Single-flight: the first completion wins; late/unknown tokens are dropped.
            Some(_) | None => {}
        }
    }

    /// Completion re-entering the core as a typed input (failure path).
    pub fn complete_err(&self, token: u64, error: HttpError) {
        let Ok(mut flights) = self.flights.lock() else { return };
        match flights.iter_mut().find(|f| f.token == token) {
            Some(flight) if matches!(flight.outcome, HttpOutcome::Pending) => {
                flight.outcome = HttpOutcome::Failed { error };
            }
            Some(_) | None => {}
        }
    }

    /// Test hook: observe the recorded outcome for a token.
    pub fn outcome(&self, token: u64) -> HttpOutcome {
        let Ok(flights) = self.flights.lock() else { return HttpOutcome::Pending };
        flights
            .iter()
            .find(|f| f.token == token)
            .map(|f| f.outcome.clone())
            .unwrap_or(HttpOutcome::Pending)
    }

    /// Measurement: Rust → Swift callback-trait overhead. Calls `ping` `iterations`
    /// times and returns total elapsed nanoseconds (spike code — timing allowed).
    pub fn measure_ping(&self, iterations: u64) -> u64 {
        let start = std::time::Instant::now();
        let mut acc = 0u64;
        for i in 0..iterations {
            acc = acc.wrapping_add(self.adapter.ping(i));
        }
        let elapsed = start.elapsed().as_nanos() as u64;
        // keep `acc` observable so the loop cannot be optimized away
        elapsed.max(acc.min(1))
    }

    /// Measurement: Swift → Rust no-op call (timed from the Swift side).
    pub fn noop(&self) {}

    /// Measurement: payload cost — bytes across the boundary and back.
    pub fn echo_len(&self, payload: Vec<u8>) -> u64 {
        payload.len() as u64
    }
}

// =================================================================================================
// S-FFI (step-24 M2): response-streaming mechanism probe — the step-02 stream shapes re-run
// INSIDE an http round-trip at boltffi 0.27.5. Row-16 (feature-matrix §5.11) gate.
//
// Flow (mirrors bolted-http's real response-streaming shape):
//   localhost HTTP server (chunked body) → URLSession consumes it on the Swift side → the Swift
//   adapter pushes each chunk ACROSS the FFI into `StreamProbe` → the core re-delivers to a LIVE
//   Swift consumer through one of three mechanisms:
//     F1  ffi_stream async push  — the exact shape that stalled 15/100 on 0.27.3
//     F2  callback-trait push    — the capability-callback machinery (~8 ns/call)
//     F3  wake-and-read batch pull — capacity-1 wake stream + a drained getter
//
// This is spike code (std::time / std::net / threads allowed; not bolted-core).
// =================================================================================================

/// One response-body chunk crossing the FFI. `t_send_ns` is stamped by Swift with
/// `DispatchTime.now().uptimeNanoseconds` immediately before the deliver call, so the consumer
/// can compute per-chunk delivery latency without a cross-language clock.
#[data]
#[derive(Clone)]
pub struct Chunk {
    pub token: u64,
    pub seq: u64,
    pub bytes: Vec<u8>,
    pub t_send_ns: u64,
    pub last: bool,
}

/// F2 mechanism: the consumer implements this in Swift and registers it; the core pushes each
/// chunk through it synchronously (same generated machinery as `HttpAdapter`/capabilities).
#[export]
pub trait ChunkSink: Send + Sync {
    fn on_chunk(&self, chunk: Chunk);
}

/// The response-streaming probe core. One instance per mechanism run.
pub struct StreamProbe {
    /// F1 — ffi_stream async push. Capacity 256: the incremental-live-consumer shape from the
    /// step-02 report's `testC2bIncrementalDefaultCapacityStallProbe` (the 15/100 stall case).
    f1: Arc<EventSubscription<Chunk>>,
    /// F3 — capacity-1 wake stream (version/seq numbers; drops harmless by construction).
    f3_wakes: Arc<EventSubscription<u64>>,
    /// F3 — the buffer the wake tells the consumer to drain via `drain_f3()`.
    f3_buf: Mutex<Vec<Chunk>>,
    /// F2 — the registered callback sink.
    sink: Mutex<Option<Arc<dyn ChunkSink>>>,
    /// How many chunks entered the core (completeness numerator source-of-truth).
    ingested: AtomicU64,
}

#[export]
impl StreamProbe {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        StreamProbe {
            f1: Arc::new(EventSubscription::new(256)),
            f3_wakes: Arc::new(EventSubscription::new(1)),
            f3_buf: Mutex::new(Vec::new()),
            sink: Mutex::new(None),
            ingested: AtomicU64::new(0),
        }
    }

    /// F2 wiring: register the Swift-side sink.
    pub fn set_sink(&self, sink: Arc<dyn ChunkSink>) {
        if let Ok(mut s) = self.sink.lock() {
            *s = Some(sink);
        }
    }

    /// F1 deliver: adapter → core → ffi_stream push out to the live consumer.
    pub fn deliver_f1(&self, chunk: Chunk) {
        self.ingested.fetch_add(1, Ordering::Relaxed);
        self.f1.push_event(chunk);
    }

    /// F2 deliver: adapter → core → callback-trait push (synchronous, producer thread).
    pub fn deliver_f2(&self, chunk: Chunk) {
        self.ingested.fetch_add(1, Ordering::Relaxed);
        let sink = self.sink.lock().ok().and_then(|s| s.clone());
        if let Some(sink) = sink {
            sink.on_chunk(chunk);
        }
    }

    /// F3 deliver: adapter → core → buffer + capacity-1 wake. The consumer drains on wake.
    pub fn deliver_f3(&self, chunk: Chunk) {
        self.ingested.fetch_add(1, Ordering::Relaxed);
        let seq = chunk.seq;
        if let Ok(mut buf) = self.f3_buf.lock() {
            buf.push(chunk);
        }
        self.f3_wakes.push_event(seq);
    }

    /// F3 read half: drain everything buffered since the last drain.
    pub fn drain_f3(&self) -> Vec<Chunk> {
        match self.f3_buf.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => Vec::new(),
        }
    }

    /// Completeness source-of-truth: chunks that entered the core.
    pub fn ingested(&self) -> u64 {
        self.ingested.load(Ordering::Relaxed)
    }

    #[ffi_stream(item = Chunk)]
    pub fn f1_stream(&self) -> Arc<EventSubscription<Chunk>> {
        Arc::clone(&self.f1)
    }

    #[ffi_stream(item = u64)]
    pub fn f3_wake_stream(&self) -> Arc<EventSubscription<u64>> {
        Arc::clone(&self.f3_wakes)
    }

    /// Spin a localhost HTTP/1.1 server that streams `chunks` application chunks as a
    /// `Transfer-Encoding: chunked` body — one line `chunk-NNNNNN\n` per HTTP chunk, flushed
    /// with `delay_us` between them to force incremental arrival on the foreign side. Returns
    /// the URL. The listener serves every connection it accepts (re-runnable) on a detached
    /// thread; the probe process owns it for the test's lifetime.
    pub fn start_chunk_server(&self, chunks: u32, delay_us: u64) -> String {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(_) => return String::new(),
        };
        let port = match listener.local_addr() {
            Ok(a) => a.port(),
            Err(_) => return String::new(),
        };
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Read (and discard) the request headers.
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
                             Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                if stream.write_all(head).is_err() {
                    continue;
                }
                let _ = stream.flush();
                let mut ok = true;
                for n in 1..=chunks {
                    let line = format!("chunk-{n:06}\n");
                    // One HTTP chunk = <hexlen>\r\n<data>\r\n
                    let framed = format!("{:X}\r\n{}\r\n", line.len(), line);
                    if stream.write_all(framed.as_bytes()).is_err() {
                        ok = false;
                        break;
                    }
                    let _ = stream.flush();
                    if delay_us > 0 {
                        thread::sleep(std::time::Duration::from_micros(delay_us));
                    }
                }
                if ok {
                    let _ = stream.write_all(b"0\r\n\r\n");
                    let _ = stream.flush();
                }
            }
        });
        format!("http://127.0.0.1:{port}/stream")
    }
}
