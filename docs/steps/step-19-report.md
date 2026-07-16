# Step 19 — OS-integration spike II: the Finder-citizen app · Report

**Status: done. Every scripted probe row executed and green; no kill criterion hit.** The two
visual rows (badge pixels, the context-menu click) remain as a two-minute manual protocol below —
the citizen was left running for it. The step's three questions all came back with clean answers,
several of them stronger than the recon expected.

## The verdict, in one line

The Finder-citizen topology works end-to-end as VISION draws it: a hand-assembled (no Xcode
project) Developer-ID `.app` + FinderSync `.appex` is accepted by pluginkit and spawned by the
OS into its own sandbox, reaches the daemon through the group-container socket, draws badge state
from canonical over tick-then-fetch, and issues the session-less command from Finder's context
menu; SMAppService registers the bundled, socket-activated daemon with **zero approval ceremony**;
and a real SwiftUI editor runs the full contract over the wire at ~100 µs per keystroke against
the 16 ms kill bar — including a reconnect story where the surfaces themselves resurrect a
`kill -9`'d daemon through socket activation.

## Environment

| | |
|---|---|
| Machine / macOS | Mac16,7 (M4 Pro) · macOS 26.5.2 (25F84) |
| Xcode / Swift / rustc | 26.6 / 6.3.3 / 1.95.0 |
| Signing | Developer ID Application (team TKBX3BV5K6), the step-18 posture |
| App group / socket | `TKBX3BV5K6.dev.bolted.os-spike` / `…/syncd.sock` (step 18's, reused) |

## What was built (`spikes/os-integration/apple/finder-citizen/`, disposable by charter)

- **`SyncWireKit`** — the step-18 Codable wire, copied (deviation: copy over cross-package
  restructuring; the probe package stays byte-untouched) and extended with the UI verbs
  (`resolve`/`close`/`stash`/`restore`/`stats`). Two clients: the blocking probe `LineClient`
  and **`WireConnection`** — a reader-thread demultiplexer (correlated responses to blocked
  requesters, pushes to a callback queue) that a push-driven UI turns out to require.
- **`BoltedSyncCore`** — `SyncViewModel`: echo-rule buffers, client-driven folder check,
  conflict resolution from snapshot data, submit rendering returned refusals, continuous stash +
  restore-on-reconnect; `ErrorMessages` (key → template, params from the wire); `GroupSocket`
  resolution. Headless-tested against a real `syncd` per test.
- **`BoltedSyncApp`** — `MenuBarExtra` + settings window as thin scene layout; SMAppService
  register/unregister (menu + `--daemon` CLI); `--drive` CLI so probes make canonical changes
  through a real product path.
- **`FinderBadges`** — the FinderSync extension: watched directory = canonical `folder`, badge =
  canonical `paused`, context-menu `toggle_paused`, 2 s reconnect loop with open-then-verify.
- **`scripts/assemble-app.sh`** (79 lines, 5 codesign-related invocations) — SPM binaries +
  hand-written plists + inside-out signing → `dist/BoltedSync.app`. **This script is the R1
  pricing artifact**: it is everything `bolted new` scaffolding would have to emit for the
  tray/badges promise, and it fits in a page.
- **`test-os-app.sh`** + the `test:os:app` verb (machine- and session-bound, never in `check`):
  U rows (headless XCTest, `BOLTED_SYNCD`-gated), the no-constraint-literals grep (planted
  positive control), G rows, S rows, M4 lifecycle rows. `run:os:app` is the unbundled dev tier.

## The three questions

### 1. Does the sandbox verdict survive OS spawning? — Yes (G rows)

- **G1**: pluginkit accepts the hand-assembled appex; `pluginkit -e use` enables it from the CLI
  with **no System Settings visit** (R4 confirmed). No Finder relaunch was needed for the spawn.
- **G2**: the OS spawns it into its own extension sandbox — home is
  `~/Library/Containers/dev.bolted.sync.finderbadges/Data`, the binary verifiably inside the
  `.appex` (and pluginkit passes it `-AppleLanguages` argv, a reminder the OS owns the launch).
- **G3**: the spawned, sandboxed process connects to the group-container socket and completes a
  ping round-trip. **Control (run in the same process, mandatory)**: a live daemon socket in
  `/tmp` is refused `errno=1 EPERM` — the sandbox is provably on, so G3 is not vacuous.
- **R2(a)**: FinderSync is alive on macOS 26 for a plain watched folder — no deprecation
  refusal anywhere in the chain (`FIFinderSync`, badges, context menus all functional).
- The appex build ceremony, priced: `CFBundlePackageType XPC!`, an `NSExtension` dict with the
  module-qualified principal class, and `NSExtensionMain` called explicitly from a normal SPM
  `main.swift` (declared `(argc, argv) -> Int32` via `@_silgen_name`) — no Xcode, no
  `-e _NSExtensionMain` linker tricks needed.

### 2. Can the app bundle own the daemon at rung 3? — Yes, with one packaging wrinkle (S rows)

- **S1**: `SMAppService.agent(plistName:)` over a bundle plist: `register()` returned OK and
  `status == .enabled` **immediately — zero approval ceremony** (no Login Items prompt, no
  notification interaction) for this Developer-ID app on this macOS. R3 confirmed, stronger
  than recon expected.
- **S2**: socket activation composes with `BundleProgram`: the first connect spawned the
  **bundled** `syncd` (proven via the txt descriptor — argv[0] is `ProgramArguments[0]`, so
  `ps` args cannot prove which binary ran).
- **S4**: `kill -9` → next connect respawns, canonical reset to v0 (step 18's A3 under new
  ownership); a second manual bootstrap of the label is refused with the identical verbatim
  `Bootstrap failed: 5: Input/output error` (A2).
- **S3**: `unregister()` boots the agent out cleanly; re-register works.
- **The wrinkle (design-pass input)**: launchd does not expand `$HOME` in `SockPathName`, so a
  socket-activated agent inside a **signed** bundle must bake a per-user absolute path into a
  sealed resource at assembly time. Fine for a spike; a real per-user installer/scaffold has to
  solve this (per-user re-sign, a fixed shared path, or giving up activation for RunAtLoad).

### 3. Does the contract's UI story hold over the wire? — Yes (U rows, headless + kill bar 2)

- **U1**: the menu-bar surface renders fetched canonical, updated by tick-then-fetch when
  another client mutates — no polling, no local echo.
- **U2**: per-keystroke `try_set` with the echo rule (rejected text and sanitized trims both
  leave the focused buffer untouched; blur adopts core raw); keyed errors render from wire
  params (`too_long {max:30, actual:31}` → the template's sentence, the core's numbers); the
  async folder check runs begin/complete from the client, whose own filesystem access is the
  capability; `folder_check_required` → settled → `folder_unreachable` on a bad path.
- **U3**: a second connection submits under an open draft → `DraftRebased` push → conflict
  triple in the snapshot (mine preserved, focused buffer untouched) → keep-mine → submit lands.
- **U4**: daemon `kill -9` under a dirty editor → the VM's continuously-refreshed stash
  survives client-side → reconnect restores it: dirty values back, **the pre-death PASSED check
  verdict correctly absent** (C20 visible in a UI for the first time). Watched red first
  (restore skipped → both assertions fail) before the green was trusted.
- **U5 / kill bar 2**: keystroke-to-state (try_set + snapshot + **stash**, i.e. three
  round-trips) p50 **96–114 µs**, p95 144–161 µs (n=300 per run, debug daemon build) against
  the 16 ms bar — ~150× headroom, and that's before attributing anything to SwiftUI rendering.

## The M4 lifecycle rows (the topology, integrated)

- The appex's 2 s reconnect loop **resurrects a killed daemon through socket activation** — the
  observing surface heals the topology by observing it (scripted: kill -9 → `live-wire
  disconnected` → reconnect → new daemon pid → badges live).
- `toggle_paused` from a real product path flips the appex's badge identity via the push tick
  (G4a); a canonical `folder` change re-points `FIFinderSyncController.directoryURLs` (G4b) —
  observe-over-wire driving an OS API. Finder's `beginObservingDirectory` fired for the real
  window (`open ~/BoltedSpikeDemo`), so the badge pipeline is engaged end-to-end.
- **Idle-exit finding**: with an always-running extension holding a connection, the daemon's
  idle-exit never fires — the on-demand bargain from step 18 becomes "effectively always-on"
  the moment a persistent surface exists. Not a defect (activation still owns crash/boot), but
  the design pass should name the intended steady state.

## The findings that matter (the generator's requirements, continued from step 18's log)

1. **`connect(2)` success is not daemon liveness under socket activation.** A connect issued in
   the post-`kill -9` window can sit in launchd's listener backlog, never accepted, no EOF —
   the client believes it is connected to nothing. Sessions must **open-then-verify** (ping
   before believing). Both clients here do; a generated client library must.
2. **The wire needs two client shapes**: a blocking request/response client (probes, CLIs) and
   a reader-thread demultiplexer for push-driven UIs. The second is ~180 lines of subtle
   threading a generator should emit once, correctly.
3. **Crash survival requires the continuous-stash idiom**: the client must refresh its stash
   after every mutation to have something to restore (H6's blob is pull-only). One extra µs-scale
   round-trip per keystroke made it free here — but the idiom should be a named pattern (or the
   wire should push stash deltas) rather than folklore. This is design-pass question 5's evidence.
4. **`$HOME` in `SockPathName`** (above) — activation vs signed-bundle portability.
5. **argv[0] is `ProgramArguments[0]`** — process-identity assertions need the txt descriptor.
6. Copying the Codable wire (M0 deviation) was the right disposable-code call: the extension it
   needed (five verbs, stash mirror, refusal decode) would have churned the probe package for
   nothing.

## Measurements

| | |
|---|---|
| Keystroke-to-state p50 / p95 (U5, 3 round-trips, debug daemon) | 96–114 µs / 144–161 µs |
| Kill bar 2 | cleared ~150× |
| Bundle / appex / binaries | 2.2 MB `.app` · 572 KB `.appex` · app 744 KB, syncd 892 KB, appex 563 KB (release, signed) |
| Assembly ceremony | 79-line script, 2 Info.plists + 1 agent plist + 2 entitlements, 3 codesign signs |
| SMAppService approval steps | **0** |
| `test:os:app` wall-clock | 27 s warm (all builds cached); first cold run is minutes (SPM + cargo release builds) |

## Manual protocol (the two visual rows — the citizen is live right now)

Everything is already running: agent registered, extension enabled, `BoltedSync.app` in the menu
bar, and a Finder window open on `~/BoltedSpikeDemo` (three demo files, currently **paused** →
pause badges).

1. **Badge pixels (G4 visual)**: look at the open Finder window — the three files should carry
   the pause badge.
2. **The command from Finder (G5)**: right-click inside that window → **"Resume Bolted Sync"** →
   badges flip to checkmarks; the menu-bar icon's state follows via the same tick.
3. **The editor**: menu-bar icon → Edit Settings… — type fast into Label (cursor must never
   jump); set Interval to `5` and Folder to something under `/Volumes/…` to see the tier-2 rule;
   invalid input shows the core's numbers in the sentences.
4. Teardown when done:
   `dist/BoltedSync.app/Contents/MacOS/BoltedSyncApp --daemon unregister && pluginkit -e ignore -i dev.bolted.sync.finderbadges && killall BoltedSyncApp`
   (from `spikes/os-integration/apple/finder-citizen/`; the demo folder is `~/BoltedSpikeDemo`.)

Screen-recording permission wasn't available to the probe session, so these rows could not be
screenshot-verified mechanically; every non-pixel aspect of both paths is asserted by log/script.

**Manual execution record (Henrik, 2026-07-16):**
- **Badge pixels: CONFIRMED** — screenshot of the Finder list view shows all three files carrying
  the checkmark badge drawn by the extension from canonical `paused=false`.
- **Menu-bar surface: CONFIRMED** — screenshot shows canonical rendered live (label, folder,
  interval, active state) plus `Daemon: enabled` (SMAppService status); a human-driven toggle
  from this menu propagated to the extension (`watching … paused=false v=3` in the appex log) —
  the app→daemon→extension fan-out under a real hand.
- **G5 (the command from Finder's context menu): CONFIRMED** — two human right-clicks in the
  Finder window round-tripped the session-less command (appex log: `G5 context-menu toggle ->
  toggled paused=false` at v=5, then `toggled paused=true` at v=6), each followed by the
  fan-out tick re-watching (`watching … v=5` / `v=6`). Every manual row is now closed.

## Deviations (smallest-reversible, recorded)

- Codable wire copied, not shared (above). — The `--drive` CLI was added to the app binary
  instead of extending `syncctl`, keeping the spike's Rust crates byte-untouched (`git diff
  main -- crates/ spikes/os-integration/crates/` is empty).
- The greppable no-literals rule is scoped to `Sources/` (tests legitimately assert the core's
  numbers arriving as params); the matcher is proven each run by a planted positive control.
- The VM connects on first menu-open (`onAppear` of the MenuBarExtra content), not app launch —
  a scene-lifecycle quirk left as-is and noted for the design pass's client-library shape.
- `swift run` dev tier (`run:os:app`, recon R5) was implemented but not exercised this session —
  the bundled app covered every row that mattered. Marked not-executed, honestly.

## Kill criteria — none hit

1. Finder-spawned extension unreachable: **no** — G3 green with its EPERM control.
2. Keystroke-to-render p50 > 16 ms: **no** — ~0.1 ms with three round-trips.
3. Values-only breaking in UI glue: **no** — every judgement rendered here arrived as keyed
   data; the no-literals grep (proven matcher) pins the shell side.
4. No rung of app-owned registration works: **no** — SMAppService worked on the first rung,
   with zero ceremony.

## For the design pass (adds to step 18's list)

1. **Open-then-verify** belongs in the generated client contract (finding 1).
2. **The steady-state question**: idle-exit vs persistent surfaces (M4 finding) — is the daemon
   on-demand, always-on, or "on while any surface lives"? The answer decides KeepAlive vs
   Sockets-only plists and the reconnect idiom.
3. **Stash cadence**: bless the continuous-stash idiom, or push stash state?
4. **Packaging**: the `$HOME`-in-plist wrinkle; and the assembly script's inventory is exactly
   the scaffold spec (`bolted new --surface finder-badges` could emit all of it).
5. Everything from step 18's list stands; nothing here contradicted it.

## Exit checklist

- [x] `mise run check` green, host-only, untouched; `git diff crates/` empty; spike Rust crates
      byte-untouched.
- [x] Probe matrix: every row scripted/tested green, except the two visual rows — executed as
      far as logs can prove, remainder is the numbered manual protocol above.
- [x] G3 empirical with its control; ceremony (R1/R4) recorded verbatim.
- [x] SMAppService ceremony end-to-end (register → enabled → activate → unregister), 0 approvals.
- [x] U4 reconnect story against a real daemon death, watched red first; ergonomics recorded
      (continuous stash, open-then-verify).
- [x] Numbers with versions; kill bar 2 with attribution (wire floor 26–45 µs from step 18; the
      VM path ~100 µs; SwiftUI not yet the bottleneck at either figure).
- [x] This report; ROADMAP updated (19 → done). **ARCHITECTURE untouched** — no §9 question
      resolved.
