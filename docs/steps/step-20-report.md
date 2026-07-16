# Step 20 report — the Linux/systemd re-confirmation probe

**Status: done. No kill criterion hit; every probe row executed.** The daemon topology stands on
the second backend, the spike sources crossed byte-unmodified, and the activation seam got
*cheaper*: systemd's protocol needs zero foreign calls where launchd's needed exactly one.
Environment: Docker Desktop 29.6.1 (linux/arm64 VM on the M-series host), Debian bookworm image,
systemd 252 (252.39-1~deb12u2), rustc 1.95.0 in-container (matching the mise pin).

## The three questions, answered

1. **The topology stands under systemd as drawn.** Socket unit owns the listener, first connect
   spawns the service (L1), unit identity gives single-instance with no lock files (L2),
   `kill -9` → the next connect respawns through the socket unit (L3), idle-exit ⇄ reactivation
   loops cleanly (L4). The launchd bargain, re-purchased at the same rung-3 price: two small
   unit files instead of one plist.
2. **The activation seam is rung 3 on both OSes — but asymmetric in kind.** launchd:
   one documented C call (`launch_activate_socket`), no env protocol exists. systemd:
   one documented env protocol (`LISTEN_PID`/`LISTEN_FDS`, fds from 3 — sd_listen_fds(3)),
   no call needed; the adapter is ~40 lines of pure std, and its guards are ordinary unit tests.
   A future generated daemon shim must therefore be **per-OS in mechanism, identical in shape**
   (both hand the accept loop a `Vec<UnixListener>`); `serve_adopted` in `syncd/src/main.rs` is
   the shared shape, written once for both.
3. **The findings generalize — with one welcome asymmetry.** Open-then-verify is *still
   required* (a `connect(2)` that succeeds proves only that some init system holds a listener),
   but the systemd window behaved better than the launchd one step 19 observed: the post-`kill
   -9` connect was **accepted by the respawned daemon in ~45 ms** — systemd starts the service
   for the queued connection, so the client's blocking request simply completed. No unaccepted
   limbo was observed on this leg (n = a handful of scripted runs; the launchd limbo was also
   intermittent). Verdict for the design pass: **open-then-verify stays a portable client-library
   requirement** — connect success is never liveness under socket activation on either OS — but
   the reconnect loop's pathological case (ping timeout against a zombie connection) has only
   ever been seen under launchd. H6 (stash across daemon death) and idle-exit⇄reactivation
   re-confirmed identically (L5, L4).

## Probe matrix results

**P — portability**
- **P1 ok.** The spike crates' whole test surface — 13 suites, 32 tests, including step 18's
  B1–B4/E1/E2/F1 contract rows and both sides of the values-only discipline
  (`sync-wire/tests/values_only.rs`, planted control included) — passes on linux/arm64 with the
  sources **byte-unmodified**: `git diff` for `sync-wire`/`sync-settings` against main is empty.
- **P2 ok.** No `cfg` grew anywhere in the wire or the vehicle; the only platform-conditional
  code in the campaign is the launchd module in the `syncd` *bin* (now correctly gated — it
  couldn't even link on Linux before this step, which is itself evidence that nothing had ever
  needed it to).
- **P3 ok.** The systemd adapter's guards are unit tests; the fd adoption runs over a **real
  inherited descriptor without systemd** (`tests/systemd_activation.rs` stages sd_listen_fds by
  hand: dup2 onto fd 3 in `pre_exec`, `LISTEN_PID=$$ exec` in `/bin/sh` so exec preserves the
  pid). Runs everywhere `mise run check` runs. The pid-mismatch red was watched with the guard
  sabotaged before the green was trusted.

**L — lifecycle under systemd** (all inside the PID-1 container; `mise run test:os:systemd`)
- **L1 ok** — service inactive until the first connect; ping spawns it and pongs.
- **L2 ok** — a second `systemctl start` is a no-op (MainPID stable); a stray by-hand
  `syncd --systemd` refuses with "not socket-activated" (the guard, doing its job as the
  single-authority backstop).
- **L3 ok** — `kill -9` → immediate client connect **succeeds against the socket unit's
  listener and is accepted by the respawned daemon in 44–47 ms**; new MainPID; canonical state
  reset to fresh (the A3 assertion, re-run).
- **L4 ok** — `--idle-exit-secs 2` (injected via a drop-in `Environment=` override, no unit
  edit) exits the idle daemon; the socket unit stays listening; the next connect respawns.
- **L5 ok** — H6 on systemd: `f1-stash` → `kill -9` → `f1-restore` into the respawned daemon;
  dirty values back, verdict reset (the C20/C21 shape asserted by `syncctl`).

**D — chattiness on Linux** (in-container, debug builds, matching step 18's method)

| row | p50 | p95 |
|---|---|---|
| D1 ping | 41.8 µs | 50.2 µs |
| D2 try_set | 46.6 µs | 57.1 µs |
| D3 snapshot | 78.1 µs | 87.0 µs |
| D4 keystroke pair | **120.5 µs** | 135.1 µs |

Kill bar 2 (D4 p50 > 1000 µs): **cleared ~8×** — with the honest caveat that this is a Docker
VM on the macOS host (same arch, different kernel path). The macOS-native step-18 numbers were
26–45 µs; the ~3× in-container penalty is visible in D1's floor (41.8 µs vs step 18's ~13 µs
native ping), i.e. it is virtualization overhead on the syscall path, not core work.

## Findings for the design pass (adds to the step-18/19 lists)

