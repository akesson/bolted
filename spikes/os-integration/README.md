# spikes/os-integration — the process-topology campaign (Phase 5)

Disposable probe artifacts for the OS-integration spike: steps 18 (macOS topology probe),
19 (Finder-citizen app), 20 (Linux/systemd re-confirmation). Charter, terrain and kill criteria:
`docs/steps/step-18-os-topology-probe.md`; campaign sketch: `docs/ROADMAP.md` (Phase 5).

## What this campaign falsifies

Every Phase 1–4 shell has the core **in-process**. VISION's product promise (the same core runs as
the daemon, sits in the menu bar, badges the file manager) breaks that assumption: a Finder
extension is a separate sandboxed process, and a daemon runs when no app does. The campaign gathers
the evidence ARCHITECTURE §9 reserved for exactly this:

1. **Where does the core run** — embedded per-process, or daemon-owned with surfaces attached?
2. **Can the contract cross a process boundary** (observe / draft / submit / async check over IPC)
   while staying on the verification ladder?
3. **Single-instance ownership** — who guarantees one daemon, who starts it, what happens on crash.

## Layout

- `crates/sync-settings` — the vehicle feature, macro-declared (the framework path), plus the
  hand-written session-less mutation `toggle_paused` (§9's demoted `command` verb, hand-written on
  purpose — the verb is **not** being designed here).
- `crates/sync-wire` — the hand-written as-if-generated IPC protocol: a D27-style versioned
  envelope over newline-delimited JSON. **Values only** — raw field values, keyed errors, ids,
  versions; zero bolted dependencies, exactly like the Swift `Codable` side.
- `crates/syncd` — the daemon: a pure Rust bin (zero FFI, no async runtime), one
  `Mutex<Store<SyncSettingsDraft>>`, thread-per-connection over a Unix socket, connection-scoped
  draft ownership, push ticks on canonical change.
- `apple/` — the Swift probe clients (M3+): envelope proof unsandboxed, then the sandboxed
  app-group variant for probe row C.
- `linux/` — step 20: the Docker image (pinned Rust + systemd) and the `syncd.socket` /
  `syncd.service` units; driven by `mise run test:os:linux` (contract + numbers) and
  `test:os:systemd` (lifecycle rows in a systemd-PID-1 container).

The spike crates are workspace members so `mise run check` compiles, clippys and tests them like
everything else. `check` gains no new external requirement from this directory.

## Disposal criteria

Everything here exists to be learned from, then deleted: **one `rm -rf spikes/os-integration` plus
removing three lines from the root `Cargo.toml` members list** must be a clean exit at any time.
Findings land in `docs/steps/step-18-report.md` and, after the design pass, in ARCHITECTURE —
never by this code becoming load-bearing. If anything under this directory acquires a dependent
outside it, that is a finding to report, not a state to accept.

Delete after: the topology design pass has resolved §9's process-topology questions into
D-decisions, and any code worth keeping has been **re-derived** in framework crates through a
normal step (evidence first, extraction later — the D22/D28 road).
