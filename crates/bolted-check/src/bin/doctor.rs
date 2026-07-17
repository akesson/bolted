//! `mise run doctor` — assemble the real [`Machine`] and print the report. All judgement lives
//! in `bolted_check::doctor` (pure, unit-tested); this main only reads the process environment
//! and runs the one subprocess probe. Always exits 0: doctor warns, never fails (VISION risk 5).

use bolted_check::doctor::{self, Machine};
use std::path::PathBuf;

fn main() {
    let path = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let machine = Machine {
        android_home: std::env::var_os("ANDROID_HOME").map(PathBuf::from),
        cargo_home: std::env::var_os("CARGO_HOME").map(PathBuf::from),
        boltffi_version: probe_boltffi(&path, &home),
        path,
        home,
    };
    print!("{}", doctor::render(&doctor::evaluate(&machine)));
}

/// Run `boltffi --version` and hand back its trimmed stdout. The lookup extends `$PATH` with
/// `${CARGO_HOME:-$HOME/.cargo}/bin` exactly as the task guards do, so doctor and the tasks see
/// the same binary. `None` = not found (or it failed to run), which the row reports as missing.
fn probe_boltffi(path: &str, home: &std::path::Path) -> Option<String> {
    let machine_paths = Machine {
        path: String::new(),
        home: home.to_path_buf(),
        android_home: None,
        cargo_home: std::env::var_os("CARGO_HOME").map(PathBuf::from),
        boltffi_version: None,
    };
    let extended = format!("{}:{path}", machine_paths.cargo_bin().display());
    let bin = doctor::find_executable("boltffi", &extended)?;
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}
