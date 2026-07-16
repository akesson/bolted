# Step 18 — OS-integration spike I: the macOS process-topology probe · Report

**Status: done. Every probe row executed; no kill criterion hit.** The three §9 questions this
step was chartered to gather evidence for all came back with clean, empirical answers on the
pinned distribution posture (Developer ID, user LaunchAgent). This report is the deliverable;
the code under `spikes/os-integration/` exists to be deleted after the topology design pass.

## The verdict, in one line

A daemon-owned store is **viable as drawn**: the shipped contract crosses a Unix-socket wire
values-only, a sandboxed Developer-ID client reaches it through an app-group container with zero
prompts, launchd owns single-instance/activation/respawn with zero hand-rolled glue, and the
keystroke pair costs ~26 µs (Rust) / ~45 µs (sandboxed Swift) against the 1.0 ms kill bar.

## Environment (record the versions)

| | |
|---|---|
| Machine | Mac16,7 (Apple M4 Pro, 48 GB) |
| macOS | 26.5.2 (25F84) |
| Xcode / Swift | 26.6 (17F113) / Swift 6.3.3 |
| rustc | 1.95.0 (edition 2024 workspace) |
| Signing identity | **Developer ID Application** (team TKBX3BV5K6) — the pinned posture; ad-hoc was never used for the sandbox rows |
| App group | `TKBX3BV5K6.dev.bolted.os-spike` (team-id prefix, the Developer ID convention) |

## What was built (all under `spikes/os-integration/`, disposable by charter)

- `crates/sync-settings` — the vehicle, macro-declared: three text values, one hand-written
  `Paused(bool)` value (D20 route), one tier-2 rule, one async check on `folder`, and the
  hand-written session-less `toggle_paused` (validate → `apply_canonical`).
- `crates/sync-wire` — the as-if-generated protocol: D27-style versioned envelope over
  newline-delimited JSON; **zero bolted dependencies**; values-only pinned by a source grep with
  a planted positive control, from both sides. Plus the blocking Rust probe `Client`.
