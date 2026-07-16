# Step 20 — OS-integration spike III: the Linux/systemd re-confirmation probe

**Phase 5 · OS-integration spike. Status: ready.** Read first: [VISION.md](../VISION.md) (bet 2:
*"real daemons under launchd/systemd"* — this step is the second half of that sentence),
[ARCHITECTURE.md](../ARCHITECTURE.md) (§9's process-topology bullet), [ROADMAP.md](../ROADMAP.md)
(Phase 5 campaign sketch), and the two banked evidence logs this step exists to re-test:
[step-18-report.md](step-18-report.md) and [step-19-report.md](step-19-report.md).

## Goal

The step-05 move: before the topology design pass freezes anything, re-confirm the daemon-owned
topology on the second, **structurally different** backend — systemd instead of launchd, a plain
Linux filesystem instead of app-group containers, no sandbox pressure at all. Steps 18/19 proved
the topology on one OS; a design pass fed by one OS would freeze launchd folklore as law. Three
questions:

1. **Does the topology stand under systemd as drawn?** Socket unit owns the listener, service
   spawns on first connect, unit identity gives single-instance, `Restart=` owns crash-respawn —
   the launchd bargain, re-priced on the other init system.
2. **Is the activation seam still rung 3 — and is it *cheaper*?** launchd cost the otherwise
   pure-Rust daemon exactly one foreign call (`launch_activate_socket`, a step-18 finding).
   systemd's protocol is documented as environment variables + inherited fds (`LISTEN_PID`,
   `LISTEN_FDS`, fds from 3) — implementable in pure std, no libsystemd. If true, the two
   activation adapters are *asymmetric in kind* (C call vs env protocol), which is precisely what
   a future generated daemon shim needs to know. If false, record what systemd actually demands.
3. **Do the campaign's portable-looking findings generalize?** Above all **open-then-verify**
   (step 19 finding 1: `connect(2)` success ≠ daemon liveness under socket activation) — if the
   same unaccepted-backlog window exists under systemd, it is a portable requirement for the
   generated client library; if not, it is launchd-specific lore and must be labeled as such.
   Likewise H6 (stash across daemon death) and the idle-exit⇄reactivation loop.

As in steps 02/05/18: **a green suite is not the deliverable; evidence is** — the answered probe
matrix, the numbers, the friction log in `docs/steps/step-20-report.md`. This probe is
**headless** (the step-18 shape, not the step-19 shape): no UI, no file-manager integration.

## What the planning pass verified (by reading the code, 2026-07-16)

- **The probe suite looks portable — verify that it is.** `syncd/tests/probe.rs` (rows B1–B4, E1,
  E2, F1) drives `--socket` mode through `std::os::unix::net` only; `sync-wire` and
  `sync-settings` have zero platform-conditional code. Expected to pass on Linux **unmodified**;
  that expectation is M1's row P1, not a fact.
- **`syncd` cannot link on Linux today.** The launchd module in `syncd/src/main.rs` declares and
  *calls* `launch_activate_socket` unconditionally — a symbol that exists in no Linux libc. M0
  gates it `#[cfg(target_os = "macos")]` and adds the `--systemd` twin. This is spike code; the
  step-18 non-goal ("no framework-crate changes") still binds `crates/` byte-for-byte.
- **The systemd fd protocol is fakeable on any Unix.** `LISTEN_PID`/`LISTEN_FDS` + fd 3 can be
  staged from a test on macOS (bind a listener, dup2 to fd 3, set the env, spawn). So the
  adapter's correctness rides `mise run check` on every machine — only the *lifecycle* rows need
  real systemd.
- **The measurement client already exists.** `syncctl` prints D1–D4 p50/p95 (step-18 row D); on
  Linux it reruns as-is.
- **Docker is present** (`docker` CLI, linux/arm64 server). Whether it can host a systemd-PID-1
  container on this machine is recon R1, **not** a fact.

## Reconnaissance to falsify (treat every claim as a hypothesis)

- **R1 — a systemd-as-PID-1 container runs under this Docker.** Modern Docker + cgroup v2 is
  supposed to run e.g. a Debian image with systemd as the entrypoint given `--privileged` (or
  targeted cgroup mounts). If refuted within the time-box: the lifecycle tier (M2) becomes a
  documented manual procedure for a real Linux box, and M1 (portable contract + numbers, no
  systemd needed) still stands. Record the refusal verbatim either way.