1. **The activation adapters are asymmetric in kind, identical in shape.** macOS: one C call,
   no env protocol. Linux: one env protocol, no call. Both reduce to "hand the accept loop its
   listeners" — the topology, the wire, the daemon body, and the client are 100% shared. What a
   generator emits per OS is ~40 trivially-testable lines, not a port.
2. **Open-then-verify is portable; the launchd limbo is not (so far).** Connect success is never
   liveness on either OS. But systemd accepted the queued post-kill connect in ~45 ms across all
   observed runs, where launchd left step-19's appex connects unaccepted long enough to need the
   close-and-retry loop. Client libraries should ship open-then-verify unconditionally and treat
   the bounded-wait case as the good day, not the contract.
3. **systemd leaves the socket *file* behind** on `systemctl stop syncd.socket`
   (`RemoveOnStop=` defaults to off) — the stale-socket-file trap exists on this OS too, just
   owned by different code. Products that ever stop the socket unit want `RemoveOnStop=yes`.
4. **The `$HOME`-in-`SockPathName` wrinkle has no Linux twin** in the system-unit posture
   (`/run/syncd.sock` is absolute); the *user*-unit posture would use `%t` (=`$XDG_RUNTIME_DIR`),
   which systemd expands properly — specifiers are first-class in unit files where launchd's
   plist expansion was the step-19 wrinkle. Priced, not built (R4, below).
5. **The user-unit delta (R4, priced on paper).** A real per-user daemon is a user unit:
   `~/.config/systemd/user/` + the same two files with `ListenStream=%t/bolted/syncd.sock`,
   `WantedBy=sockets.target` in the *user* manager, started by any login session; headless
   machines need `loginctl enable-linger <user>` once. No approval ceremony exists at all — the
   SMAppService "0 prompts" finding is trivially matched. The container tier used system units
   only because a PID-1 container has no logind session; nothing in the daemon or units is
   posture-specific beyond the socket path.
6. **No sandbox pressure on this leg, confirmed by construction.** Nothing sandboxes a plain
   user process's Unix-socket connect on stock Debian; step 18's C rows have no Linux analog to
   run. (Flatpak/snap confinement would change this — out of scope, noted for whenever a
   packaged Linux GUI surface becomes real.)

## Measurements

| | |
|---|---|
| D rows | table above; bar cleared ~8× in-container |
| L3 respawn-accept latency | 44–47 ms (kill -9 → queued connect served) |
| `syncd` stripped (linux/arm64, release) | 725 704 B (macOS step-18 twin: 892 KB signed) |
| `test:os:linux` wall-clock | ~7 s warm; cold ≈ 3–4 min (image pull + full workspace build) |
| `test:os:systemd` wall-clock | ~7 s warm (incl. the 4 s L4 idle window) |
| New platform-specific daemon code | ~40 lines (the systemd module) + 2 unit files |

## Deviations (smallest-reversible, recorded)

- **`libc` entered as a dev-dependency of `syncd`** — `tests/systemd_activation.rs` needs one
  `dup2` in `pre_exec`, which std cannot express. Test-only; the daemon binary stays libc-free
  (transitively it always linked libc, as all std binaries do — the claim is about *our* code).
- **The lifecycle tier runs system units, not user units** — a PID-1 container has no logind
  session. The delta is priced in finding 5; the daemon cannot tell the difference.
- **Idle-exit injection via drop-in `Environment=SYNCD_ARGS=…`** rather than a second unit file
  or a unit edit — smallest thing that let L4 reuse the installed unit.
- **D rows measured in-container only.** No native Linux hardware was available this session;
  the numbers carry the VM caveat and the floor-attribution (D1) that keeps them honest.

## Friction log

- `systemctl is-system-running --wait` **races the bus** right after PID-1 start ("Failed to
  connect to bus") — the harness polls instead. First watched red of the tier, organically.
- `syncctl` prints `Pong` (Debug formatting); the harness grepped `pong`. Case bit once.
- `docker exec` without `-i` has **no stdin** — the L5 blob initially piped into a `read` that
  got instant EOF. Passed as argv instead. (Second organic red — the harness caught both.)
- **`dup2(3,3)` is a no-op that leaves CLOEXEC set** — when the staged listener happened to
  already *be* fd 3 (parallel-test fd layout), the exec killed it and the daemon silently
  served nothing; the ping timed out 10 s later. A real intermittent red (2-in-8), caught by
  `mise run check` post-M3 and fixed with the same clear-the-flag dance systemd's own fd
  staging does. The subtlety is inherent to the protocol's "fds start at 3" contract — worth
  remembering if a generated shim ever *stages* (not just adopts) these fds, e.g. in its tests.
- The launchd module having *never been compilable on Linux* went unnoticed through steps 18–19
  because nothing ever built the spike for Linux — exactly the kind of latent breakage a
  re-confirmation step exists to surface.

## Kill criteria — none hit

1. Wire/vehicle unedited (P1/P2) — not hit. 2. D4 p50 120.5 µs vs 1000 µs — not hit (and the
floor attribution shows the gap to native is syscall virtualization). 3. Pure-std adapter
shipped — not hit. 4. Single-instance is unit identity — not hit.

## Exit checklist

- `mise run check` green on macOS; the P3 tests ride it with no new external requirement.
- `git diff main -- crates/` empty; `sync-wire`/`sync-settings` byte-untouched.
- Every matrix row executed (no *not executed* rows this step — R1 held, so the container tier
  ran in full).
- Open-then-verify generalization: **empirical verdict recorded** (finding 2).
- ARCHITECTURE untouched. All three probes have now reported: **the topology design pass is
  unblocked.**
