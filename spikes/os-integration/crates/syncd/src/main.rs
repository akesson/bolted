//! `syncd` binary: bind a Unix socket, seed the canonical, serve until killed.
//!
//! M2 mode: `syncd --socket <path>` binds the path itself (manual runs, integration tests).
//! M4 adds launchd socket activation alongside.

use std::os::unix::net::UnixListener;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = match (args.next().as_deref(), args.next()) {
        (Some("--socket"), Some(p)) => p,
        _ => {
            eprintln!("usage: syncd --socket <path>");
            return ExitCode::FAILURE;
        }
    };

    // A stale socket file from a previous run refuses the bind; remove it. launchd mode (M4)
    // never owns the file and never does this.
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("syncd: cannot bind {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let shared = syncd::new_shared();
    eprintln!("syncd: listening on {path} (pid {})", std::process::id());
    syncd::serve(listener, shared);
    ExitCode::SUCCESS
}
