//! `syncd` binary: seed the canonical and serve until killed (or idle-exited).
//!
//! Two modes:
//! - `syncd --socket <path>` — bind the path itself (manual runs, the M2/M3 tiers).
//! - `syncd --launchd [--idle-exit-secs N]` — adopt the listener(s) launchd holds for the
//!   `Listener` key in the agent plist (socket activation, probe row A1). launchd owns the
//!   socket file; the daemon only accepts. With `--idle-exit-secs N` the process exits once no
//!   connection has been live for N contiguous seconds (A4) — launchd respawns it on the next
//!   connect, which is the whole on-demand bargain.

use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// The one C call in the spike: adopting launchd's listener fds. This is the documented API for
/// socket activation (there is no environment-variable protocol on macOS), and the reason the
/// otherwise pure-Rust daemon's BIN carries an `unsafe extern` while the lib stays
/// `#![forbid(unsafe_code)]`. Recorded as a step-18 finding: "zero FFI" holds for the core and
/// the wire; the launchd seam costs exactly one foreign call.
mod launchd {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixListener;

    unsafe extern "C" {
        fn launch_activate_socket(
            name: *const c_char,
            fds: *mut *mut c_int,
            cnt: *mut usize,
        ) -> c_int;
        fn free(p: *mut c_void);
    }

    /// The fds launchd holds for the named `Sockets` entry, as listeners. `Err` carries the
    /// launchd errno (ENOENT = no such key / not launched by launchd, ESRCH = not managed).
    pub fn activate(name: &str) -> Result<Vec<UnixListener>, i32> {
        let Ok(cname) = CString::new(name) else {
            return Err(-1);
        };
        let mut fds: *mut c_int = std::ptr::null_mut();
        let mut cnt: usize = 0;
        // SAFETY: the API contract is "on success, *fds is a malloc'd array of cnt fds the
        // caller owns"; each fd is a listening socket we adopt exactly once.
        let rc = unsafe { launch_activate_socket(cname.as_ptr(), &mut fds, &mut cnt) };
        if rc != 0 {
            return Err(rc);
        }
        let mut out = Vec::with_capacity(cnt);
        for i in 0..cnt {
            let fd = unsafe { *fds.add(i) };
            out.push(unsafe { UnixListener::from_raw_fd(fd) });
        }
        if !fds.is_null() {
            unsafe { free(fds.cast()) };
        }
        Ok(out)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("--socket") => {
            let Some(path) = args.get(1) else {
                return usage();
            };
            // A stale socket file from a previous run refuses the bind; remove it. launchd mode
            // never owns the file and never does this.
            let _ = std::fs::remove_file(path);
            let listener = match UnixListener::bind(path) {
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
        Some("--launchd") => {
            let idle_secs: Option<u64> = match (args.get(1).map(String::as_str), args.get(2)) {
                (Some("--idle-exit-secs"), Some(n)) => n.parse().ok(),
                (None, _) => None,
                _ => return usage(),
            };
            let listeners = match launchd::activate("Listener") {
                Ok(l) if !l.is_empty() => l,
                Ok(_) => {
                    eprintln!("syncd: launchd handed us zero sockets for key 'Listener'");
                    return ExitCode::FAILURE;
                }
                Err(e) => {
                    eprintln!(
                        "syncd: launch_activate_socket failed (errno {e}) — not launched by \
                         launchd, or the plist has no 'Listener' socket"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let shared = syncd::new_shared();
            eprintln!(
                "syncd: adopted {} launchd listener(s) (pid {})",
                listeners.len(),
                std::process::id()
            );

            if let Some(secs) = idle_secs {
                let shared = std::sync::Arc::clone(&shared);
                // Wall-clock is legitimate here: idle-exit is daemon lifecycle, not core state.
                #[allow(clippy::disallowed_methods)]
                std::thread::spawn(move || {
                    let mut idle_since = Instant::now();
                    loop {
                        std::thread::sleep(Duration::from_millis(500));
                        if syncd::connection_count(&shared) > 0 {
                            idle_since = Instant::now();
                        } else if idle_since.elapsed() >= Duration::from_secs(secs) {
                            eprintln!("syncd: idle for {secs}s, exiting (launchd respawns)");
                            std::process::exit(0);
                        }
                    }
                });
            }

            let mut threads = Vec::new();
            for listener in listeners {
                let shared = std::sync::Arc::clone(&shared);
                threads.push(std::thread::spawn(move || syncd::serve(listener, shared)));
            }
            for t in threads {
                let _ = t.join();
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: syncd --socket <path> | syncd --launchd [--idle-exit-secs N]");
    ExitCode::FAILURE
}