- `crates/syncd` — the daemon: thread-per-connection, one `Mutex<Store>`, no tokio, no FFI in
  the library (`#![forbid(unsafe_code)]`); connection-scoped draft ownership; push ticks
  collected under the lock and flushed after it (the FFI wrapper's two-phase move, D16). The
  `syncctl` driver CLI rides in the same crate.
- `apple/sync-probe` — the Swift client: Codable wire mirror + raw POSIX sockets; signed
  variants for the sandbox rows.
- Two machine-bound verbs, never in `check`: `test:os:sandbox` (M3) and `test:os:launchd` (M4,
  also session-bound — it bootstraps into the user's GUI domain and boots out on exit).
  `mise run check` gained no new external requirement; the spike's Rust crates are ordinary
  workspace members riding `check`.

## The three §9 questions — the banked evidence

### 1. Where does the core run

The daemon-owned arm is priced and it is cheap: `syncd` consumes the core as a plain crate (the
Linux/web-row precedent — no FFI), holds it behind one mutex with std threads, release binary
879 KB (690 KB stripped), 1.7 MB RSS at boot, 2.0 MB after 100 draft cycles. Two clients in two
languages (three counting the sandboxed variant) attached simultaneously and live rebase worked
across them (E1). Nothing about the store's shape resisted: **no framework crate was touched**
(`git diff crates/` is empty).

### 2. Can the contract cross a process boundary on the ladder

**H1 confirmed.** `DraftId` as a `u64` wire token, effects as pushed data, and every judgement
crossing as the core's own keyed report:

- Tier-1 refusals arrive with structured params intact (`too_long {max:30, actual:31}`), B1.
- The tier-2 rule and the check's pending/required/failed states arrive as the same
  `validate()` report an in-process shell reads — the wire never re-derives them.
- Single-flight held with the driver in another process: a superseded token's completion is
  discarded (B2; watched red with the daemon-side guard bypassed before it was trusted).
- The three-way conflict shape (`raw`/`base`/`theirs`) crossed as values; keep-mine resolution
  and the subsequent submit worked from the second client (E1).
- The wire stayed values-only to the end — `sync-wire` compiles with no bolted dependency, and
  its source greps clean of judgement names (kill criterion 3 never approached).

### 3. Single-instance ownership

**R1 confirmed; launchd owns all of it, rung 3 (a generated plist), zero lock files:**

- A1: socket activation — launchd holds the listener; the first client connect spawns `syncd`,
  which adopts the fd.
- A2: a second `launchctl bootstrap` of the label is refused: `Bootstrap failed: 5:
  Input/output error` (verbatim). The label is the authority.
- A3: `kill -9` → the next connect respawns a fresh daemon; canonical version reset to 0, zero
  drafts — **all pre-crash state is gone**, asserted, which is what makes H6 matter.
- A4: idle-exit implemented (4 s sweeper in the bin); the daemon exits when connection-less and
  the next connect respawns it.
- F1/**H6 confirmed**: a client-held stash blob survived a real daemon `kill -9` — dirty values
  restored, the pre-death **passed** verdict correctly absent (C20), `folder_check_required`
  demanded a fresh check. D27's envelope played the role it was born for.

One honest caveat on A2: launchd guarantees one *service instance per label*. Nothing stops a
rogue process from binding its own socket at a different path, or deleting launchd's socket
file and squatting the path (file-path discipline, not a kernel guarantee). Peer authentication
was priced-only per the step doc — see "for the design pass".

## The sandbox verdict (row C — the campaign's riskiest unknown, and it cleared)

- **C2 (the mandatory control, run first):** the sandboxed, Developer-ID-signed client gets
  `errno=1 (EPERM)` connecting to a live socket in `/tmp`. The sandbox is provably on; C1 means
  something.
- **C1: REACHED.** The same binary connects to the daemon's socket inside
  `~/Library/Group Containers/TKBX3BV5K6.dev.bolted.os-spike/` and completes a round-trip.
  **No TCC prompt, no consent dialog, nothing to click** on macOS 26.5.2 with the team-id-prefixed
  group. R2 confirmed.
- **C3:** tick-then-fetch works sandboxed — the sandboxed client observed a Rust-client-driven
  toggle via the pushed version tick and fetched the changed canonical.
- **The ceremony, priced (R4):** exactly three ingredients — (1) App Sandbox + app-group
  entitlements; (2) a real signing identity; (3) **a bundle identity**: a bare executable with
  the sandbox entitlement is killed at launch (SIGTRAP in `_libsecinit_appsandbox`) until a
  `CFBundleIdentifier` is embedded via `__TEXT,__info_plist`. That third ingredient cost the
  probe an hour and is invisible in documentation — priced now for VISION's scaffolding promise.
- The R3 XPC fallback ladder was **not walked** — R2 held, so nothing forced it.

## Measurements (baseline, not thresholds — except kill bar 2)

Release builds both sides, 2000 iterations, local Unix socket, M4 Pro. p50/p95 in µs:

| Row | Rust client | Swift client | Swift client, sandboxed |
|---|---|---|---|
| D1 ping (the floor) | 9.3 / 10.7 | 16.6 / 21.5 | 15.3 / 19.4 |
| D2 `try_set` | 9.0 / 11.7 | 15.5 / 19.0 | 16.1 / 27.4 |
| D3 snapshot | 10.5 / 15.3 | 27.5 / 33.0 | 27.4 / 38.5 |
| **D4 keystroke pair** | **25.8** / 36.3 | **40.5** / 53.6 | **45.1** / 58.0 |

**Kill bar 2 (D4 p50 > 1.0 ms): cleared by ~25–40×.** Interactive UI on a remote core is not in
tension with these numbers on this class of machine.

Caveats: one desktop-class machine, unloaded; the JSON codec is the spike's debuggability
choice, so these numbers *include* its cost (that is the datum, not a distortion); D1 vs D2
shows the core's per-keystroke work is ~free relative to framing + syscalls — the floor IS the
cost. The sandbox adds ~5 µs to the pair (visible but irrelevant at this scale).

Frame sizes (compact JSON): `try_set` request 84 B / response 72 B; `draft_snapshot` request
54 B / response 463 B. Daemon RSS 1 680 KB at boot, 2 032 KB after 100 draft cycles (no growth
concern at probe scale). `test:os:launchd` wall-clock: ~21 s.

## Probe matrix — all rows executed

| Row | Verdict | Where |
|---|---|---|
| A1 activation | **pass** | `test:os:launchd` + by-hand session |
| A2 single instance | **pass** (refusal recorded verbatim) | same |
| A3 crash-respawn, state gone | **pass** (version 0, drafts 0 asserted) | same |
| A4 idle-exit | **pass** (implemented, 4 s) | same |
| B1 full cycle remotely | **pass** | `syncd/tests/probe.rs` + Swift `cycle` |
| B2 async check single-flight | **pass** (watched red first) | same, both languages |
| B3 `toggle_paused` + fan-out | **pass** | `probe.rs` |
| B4 draft-id hygiene | **pass** (typed `NotYourDraft` / `UnknownDraft`) | `probe.rs` |
| C1 sandboxed reach | **pass** | `test:os:sandbox` |
| C2 sandbox control | **pass** (EPERM) | same |
| C3 tick-then-fetch sandboxed | **pass** | same |
| D1–D4 chattiness | **measured**, kill bar cleared | table above |
| E1 live rebase across processes | **pass** | `probe.rs` |
| E2 disconnect pruning | **pass** (incl. abrupt-drop variant) | `probe.rs` |
| F1 stash across daemon death | **pass** (real `kill -9`, H6) | `test:os:launchd` |

## The `command` verb evidence (§9 — banked, not resolved)

`toggle_paused` is real and its hand-written shape teaches two things:

1. **Tier-1 validity is free** for a canonical-to-canonical mutation — the flip happens inside
   a value type and the source entity is always-valid by construction.
2. **Tier-2 rules are NOT free.** `apply_canonical` runs no rules; a command that skipped the
   scratch-checkout validation would silently write a canonical no draft could ever submit.
   "Submit re-validates everything, always" must bind session-less mutations too, and today
   nothing but discipline makes it so. This is the strongest single input to the verb-design
   question.

## Friction log (the wire-generator's requirements document)

1. **`CheckToken` cannot cross a boundary** (private by design). The connection layer holds it
   and issues its own correlation id — exactly what `spike-profile-ffi` already does. A wire
   generator must emit this token-registry plumbing per checked feature.
2. **Verdicts cross as closed data.** `ErrorData.key` is `&'static str`, so a client cannot
   send an arbitrary failure key; the wire carries `ok: bool` and the daemon maps failure to the
   *declared* `failed_key`. Same as the FFI wrapper's verdict enum. The generator owns this
   mapping because it owns the declaration.
3. **serde tuples are JSON arrays.** `(String, String)` params and `(FieldId, Error)` report
   entries forced tiny custom `Codable` decoders Swift-side. A generator emitting both ends
   should pick object shapes.
4. **`AlreadySubmitted` flattens to `UnknownDraft` over the wire** — connection-scoped ownership
   is checked before the store is asked, so the store's most precise refusal is shadowed. Not
   harmful, but an honest contract-fidelity deviation the design pass should rule on.
5. **Push ordering across concurrent mutators is not globally serialized** — pushes are built
   under the lock but flushed after it, so interleavings across connections are possible.
   Version numbers on every tick make this safe (clients dedupe/fetch), but a generated client
   library should say so.
6. **The stash blob is the one frame that lives outside the envelope** (a client-kept file, not
   a daemon-read frame). It re-enters versioned (inside `restore`), but stash-at-rest versioning
   is D27's other half and stays a design-pass question.
7. **`--launchd` costs exactly one C call** (`launch_activate_socket`) — in the bin, behind the
   lib's `forbid(unsafe_code)`. "Zero FFI" held for core and wire; the launchd seam is one
   `unsafe extern` block. There is no pure-Rust path to socket activation on macOS.
8. The A2 caveat above: launchd single-instance is per-label; the socket *path* is defended by
   filesystem convention only.

## Deviations (smallest-reversible, recorded)

- `SyncInterval` is minutes-as-text with a `custom(..)` range predicate — the value DSL is
  text-first (no numeric raw shape, a known step-09 boundary); `Paused(bool)` took the D20
  hand-written route for the same reason. Both worked without framework changes.
- F1 also has an in-process "fresh daemon" variant in `probe.rs` so `check` exercises the
  restore path host-only; the launchd tier does the real `kill -9`.
- M5 produced no new code, so its numbers land in this report rather than a separate commit
  (the harnesses shipped in M3/M4).
- The values-only grep exempts comment lines — the doc comments legitimately *name*
  `bolted-ffi-gen` while explaining what the crate is; the discipline binds code. The positive
  control covers the matcher either way.

## Kill criteria — none hit

1. Sandbox unreachable: **no** — C1 green under the real posture, R3 never needed.
2. D4 p50 > 1.0 ms: **no** — 25.8–45.1 µs, ~25–40× headroom.
3. Wire forced to judge: **no** — values-only held, pinned by tests, zero bolted deps.
4. launchd can't own single-instance: **no** — label refusal recorded; no lock files anywhere.

## For the design pass (questions banked, none resolved here)

1. **Topology**: this probe prices the daemon-owned arm. What it does *not* answer: should the
   UI app embed the core and the daemon serve only background surfaces, or does everything
   attach? The latency numbers say either is affordable; the decision is about state ownership
   and offline semantics, not speed.
2. **The `command` verb**: the tier-2 bypass finding above is the case for designing it (or for
   a documented "commands validate via scratch checkout" idiom).
3. **Peer authentication** (priced, zero code): same-user filesystem permissions on the group
   container are the only gate today. A real product wants `SO_PEERCRED`-equivalent
   (`LOCAL_PEERCRED`/`getpeereid`) plus, for hostile-peer resistance, audit-token → code-signing
   checks — which on macOS pushes toward XPC for the *authenticated* surfaces even though the
   socket suffices for transport. Design-pass trade.
4. **Wire schema ownership**: `sync-wire` proves the emitted protocol can be framework-free.
   Whether `bolted-ffi-gen` emits Rust wire crates + Swift Codable from the same declaration
   (the D28 road) and what the envelope's cross-version story is (D27's other half).
5. **Cleanup policy**: connection-scoped draft closure was the strictest choice and produced no
   friction in the probe — but a menu-bar app that reconnects will want stash/restore or draft
   re-adoption, which is exactly step 19's terrain.
6. **Canonical persistence** is still nobody's job (seeded at boot, gone on exit) — the known
   VISION optional-battery gap, now with a daemon-shaped consumer.

## Exit checklist

- [x] `mise run check` green, host-only, no new external tool; `git diff crates/` empty.
- [x] `spikes/os-integration/` self-contained; README states charter + disposal criteria.
- [x] Every probe matrix row: test, scripted procedure, or measured — none skipped.
- [x] Sandbox verdict empirical, ceremony recorded (incl. the `__info_plist` trap), C2 first.
- [x] Numbers recorded with versions; kill bar 2 checked from both clients (+ sandboxed).
- [x] H6 empirical (real `kill -9`, by-hand session included).
- [x] This report; ROADMAP updated. **ARCHITECTURE untouched.**
