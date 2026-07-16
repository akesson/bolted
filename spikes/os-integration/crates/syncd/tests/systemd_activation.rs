//! P3 — the systemd fd-passing seam, exercised over a REAL inherited descriptor without
//! systemd: stage the sd_listen_fds(3) protocol by hand. The listener is dup2'd onto fd 3 in
//! `pre_exec` (dup2 is async-signal-safe and clears CLOEXEC), and `/bin/sh` sets
//! `LISTEN_PID=$$` before `exec`ing the daemon — exec preserves the pid, so the guard sees its
//! own pid, exactly as systemd arranges it. Runs on any Unix, which is the point: the adapter's
//! correctness rides `mise run check` on every machine; only the lifecycle rows need Linux.

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use sync_wire::{Client, Request, Response};

fn temp_sock(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("syncd-sd-{tag}-{}.sock", std::process::id()))
}

#[test]
fn adopts_a_staged_listener_and_serves() {
    let path = temp_sock("ok");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind staging socket");
    let fd = listener.as_raw_fd();

    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(r#"LISTEN_PID=$$ LISTEN_FDS=1 exec "$0" --systemd"#)
        .arg(env!("CARGO_BIN_EXE_syncd"))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        // SAFETY: only async-signal-safe calls after fork; dup2 qualifies.
        cmd.pre_exec(move || {
            if libc::dup2(fd, 3) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn syncd under the staged protocol");

    // The socket is already bound (we bound it), so connect succeeds immediately and queues;
    // the round-trip below is the open-then-verify proof that the daemon actually adopted the
    // fd and is accepting on it.
    let mut c = Client::connect(&path).expect("connect");
    match c.request(Request::Ping).expect("ping over the adopted fd") {
        Response::Pong => {}
        other => panic!("expected Pong, got {other:?}"),
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn refuses_fds_meant_for_another_pid() {
    // No fd staging needed: the guard must fire before any adoption happens.
    let out = Command::new(env!("CARGO_BIN_EXE_syncd"))
        .arg("--systemd")
        .env("LISTEN_PID", "1")
        .env("LISTEN_FDS", "1")
        .output()
        .expect("run syncd");
    assert!(!out.status.success(), "must refuse a foreign LISTEN_PID");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("LISTEN_PID"), "stderr was: {err}");
}

#[test]
fn refuses_to_start_without_the_protocol() {
    let out = Command::new(env!("CARGO_BIN_EXE_syncd"))
        .arg("--systemd")
        .env_remove("LISTEN_PID")
        .env_remove("LISTEN_FDS")
        .output()
        .expect("run syncd");
    assert!(
        !out.status.success(),
        "must refuse when not socket-activated"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not socket-activated"), "stderr was: {err}");
}
