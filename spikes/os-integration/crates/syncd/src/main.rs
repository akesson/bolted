//! `syncd` binary: seed the canonical and serve until killed (or idle-exited).
//!
//! Three modes:
//! - `syncd --socket <path>` — bind the path itself (manual runs, the M2/M3 tiers).
//! - `syncd --launchd [--idle-exit-secs N]` (macOS) — adopt the listener(s) launchd holds for
//!   the `Listener` key in the agent plist (socket activation, step-18 row A1). launchd owns
//!   the socket file; the daemon only accepts.
//! - `syncd --systemd [--idle-exit-secs N]` (any Unix) — adopt the listener(s) systemd passes
//!   per sd_listen_fds(3): `LISTEN_PID`/`LISTEN_FDS` + inherited fds from 3 (step-20 row L1).
//!
//! With `--idle-exit-secs N` the adopted modes exit once no connection has been live for N
//! contiguous seconds — the init system respawns on the next connect, which is the whole
//! on-demand bargain.

use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// The one C call in the spike: adopting launchd's listener fds. This is the documented API for
/// socket activation (there is no environment-variable protocol on macOS), and the reason the
/// otherwise pure-Rust daemon's BIN carries an `unsafe extern` while the lib stays
/// `#![forbid(unsafe_code)]`. Recorded as a step-18 finding: "zero FFI" holds for the core and
/// the wire; the launchd seam costs exactly one foreign call.
#[cfg(target_os = "macos")]
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

/// systemd's socket-activation seam, for contrast: no foreign call at all. The protocol
/// (sd_listen_fds(3)) is pure environment + fd inheritance — `LISTEN_PID` names the intended
/// recipient, `LISTEN_FDS` counts already-listening fds handed over starting at fd 3. The
/// launchd/systemd asymmetry (one C call vs one env read) is step-20 evidence for whatever
/// activation shim a generator one day emits.
mod systemd {
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixListener;

    /// sd_listen_fds(3): the first passed descriptor (`SD_LISTEN_FDS_START`).
    const LISTEN_FDS_START: i32 = 3;

    /// The protocol guards, split from fd adoption so they are unit-testable without staging
    /// real descriptors (the adoption itself is exercised in `tests/systemd_activation.rs`).
    pub fn fd_count(
        listen_pid: Option<&str>,
        listen_fds: Option<&str>,
        my_pid: u32,
    ) -> Result<usize, String> {
        let Some(fds) = listen_fds else {
            return Err("LISTEN_FDS is unset — not socket-activated".to_string());
        };
        let pid = listen_pid.unwrap_or("");
        if pid.parse::<u32>() != Ok(my_pid) {
            return Err(format!(
                "LISTEN_PID={pid:?} is not this process (pid {my_pid}) — refusing fds meant \
                 for someone else"
            ));
        }
        match fds.parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            Ok(_) => Err("LISTEN_FDS=0 — activated with no sockets".to_string()),
            Err(_) => Err(format!("LISTEN_FDS={fds:?} is not a count")),
        }
    }

    /// Adopt the inherited listeners.
    pub fn activate() -> Result<Vec<UnixListener>, String> {
        let n = fd_count(
            std::env::var("LISTEN_PID").ok().as_deref(),
            std::env::var("LISTEN_FDS").ok().as_deref(),
            std::process::id(),
        )?;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            // SAFETY: the activation contract transfers ownership of fds 3..3+N to this
            // process, each a listening Unix socket, adopted exactly once.
            out.push(unsafe { UnixListener::from_raw_fd(LISTEN_FDS_START + i as i32) });
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
            // A stale socket file from a previous run refuses the bind; remove it. The adopted
            // modes never own the file and never do this.
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
        #[cfg(target_os = "macos")]
        Some("--launchd") => {
            let Ok(idle_secs) = idle_arg(&args[1..]) else {
                return usage();
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
            serve_adopted(listeners, idle_secs, "launchd")
        }
        Some("--systemd") => {
            let Ok(idle_secs) = idle_arg(&args[1..]) else {
                return usage();
            };
            let listeners = match systemd::activate() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("syncd: systemd activation refused: {e}");
                    return ExitCode::FAILURE;
                }
            };
            serve_adopted(listeners, idle_secs, "systemd")
        }
        _ => usage(),
    }
}

/// `[--idle-exit-secs N]` or nothing; anything else is a usage error.
fn idle_arg(rest: &[String]) -> Result<Option<u64>, ()> {
    match (rest.first().map(String::as_str), rest.get(1)) {
        (Some("--idle-exit-secs"), Some(n)) => n.parse().map(Some).map_err(|_| ()),
        (None, _) => Ok(None),
        _ => Err(()),
    }
}

/// Serve listeners handed over by an init system (shared by the launchd and systemd modes).
fn serve_adopted(
    listeners: Vec<UnixListener>,
    idle_secs: Option<u64>,
    origin: &'static str,
) -> ExitCode {
    let shared = syncd::new_shared();
    eprintln!(
        "syncd: adopted {} {origin} listener(s) (pid {})",
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
                    eprintln!("syncd: idle for {secs}s, exiting ({origin} respawns)");
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

fn usage() -> ExitCode {
    eprintln!(
        "usage: syncd --socket <path> | syncd --launchd|--systemd [--idle-exit-secs N] \
         (--launchd is macOS-only)"
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    // P3's guard cases. The fd adoption itself runs over a real inherited descriptor in
    // tests/systemd_activation.rs.
    use super::systemd;

    #[test]
    fn systemd_guard_refuses_unset_env() {
        let err = systemd::fd_count(None, None, 42).unwrap_err();
        assert!(err.contains("LISTEN_FDS"), "{err}");
    }

    #[test]
    fn systemd_guard_refuses_fds_meant_for_another_pid() {
        let err = systemd::fd_count(Some("41"), Some("1"), 42).unwrap_err();
        assert!(err.contains("LISTEN_PID"), "{err}");
        // Unset LISTEN_PID with LISTEN_FDS set is the same refusal, not an adoption.
        assert!(systemd::fd_count(None, Some("1"), 42).is_err());
    }

    #[test]
    fn systemd_guard_refuses_zero_and_garbage_counts() {
        assert!(systemd::fd_count(Some("42"), Some("0"), 42).is_err());
        assert!(systemd::fd_count(Some("42"), Some("many"), 42).is_err());
    }

    #[test]
    fn systemd_guard_accepts_the_protocol() {
        assert_eq!(systemd::fd_count(Some("42"), Some("2"), 42), Ok(2));
    }
}
