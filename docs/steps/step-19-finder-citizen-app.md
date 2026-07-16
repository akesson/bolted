# Step 19 — OS-integration spike II: the Finder-citizen app

**Phase 5 · OS-integration spike. Status: ready.** Read first: [VISION.md](../VISION.md) (bet 2 —
*"real tray icons, real daemons"*; the scaffolding promise: *"a tray icon or a daemon is a scaffold
option, not a custom engineering project"*), [ARCHITECTURE.md](../ARCHITECTURE.md) (§1 the verbs,
§9's process-topology bullet), [ROADMAP.md](../ROADMAP.md) (Phase 5 campaign sketch), and the
[step-18 report](step-18-report.md) — this step is built entirely on its seam and its banked
ceremony (the app-group socket, the signing posture, the `__info_plist` trap).

## Goal

Step 18 proved the wire with processes **we** launched: a shell script started the daemon (or
launchctl did, from a hand-installed plist), and every client was an executable the probe ran by
hand. VISION's product promise is about processes the **OS** owns: the user launches a menu-bar
app; the app registers its own daemon; **Finder spawns a sandboxed extension** when the user opens
a folder. Step 03's analog on step 18's seam — a real UI on the contract, except the contract is
now on the other side of a process boundary. Three questions, each a step-18 answer re-tested with
the OS in the driver's seat:

1. **Does the sandbox verdict survive OS spawning?** C1 proved a *manually-launched* sandboxed
   process reaches the group-container socket. A FinderSync extension is spawned by
   `pluginkit`/Finder with an extension sandbox we do not configure at launch time — the first
   process on the wire whose lifecycle, environment, and container are entirely the OS's.
2. **Can the app bundle own the daemon at rung 3?** Step 18's launchd tier hand-installed a plist.
   Real Dropbox-style distribution registers a bundled LaunchAgent via `SMAppService` — does socket
   activation survive that packaging, and what approval ceremony does the OS demand?
3. **Does the contract's UI story hold over the wire?** Step 03's behaviors (echo rule, conflict
   UI, live rebase, submit flow) were proven with the core in-process. The settings editor now
   drives checkout / per-keystroke `try_set` / async check / submit through the socket — plus the
   story step 18's report explicitly deferred here (design-pass question 5): **what does a
   reconnecting UI do with its dirty draft?** (stash/restore, H6's intended consumer).

As in steps 03/18, **a green suite and a running app are not the deliverable; evidence is** — the
answered probe matrix, the recorded ceremony (verbatim prompts and refusals), the measured numbers,
and the friction log in `docs/steps/step-19-report.md`.

**Load-bearing principle (steps 02/05/18):** every restructuring the OS forces is the probe's most
valuable output. Do not patch the spike's Rust crates — or worse, the framework — to make the app
prettier; record what you had to work around.

## Where this lives

`spikes/os-integration/apple/finder-citizen/` (one SwiftPM package: shared wire library + app +
extension targets) plus `spikes/os-integration/scripts/` for assembly/probe scripts. Same disposal
charter as the rest of the campaign: deletable in one `rm -rf`, findings outlive the code. The
existing `apple/sync-probe` package stays untouched except for whatever minimal restructuring lets
its Codable wire layer be shared (a library target is the expected shape — implementer latitude;
copying the two files is the fallback and also fine, this is disposable code).

## What the planning pass verified (by reading the code, 2026-07-16)

- **The wire already carries a full UI.** `sync-wire`'s `Request` has `CanonicalSnapshot`, the
  complete draft cycle (`Checkout`/`TrySet`/`Validate`/`Resolve`/`BeginCheck`/`CompleteCheck`/
  `Submit`/`Close`), `Stash`/`Restore`, `TogglePaused`, and push frames (`CanonicalChanged`,
  `DraftRebased`). **Hypothesis: zero new wire verbs and zero Rust changes are needed.** If a
  surface forces a new verb, that is first-order generator evidence — record it, then add it in
  the spike crates only.
- **The vehicle is already Finder-shaped.** `SyncSettings.folder` (an absolute path, async-checked)
  is a natural watched directory for `FIFinderSyncController.directoryURLs`; `paused` is a natural
  badge state; `toggle_paused` is the context-menu command. No vehicle changes expected.
- **The ceremony is priced and reusable.** `test-sandbox.sh` already derives the signing identity
  and team-id-prefixed app group from the keychain; `sync-probe/Package.swift` carries the
  `__TEXT,__info_plist` embedding. The new scripts extend this, not reinvent it.
- **The step-03 UI patterns transfer.** Echo rule (focused buffer never overwritten from core),
  subscribe-then-fetch reconciled by version, debounced check driving — all shell-side *when*
  patterns; over the wire the snapshot fetch is a request instead of an FFI call. The VM is
  headless-testable against a real daemon on a temp socket (the `probe.rs` precedent, from Swift).

## Reconnaissance to falsify (OS claims — hypotheses, not facts; a refutation is a finding)

- **R1 — a hand-assembled bundle is a real bundle.** An `.app` (and an embedded `.appex`) built by
  script — SPM executables + hand-written `Info.plist`s + `codesign` — is accepted by
  LaunchServices, pluginkit, and SMAppService exactly like an Xcode-built one. Nothing in the
  formats requires Xcode; the ceremony (nested signing order, embedded profiles, plist keys) is
  the unknown. **This is also the pricing exercise**: the assembly script *is* the inventory of
  what `bolted new`'s scaffolding would have to emit.
- **R2 — FinderSync is alive and its extension can reach the socket.** (a) `FIFinderSync` still
  loads and badges on this macOS for a plain watched folder (Apple has been steering sync apps
  toward File Provider — if FinderSync is refused or hollowed out here, that redirects the
  file-manager leg and the design pass must know); (b) the Finder-spawned, OS-sandboxed extension
  process can `connect()` to the group-container socket like step 18's hand-launched client did.
  (b) is the campaign question this step exists to answer.
- **R3 — SMAppService owns the bundled agent.** `SMAppService.agent(plistName:)` registers a plist
  from `Contents/Library/LaunchAgents/` using `BundleProgram`; the `Sockets` key (socket
  activation) is honored under that registration; approval is one Login-Items consent, recorded
  verbatim; `unregister` boots it out cleanly.
- **R4 — pluginkit can enable the extension from the CLI** (`pluginkit -e use -i <id>`) without a
  System Settings visit on this macOS. If refused, the System Settings path is the recorded
  ceremony (screenshot-level precision: which pane, which toggle).
- **R5 — a `swift run` executable can be a menu-bar app.** `MenuBarExtra` (or `NSStatusItem`) works
  from a bare SPM executable with the accessory activation policy — the fast dev tier for the UI
  rows, no bundle needed. Only bundle-dependent rows (SMAppService, the appex) need assembly.

**Pinned assumptions (record in the report):** Developer ID posture throughout (step 18's pin); the
same app group (`<TEAM>.dev.bolted.os-spike`) and socket path, so the step-18 daemon artifacts are
reused as-is; macOS version recorded with every ceremony observation (this terrain shifts by
release).

## The surfaces

**The menu-bar app** (`BoltedSync.app` when bundled; plain executable in the dev tier):
- A `MenuBarExtra` showing canonical state via tick-then-fetch (label, folder, interval, paused),
  with a Pause/Resume item (`TogglePaused`) and an "Edit Settings…" window.
- The settings window is **step 03's form over the wire**: per-field error text from keyed
  `ErrorWire` params (a `key → template` map, params from the wire — no constraint literals in
  Swift, greppable), the echo rule on text fields, the async `folder` check driven
  begin/complete client-side with a debounce, conflict banners with keep-mine/take-theirs, submit
  rendering the returned report. The check's completion driver is the Swift client itself (it
  probes reachability of the path however trivially — the *plumbing* is what's on trial, not the
  check body).
- **The reconnect story (design-pass question 5's evidence):** on daemon death (socket EOF) or app
  quit with a dirty draft, the VM stashes client-side (the H6 blob, kept in memory or a file —
  latitude); on reconnect it restores and re-renders. Record the ergonomics honestly — this is the
  strongest input to the cleanup-policy design question.
- SMAppService registration UI: a menu item / first-run path that registers the bundled daemon and
  surfaces `SMAppService.status` truthfully.

**The FinderSync extension** (`FinderBadges.appex`, embedded in the app bundle):
- Watches the canonical `folder`; badges items by `paused` state (two badge identifiers — this is
  canonical-state-driven badging, deliberately **not** per-file sync truth).
- A context-menu item ("Pause/Resume Bolted Sync") issuing `TogglePaused` — the session-less
  command from the most OS-shaped surface there is.
- Connects lazily; tick-then-fetch keeps the badge and the watched URL current (a canonical
  `folder` change re-points `directoryURLs` — observe-over-wire driving an OS API).

**The assembly script** (`scripts/assemble-app.sh`): SPM release builds → bundle layout →
Info.plists → entitlements → `codesign` (inside-out: appex, then daemon binary, then app). Also
the probe's pricing artifact: its length and its gotcha comments are measurements.

## Non-goals (hard boundaries)

- **No notarization, stapling, DMG, installer, or update story.** Local Developer ID signing only.
- **No real syncing, no FSEvents, no File Provider.** Badges reflect canonical state; if
  FinderSync's API pushes toward per-file state on the wire, record the pressure — do not add the
  verbs.
- **No framework-crate changes** (`git diff crates/` empty at exit) and **expected zero changes to
  the spike's Rust crates** — a forced Rust change is a finding first, a change second (spike
  crates only).
- **No designing the `command` verb, no resolving any §9 question, no ARCHITECTURE edits.**
- **No XPC ladder** unless kill 1 territory is reached — then one rung, recorded, per step 18's R3
  rule.
- **No App Store variant, no persistence, no peer authentication** (all priced/deferred in the
  step-18 report; nothing new here changes that).
- **No Xcode project.** SwiftPM + scripts, per the step-03 rule; `xcodebuild` may appear only
  inside a machine-bound tier as a recorded fallback if R1's appex leg fails hand-assembled (see
  time-boxes). `mise run check` stays exactly as host-only as it is today.

## Deliverables

1. **`spikes/os-integration/apple/finder-citizen/`** — the SwiftPM package: shared wire target,
   `BoltedSyncApp` executable (menu bar + settings + VM as a testable library), `FinderBadges`
   extension executable, headless VM tests that run against a real `syncd` on a temp socket.
2. **`scripts/assemble-app.sh`** — bundle assembly + signing, identity/team discovered as in
   `test-sandbox.sh`.
3. **Two mise verbs**, machine-bound, never in `check`:
   - `run:os:app` — dev tier: build and run the menu-bar app unbundled against a manually-started
     daemon (the UI iteration loop, R5).
   - `test:os:app` — scripted rows: assemble, sign, register, verify the appex is spawned by
     Finder and reaches the socket (process-identity evidence, not vibes), SMAppService
     register/unregister, socket activation through the bundled plist. Partly manual rows are
     documented as a numbered protocol in the report (the step-07/18 precedent: an honest manual
     procedure beats flaky automation).
4. **`docs/steps/step-19-report.md`** + ROADMAP update (19 → done; 20 stays next): answered
   matrix, ceremony verbatim, numbers, friction log, the reconnect-story evidence for the design
   pass.

## Milestones (fail fast: the OS-spawned verdict before the UI is pretty)

- **M0 — scaffold.** Package + shared wire target + walking-skeleton targets; `mise run check`
  untouched and green. Commit.
- **M1 — the OS-spawned verdict (the campaign's remaining riskiest unknown).** Minimal host app +
  minimal appex (no badges yet — it connects, pings, logs); assembly script v1; register + enable
  (R4); open the watched folder; **prove the Finder-spawned extension process reached the daemon**
  (G1–G3) with process-identity evidence. Time-box the appex linking/loading ceremony
  (`NSExtensionMain`, principal class, sandbox keys); if hand-assembly is refused, record the
  refusal verbatim, fall back to `xcodebuild` for the appex only, and keep going — the delta
  between the two ceremonies is itself the R1 finding. Commit.
- **M2 — the UI on the wire.** Menu-bar surface + settings editor + VM tests (U rows, dev tier,
  manual daemon); echo rule, conflicts, submit, check plumbing; keystroke instrumentation for
  kill bar 2. Commit.
- **M3 — SMAppService.** Daemon + plist into the bundle; register/approve/unregister recorded;
  socket activation through the bundled plist; step-18 A-row spot-checks under the new ownership
  (S rows). Commit.
- **M4 — the integrated citizen.** Badges live from canonical state; context-menu toggle with
  fan-out visible in the menu bar; the reconnect/stash story under a real daemon `kill -9` with
  the editor open; idle-exit with surfaces attached (G4/G5, U4, S4). By-hand session mandatory.
  Commit.
- **M5 — numbers + report + ROADMAP.** Commit.

If M1 hits a wall that consumes the session, that verdict alone — reported precisely — is a
successful step outcome (it is kill 1's territory); M2 can proceed regardless since the dev tier
doesn't need the appex.

## Probe matrix (each row ⇒ a test, a scripted procedure, or a numbered manual protocol row)

**G — the OS-spawned extension** *(R1/R2/R4; question 1; kill 1)*
- G1: the hand-assembled `.appex` registers with pluginkit and can be enabled (ceremony verbatim;
  CLI vs System Settings recorded).
- G2: Finder actually spawns it — the extension process exists after the watched folder is opened
  (`pluginkit -m` / process listing; record its sandbox container path).
- G3: **the spawned extension reaches the group-container socket** — a daemon round-trip from
  extension code, with process-identity evidence that the connected peer *is* the appex process
  (e.g. `lsof` on the socket showing the appex pid), not the host app. Control (the step-10/18
  lesson, mandatory before G3 counts): the same extension code path against a socket *outside*
  the container must be refused — proving the extension sandbox is on and G3 is not vacuous.
- G4: badge state follows canonical over the wire — `toggle_paused` from another client flips the
  badge (manual visual row + logged tick-then-fetch evidence); a canonical `folder` change
  re-points the watched directory.
- G5: the context-menu command works — `TogglePaused` issued from Finder's context menu; the
  fan-out push observed by the menu-bar app.

**S — app-owned daemon lifecycle** *(R3; question 2)*
- S1: `SMAppService.agent` registers the bundled plist; the approval UX recorded verbatim
  (prompt text, which Settings pane, how many clicks); `status` reflects reality at each stage.
- S2: socket activation through the bundled plist — first connect spawns the bundled daemon.
- S3: unregister boots the agent out (and re-register works) — no orphaned launchd state.
- S4: spot-check step-18's A2/A3 under SMAppService ownership: still single-instance, still
  respawns after `kill -9`.

**U — the UI over the wire** *(question 3; step-03 rows re-run remotely; kill 2/3)*
- U1: menu-bar state is live — a mutation from another client (syncctl) appears via
  tick-then-fetch without any UI-side polling loop.
- U2: the full editing cycle from SwiftUI — per-keystroke `try_set` with the echo rule (cursor
  survives), keyed errors rendered from wire params (greppable: no constraint literals in Swift),
  the debounced async check begin/complete showing pending/passed/failed, submit success and
  refusal both rendered from returned data.
- U3: conflict over the wire — a second client submits; the open draft's `DraftRebased` push
  arrives; the conflict banner renders mine/theirs/base from snapshot data; keep-mine and
  take-theirs both resolve and the draft submits.
- U4: **the reconnect story** — (a) daemon `kill -9` with a dirty editor open: the VM stashes on
  EOF, reconnects (respawn), restores, and the user's text is intact with verdicts correctly
  reset (C20 visible in a real UI); (b) app relaunch with a stash from the previous run. Record
  the ergonomics — this row is design-pass input, not just a pass/fail.
- U5: keystroke-to-render latency instrumented under real typing (kill bar 2).

## Measurements

Keystroke-to-render p50/p95 under real typing in the settings window (the wire's 45 µs is the
floor; the main-actor round-trip is the question); toggle-to-badge-flip latency (manual stopwatch
precision is fine — the datum is "instant vs noticeable"); SMAppService approval step count;
assembly-script inventory (LOC, number of plist keys, signing invocations — the scaffolding
price); bundle and appex sizes; `test:os:app` wall-clock. Versions: macOS build, Xcode/Swift,
signing identity kind, rustc.

## Kill criteria (hitting one is a successful probe outcome — stop and report)

1. **The Finder-spawned extension cannot reach the daemon** — G3 refused under the real posture
   (with the G3 control proving the refusal is the extension sandbox, not a bug), and R2(a)
   confirms the extension itself loads. The badge leg of the topology is dead as drawn; the
   design session decides (host-app XPC relay, mirrored state file, File Provider, or a narrowed
   promise). Do not improvise a workaround here.
2. **The UI cannot sit on the remote core:** keystroke-to-render **p50 > 16 ms** (one 60 Hz frame)
   under real typing, attributable to the wire round-trip rather than SwiftUI overhead (measure
   both to attribute honestly). This forks the topology answer exactly as step 18's kill 2 would
   have.
3. **The contract cannot stay values-only when a real UI drives it over the wire** — the Swift
   side is forced to re-derive a validity judgement, restate a constraint, or grow judgement
   logic to render. Founding rule; outranks everything.
4. **No rung of app-owned daemon registration works** — SMAppService refuses the bundled agent
   *and* a bundle-carried plist can't be bootstrapped without rung-4 glue (lock files, login
   scripts). Step 18's single-instance answer would then not survive packaging.

## Inherited cautions

- **Every new red watched before it is trusted** (steps 10/13/16/17/18): the G3 control is
  mandatory; the "no constraint literals" grep needs a planted positive control; the reconnect
  test must first be seen failing (e.g. restore skipped) before its green counts.
- **A green suite is not evidence about a running system**: M4's by-hand session — real Finder,
  real badge, real `kill -9` under an open editor — is mandatory, not optional polish.
- **Machine-bound tiers are honest tiers**: GUI rows (badge visuals, approval dialogs) are a
  numbered manual protocol in the report; scripted rows assert on actual output, never a
  wrapper's exit code (`xcodebuild`/`pluginkit`/`codesign`/`launchctl` all lie by omission).
- **Time-box the ceremony quagmires**: appex loading (M1) and Login-Items approval flows (M3) are
  the expected swamps. A precisely-recorded refusal beats a day of thrashing; partial matrix
  honestly reported outranks a complete one quietly faked.
- Commit per milestone; never `git -C`; build/test only via `mise run …`; edition 2024, clippy
  `-D warnings`; the determinism deny-list applies — wall-clock in Swift UI code is fine, but any
  spike-Rust timing needs the local-allow + justification pattern from `3157d43`.

## Exit checklist

- [ ] `mise run check` green, host-only, untouched in scope; `git diff crates/` empty; spike Rust
      crates unchanged (or every change recorded as a finding).
- [ ] Probe matrix: every row has a test, a scripted procedure, or a numbered executed manual
      protocol row — or an explicit justified *not executed*.
- [ ] G3 (the OS-spawned verdict) is empirical, with its control, and the ceremony (R1/R4)
      recorded verbatim — or kill 1 reported with refusals verbatim.
- [ ] SMAppService ceremony recorded end-to-end (register → approve → activate → unregister).
- [ ] U4's reconnect story executed against a real daemon death; ergonomics recorded for the
      design pass.
- [ ] Numbers with versions; kill bar 2 checked with attribution (wire vs UI overhead).
- [ ] `docs/steps/step-19-report.md` written; ROADMAP updated (19 → done). **ARCHITECTURE
      untouched** — no §9 question resolved.

## If you hit a wall

Omitted decision → smallest reversible choice, recorded. Structural conflict (framework change, a
new wire verb, an ARCHITECTURE amendment) → stop and record the question for the design pass. The
kill criteria are the explicit stop-and-report triggers, and hitting one — especially kill 1 — is
a successful probe outcome: the campaign exists to learn exactly that before anything freezes.
