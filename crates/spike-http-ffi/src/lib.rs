//! spike-http-ffi — step-02 third-cluster probe (capability adapter packaging).
//!
//! Probes, per `crates/bolted-http/docs/architecture.md` §4:
//! 1. packaging: generated bindings + hand-written `BoltedHttp.swift` in ONE Swift package
//! 2. the capability round-trip: core → callback trait → URLSession → typed input back in
//! 3. measurements: callback overhead, payload cost, completion thread
//! 4. error taxonomy: timeout / DNS / TLS → typed keys
//!
//! This is spike code, not bolted-core: `std::time` and locking are allowed here.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