- **R2 — the fd-passing protocol is exactly `LISTEN_PID` == getpid(), `LISTEN_FDS` = N, fds from
  3 (`SD_LISTEN_FDS_START`), already-listening.** No C call, no library. Also verify who owns and
  unlinks the socket *file* in activation mode (systemd should; step 18's stale-socket-file trap
  says observe, don't assume).
- **R3 — the socket unit outlives the service.** After `kill -9`, systemd keeps the listener; a
  client `connect(2)` in the respawn window **succeeds and queues**. This is the open-then-verify
  re-check: observe whether the queued connect is eventually accepted by the respawned daemon
  (systemd's on-demand restart) or sits dead like launchd's window, and for how long.
- **R4 — user units vs system units.** The real product wants a *user* unit (the launchd
  LaunchAgent analog), which needs a logind session or `loginctl enable-linger`; a PID-1 container
  most cheaply runs *system* units. The probe uses whichever works in the container and **prices
  the delta on paper** in the report (one paragraph, zero extra builds).
- **R5 — `systemd-socket-activate` exists** (a systemd test tool that emulates socket activation
  without PID 1) as a cheap first rung for the fd handoff before the full container ceremony.

## The vehicle and the wire: unchanged, on purpose

Same three crates. `sync-settings` and `sync-wire` are expected **byte-untouched** — the moment
either needs a `cfg` or any edit *to function* on Linux, that is kill criterion 1, the contract
failing to port. `syncd` gains only the cfg gate and the `--systemd` adapter (both in the bin;
the lib keeps `#![forbid(unsafe_code)]`, and the systemd adapter needs no unsafe beyond
`FromRawFd`, which is its documented contract).

## Non-goals (hard boundaries)

- **No UI, no file-manager integration.** A Nautilus/Dolphin badge story is not a re-confirmation
  probe; whether the campaign needs a Linux step-19 twin is a design-pass decision.
- **No packaging** (deb/rpm/Flatpak), **no D-Bus, no polkit, no sandbox tech** (Landlock,
  AppArmor). There is no sandbox pressure on this leg — say so once in the report; do not
  manufacture some.
- **No framework-crate changes**; `git diff crates/` empty at exit.
- **No libsystemd, no systemd/sd-notify crate, no tokio.** The adapter is std-only or the finding
  is that it can't be.
- **No designing the per-OS activation adapter.** Bank the launchd/systemd asymmetry as generator
  evidence; the design pass decides what `bolted-ffi-gen` (or a sibling) emits.
- **No CI wiring.** The Linux tiers are machine-bound and docker-gated, like `test:os:launchd` —
  never inside `check`.

## Deliverables

1. **`syncd` portable**: launchd module cfg-gated to macOS; `--systemd` mode adopting
   `LISTEN_FDS` in pure std; a host unit/integration test that *fakes* the protocol (env + fd 3)
   and rides `mise run check` on every OS.
2. **`spikes/os-integration/linux/`**: `syncd.socket` + `syncd.service` units, the container
   Dockerfile (Rust toolchain + systemd), and a short README (what runs where, how to re-run).
3. **Scripts + verbs**: `test:os:linux` (build + `cargo test -p syncd` + `syncctl` numbers,
   inside the Linux container) and `test:os:systemd` (the L rows, inside the PID-1 container).
   Both docker-gated with an honest skip message.
4. **`docs/steps/step-20-report.md`** + ROADMAP update: answered matrix, numbers with caveats,
   friction log, the generalization verdicts (open-then-verify, H6, idle-exit), the
   activation-adapter asymmetry for the design pass.

## Milestones (portable seam first; the container ceremony second)

- **M0 — the portable seam (host-only).** cfg gate + `--systemd` + the faked-protocol test;
  `mise run check` green on macOS with the new test riding it. **Watched red first**: the test
  must fail when `LISTEN_PID` doesn't match. Commit.
- **M1 — the contract on Linux.** Rust container builds the workspace members; `cargo test -p
  syncd` green on linux/arm64 (row P1); `syncctl` D1–D4 recorded in-container. Commit.
- **M2 — the systemd lifecycle (the expected quagmire — time-box the container ceremony).**
  PID-1 systemd container; install units; rows L1–L5. If R1 refuses within the box: record
  verbatim, mark L rows *not executed*, M1 stands (the step-05 precedent). Commit.
- **M3 — report + ROADMAP.** Commit.

## Probe matrix (each row ⇒ a test or a scripted, reproducible procedure)

**P — Portability** *(question 3; kill 1)*
- P1: `syncd/tests/probe.rs` (B1–B4, E1, E2, F1) passes on Linux **unmodified** — the whole
  step-18 contract matrix, re-run on the second OS in one verb.
- P2: `sync-wire`/`sync-settings` byte-untouched (`git diff` empty for both); the values-only
  grep discipline from step 18 still holds — with its planted positive control re-run, not
  assumed.
- P3: the faked-`LISTEN_FDS` host test — correct adoption of fd 3, refusal on `LISTEN_PID`
  mismatch (watched red), refusal on zero fds.

**L — Lifecycle under systemd** *(questions 1–2; R1–R4)*
- L1: socket activation — first client connect spawns `syncd --systemd`, gets a pong.
- L2: single instance — a second manual start of the service (and a stray `syncd --systemd` by
  hand) cannot create a second authority; record the exact refusal mode.
- L3: crash-respawn + **the backlog window** — `kill -9` mid-session; a client connects
  *immediately* (before respawn): does `connect(2)` succeed? is the connection eventually
  accepted, and after how long? does an open-then-verify ping bound the wait? This is the
  step-19 finding, generalized or localized.
- L4: idle-exit ⇄ reactivation — `--idle-exit-secs` fires with no clients; the *next* connect
  respawns through the socket unit (step 18 A4's loop, on systemd).
- L5: **H6 by hand** — dirty draft, stash client-side, `kill -9` the daemon, restore into the
  respawned daemon over a fresh connection (F1's assertions, once, eyes on).

**D — Chattiness on Linux** *(kill 2)*
- D1–D4 via `syncctl` in-container: ping floor, `try_set`, `snapshot`, keystroke pair p50/p95.

## Measurements

D1–D4 p50/p95 (with the caveat named honestly: Docker VM on Apple Silicon — same arch, wrong
kernel path); `syncd` stripped binary size on Linux; container image cold/warm build wall-clock;
tier wall-clocks; versions (Docker, distro image, systemd version, rustc). L3's
backlog-acceptance latency. No pass/fail thresholds except kill bar 2.

## Kill criteria (hitting one is a successful probe outcome — stop and report)

1. **The contract cannot cross on Linux without editing `sync-wire`/`sync-settings`** (any edit
   *needed to function*, cfg or otherwise) — step 18 kill 3's portability face. Outranks
   everything here.
2. **Chattiness:** D4 keystroke pair **p50 > 1.0 ms** in-container. A miss here is *suspect*
   (virtualization), not final: attribute before declaring — re-run D1 (the floor) and compare;
   if the floor carries the miss, report "environment-bound, needs real hardware" instead of a
   kill. *(Calibration: step 18 measured 26–45 µs; the bar should not fire.)*
3. **systemd cannot hand a plain executable its socket without a linked library** — R2 refuted;
   the pure-std adapter is impossible. Record what it actually takes.
4. **Single-instance needs hand-rolled locks under systemd** — the step-18 kill 4 twin. Unlikely;
   evidence if so.

## Inherited cautions

- **Every new red watched** (steps 10/13/16–19): P3's mismatch case verified to fail with the
  guard removed; P2's grep re-proven by its planted control; L3 asserted with step 19's lesson
  loaded — a successful `connect(2)` proves nothing by itself.
- **The exit-code trap, container edition**: `docker run`'s exit status is the inner command's
  only if the script propagates it — read actual test counts/output out of the container, never
  trust a wrapper's zero (memory: `test-android-exit-code-masks-failures`).
- **`| tail` on background output buffers until EOF** (step-19 friction) — read log files
  directly.
- **Stale socket files refuse binds** (step-18/19 trap) — in activation mode observe who unlinks;
  in `--socket` mode the daemon already handles it.
- **A green suite is not evidence about a running system**: L5 is mandatory and by-hand.
- Commit per milestone; build/test only via `mise run …`; edition 2024; clippy `-D warnings`; no
  `unwrap`/`expect`/`panic!` in library code (probe tests may).

## Exit checklist

- [ ] `mise run check` green **on macOS**, host-only, no new external requirement; the P3 test
      rides it everywhere.
- [ ] `git diff crates/` empty; `sync-wire`/`sync-settings` byte-untouched.
- [ ] Probe matrix: every row has a test, a scripted procedure, or an explicit justified
      *not executed* (R1 refusal is the anticipated case for L rows).
- [ ] The open-then-verify generalization has an **empirical verdict** (L3), whichever way it
      lands.
- [ ] Numbers recorded with versions and the container caveat; kill bar 2 checked.
- [ ] `step-20-report.md`: answered matrix, friction log, the activation-adapter asymmetry
      priced, the R4 user-unit delta priced on paper; ROADMAP updated (20 → done, design pass
      unblocked). **ARCHITECTURE untouched.**

## If you hit a wall

Omitted decision → smallest reversible choice, recorded. Structural conflict → stop and record
for the design session. The expected quagmire is R1 (systemd-in-container); time-box it, and
remember: a partial matrix honestly reported outranks a complete one quietly faked — "no systemd
environment on this machine" is a legitimate, useful verdict that leaves M1's portability
evidence fully intact.
