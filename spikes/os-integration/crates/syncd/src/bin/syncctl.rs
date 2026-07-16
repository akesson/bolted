//! `syncctl` — the Rust driver CLI for the scripted probes:
//!
//!   syncctl <socket> ping             liveness + one round-trip
//!   syncctl <socket> version          store version (A3 asserts it resets after kill -9)
//!   syncctl <socket> stats            draft counts (A3's "all pre-crash state is gone")
//!   syncctl <socket> toggle           drive a canonical change (C3's Rust-side actor)
//!   syncctl <socket> latency <iters>  row D from Rust: D1 ping, D2 try_set, D3 snapshot,
//!                                     D4 keystroke pair — p50/p95 in µs

use std::process::ExitCode;
use std::time::Instant;
use sync_wire::{Client, FieldName, RawWire, Request, Response};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (socket, verb) = match (args.first(), args.get(1)) {
        (Some(s), Some(v)) => (s.clone(), v.clone()),
        _ => {
            eprintln!("usage: syncctl <socket> <ping|version|stats|toggle|latency> [iters]");
            return ExitCode::from(64);
        }
    };
    let mut client = match Client::connect(std::path::Path::new(&socket)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("syncctl: connect {socket}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let outcome = match verb.as_str() {
        "ping" => run(&mut client, Request::Ping),
        "version" => run(&mut client, Request::Version),
        "stats" => run(&mut client, Request::Stats),
        "toggle" => run(&mut client, Request::TogglePaused),
        "latency" => {
            let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);
            return latency(&mut client, iters);
        }
        other => {
            eprintln!("syncctl: unknown verb {other}");
            return ExitCode::from(64);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn run(client: &mut Client, req: Request) -> Result<(), ()> {
    match client.request(req) {
        Ok(resp) => {
            println!("{resp:?}");
            Ok(())
        }
        Err(e) => {
            eprintln!("syncctl: {e}");
            Err(())
        }
    }
}

fn percentiles(label: &str, mut samples: Vec<u128>) {
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    println!(
        "{label} n={} p50_us={} p95_us={}",
        samples.len(),
        p50 as f64 / 1000.0,
        p95 as f64 / 1000.0
    );
}

fn measure(n: usize, mut op: impl FnMut(usize) -> bool) -> Result<Vec<u128>, ()> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t0 = Instant::now();
        if !op(i) {
            eprintln!("syncctl: latency op failed at iteration {i}");
            return Err(());
        }
        out.push(t0.elapsed().as_nanos());
    }
    Ok(out)
}

fn latency(client: &mut Client, iters: usize) -> ExitCode {
    let draft = match client.request(Request::Checkout) {
        Ok(Response::DraftId { draft }) => draft,
        other => {
            eprintln!("syncctl: checkout failed: {other:?}");
            return ExitCode::FAILURE;
        }
    };
    let ping = |c: &mut Client| matches!(c.request(Request::Ping), Ok(Response::Pong));
    let set = |c: &mut Client, i: usize| {
        matches!(
            c.request(Request::TrySet {
                draft,
                field: FieldName::Label,
                value: RawWire::Text(format!("Rust-{i}")),
            }),
            Ok(Response::SetOutcome { error: None })
        )
    };
    let snap = |c: &mut Client| {
        matches!(
            c.request(Request::DraftSnapshot { draft }),
            Ok(Response::Draft { .. })
        )
    };

    // Warm-up, then the four D rows.
    let Ok(_) = measure(100, |_| ping(client)) else {
        return ExitCode::FAILURE;
    };
    let rows: [(&str, Result<Vec<u128>, ()>); 4] = [
        ("D1_ping", measure(iters, |_| ping(client))),
        ("D2_try_set", measure(iters, |i| set(client, i))),
        ("D3_snapshot", measure(iters, |_| snap(client))),
        (
            "D4_keystroke_pair",
            measure(iters, |i| set(client, i) && snap(client)),
        ),
    ];
    for (label, samples) in rows {
        match samples {
            Ok(s) => percentiles(label, s),
            Err(()) => return ExitCode::FAILURE,
        }
    }
    println!("LATENCY-OK");
    ExitCode::SUCCESS
}
