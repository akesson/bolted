//! `syncctl` — the Rust driver CLI for the scripted probes:
//!
//!   syncctl <socket> ping             liveness + one round-trip
//!   syncctl <socket> version          store version (A3 asserts it resets after kill -9)
//!   syncctl <socket> stats            draft counts (A3's "all pre-crash state is gone")
//!   syncctl <socket> toggle           drive a canonical change (C3's Rust-side actor)
//!   syncctl <socket> latency <iters>  row D from Rust: D1 ping, D2 try_set, D3 snapshot,
//!                                     D4 keystroke pair — p50/p95 in µs
//!   syncctl <socket> f1-stash         checkout, dirty two fields, pass the check, print the
//!                                     stash as one JSON line (the client-side survival blob)
//!   syncctl <socket> f1-restore <j>   restore the blob into (a fresh) daemon and assert the
//!                                     C20/C21 shape: dirty values back, verdict reset

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
        "f1-stash" => return f1_stash(&mut client),
        "f1-restore" => {
            let Some(blob) = args.get(2) else {
                eprintln!("usage: syncctl <socket> f1-restore <stash-json>");
                return ExitCode::from(64);
            };
            return f1_restore(&mut client, blob);
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

/// F1's first half: a draft with two dirty fields and a PASSED check, flattened. The passed
/// verdict is the point — restore must NOT carry it (C20).
fn f1_stash(client: &mut Client) -> ExitCode {
    let draft = match client.request(Request::Checkout) {
        Ok(Response::DraftId { draft }) => draft,
        other => {
            eprintln!("syncctl: checkout failed: {other:?}");
            return ExitCode::FAILURE;
        }
    };
    let sets = [
        (FieldName::Label, "Survives-the-daemon"),
        (FieldName::Folder, "/Users/Shared/F1"),
    ];
    for (field, value) in sets {
        match client.request(Request::TrySet {
            draft,
            field,
            value: RawWire::Text(value.to_string()),
        }) {
            Ok(Response::SetOutcome { error: None }) => {}
            other => {
                eprintln!("syncctl: set {field:?} failed: {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    let token = match client.request(Request::BeginCheck {
        draft,
        check: sync_wire::CheckName::FolderReachable,
    }) {
        Ok(Response::CheckBegun { token }) => token,
        other => {
            eprintln!("syncctl: begin_check failed: {other:?}");
            return ExitCode::FAILURE;
        }
    };
    match client.request(Request::CompleteCheck {
        draft,
        check: sync_wire::CheckName::FolderReachable,
        token,
        ok: true,
    }) {
        Ok(Response::CheckSettled { accepted: true }) => {}
        other => {
            eprintln!("syncctl: complete_check failed: {other:?}");
            return ExitCode::FAILURE;
        }
    }
    match client.request(Request::Stash { draft }) {
        Ok(Response::Stashed { stash }) => match serde_json::to_string(&stash) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("syncctl: stash encode: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("syncctl: stash failed: {other:?}");
            ExitCode::FAILURE
        }
    }
}

/// F1's second half, against the RESPAWNED daemon: the dirty values are back, and the verdict is
/// not — the dirty folder demands a fresh check (C16 after C20).
fn f1_restore(client: &mut Client, blob: &str) -> ExitCode {
    let stash: sync_wire::StashWire = match serde_json::from_str(blob) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("syncctl: stash parse: {e}");
            return ExitCode::FAILURE;
        }
    };
    let draft = match client.request(Request::Restore { stash }) {
        Ok(Response::DraftId { draft }) => draft,
        other => {
            eprintln!("syncctl: restore failed: {other:?}");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = match client.request(Request::DraftSnapshot { draft }) {
        Ok(Response::Draft { snapshot }) => snapshot,
        other => {
            eprintln!("syncctl: snapshot failed: {other:?}");
            return ExitCode::FAILURE;
        }
    };
    let label_ok = snapshot.label.raw == Some(RawWire::Text("Survives-the-daemon".to_string()))
        && snapshot.label.dirty;
    let check_reset = snapshot
        .report
        .rule_errors
        .iter()
        .any(|r| r.error.key == "folder_check_required");
    if !label_ok {
        eprintln!(
            "syncctl: F1 FAILED — dirty label did not survive: {:?}",
            snapshot.label
        );
        return ExitCode::FAILURE;
    }
    if !check_reset {
        eprintln!(
            "syncctl: F1 FAILED — the pre-death PASSED verdict survived the restore (C20 broken): {:?}",
            snapshot.report
        );
        return ExitCode::FAILURE;
    }
    println!(
        "F1-OK label_survived verdict_reset base_version={}",
        snapshot.base_version
    );
    ExitCode::SUCCESS
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
