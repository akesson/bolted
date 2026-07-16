# Step 18 — OS-integration spike I: the macOS process-topology probe

**Phase 5 · OS-integration spike. Status: ready.** Read first: [VISION.md](../VISION.md) (bet 2:
native everything — *"real daemons under launchd/systemd"*; risk 2: **"deep OS integration is the
roughest terrain — it must be spiked, not assumed"**), [ARCHITECTURE.md](../ARCHITECTURE.md) (§1
store-owned shape + the demoted `command` verb, §9's process-topology bullet — this step exists to
gather its evidence), [ROADMAP.md](../ROADMAP.md) (working agreement + the Phase 5 campaign sketch).

## Goal

Every shell built in Phases 1–4 has the core **in-process**: linked as a crate (web, Linux) or
FFI-called in the same address space (Swift, Kotlin). VISION's product promise — the same core runs
as the daemon, sits in the menu bar, badges the file manager — breaks that assumption the day it is
kept: a Finder extension is a **separate sandboxed process the OS spawns**, and a daemon runs when
no app does. §9 reserved three questions for exactly this step:

1. **Where does the core run** — embedded per-process, or owned by a daemon that other surfaces
   attach to? (If each surface links its own core, there are N stores and N truths; the contract
   is meaningless across them.)
2. **Can the contract cross a process boundary** — observe / draft / submit / async check over IPC —
   while staying on the verification ladder, or does IPC glue degenerate into the stringly
   runtime-checked kind the founding rule forbids?
3. **Single-instance ownership** — who guarantees one daemon, who starts it, what happens on crash.

As in steps 02/05, **a green suite is not the deliverable; evidence is** — the answered probe
matrix, the measured numbers, and the friction log in `docs/steps/step-18-report.md`. This probe is
**headless**: no menu bar, no FinderSync, no UI (that is step 19, on this step's seam).

**Load-bearing principle (steps 02/05):** every restructuring the IPC layer forces is the probe's
most valuable output. Do not patch `bolted-core` or the framework crates to make the daemon
prettier — record what you had to work around.

## Where this lives: the `spikes/` convention (decided in the planning pass)

This is the project's **second Phase-1-style campaign**, and unlike campaign 1 its artifacts must
stay **disposable** — the daemon skeleton, the wire protocol, the probe clients exist to be learned
from, then deleted once their findings land in ARCHITECTURE. They get a self-contained home:

```
spikes/os-integration/
  README.md            # what this campaign falsifies; disposal criteria
  crates/sync-settings # the vehicle feature (macro-declared, framework path)
  crates/sync-wire     # the hand-written IPC protocol (as-if-generated)
  crates/syncd         # the daemon (pure Rust bin — zero FFI, the Linux-row precedent)
  apple/               # Swift probe client (M3+), self-contained here, NOT under /apple
```

The convention is **forward-only**: the existing `crates/*` stay where they are (they graduated
into harness fixtures; 17+ files hard-code their paths, including byte-checked generated
fixtures — a retro-move is churn without payoff). New spike crates **are** workspace members (root
`Cargo.toml`), so `mise run check` compiles, clippys, and unit-tests them like everything else.
Everything under `spikes/os-integration/` must remain deletable in one `rm -rf` plus a members-list
edit — if something in it becomes load-bearing, that is a finding to report, not a state to accept.

## What the planning pass verified (by reading the code, 2026-07-15)

- **The store shape looks built for this — verify that it is.** `DraftId` is a `Copy` newtype over
  `u64` (`crates/bolted-core/src/store.rs:34`), the store owns every draft (D16), mutations return
  effects **as data** (`submit`/`apply_canonical` return `Vec<DraftId>` fan-outs —
  `store.rs:224,:265`), and the async check is a `begin`/`complete` token pair (D10/D18). Nothing
  in the per-process contract is a pointer, a callback, or a lock. **Hypothesis H1** below is that
  this survives the wire unchanged: `DraftId` as the wire token, effects as pushed data.
- **The session-less mutation is expressible without touching the framework.**
  `Store::apply_canonical(entity) -> Vec<DraftId>` (`store.rs:224`) lets spike code hand-write
  `toggle_paused()` as validate-new-entity → apply. §9 demoted the `command` verb pending "a real
  feature that needs a session-less mutation" — a daemon command ("pause syncing") is plausibly the
  first real customer. **Hand-write it in the spike crate; do not design the verb** (§9 stays open;
  the report banks the evidence).
- **A draft can already outlive a process.** `Store::restore(&D::Stash) -> DraftId`
  (`store.rs:288`) plus D27's versioned envelope were built for stash/restore across app restarts;
  **H6** tests them across *daemon* restarts — arguably the role they were born for.
- **The observe verb has a known IPC-friendly shape.** Step 04 proved a Rust shell wants
  read-direct + a **version tick**, not a snapshot stream. Across processes there is no
  read-direct, but tick-then-fetch (push a small "canonical changed, v=N" notification; client
  fetches the snapshot) is the same race-free pattern with one extra round-trip. Probe row C3.
- **The daemon needs no FFI and no async runtime.** It has no UI, so per VISION's target table it
  consumes the core as a plain crate (the Linux/web rows). A blocking `std::os::unix::net`
  accept loop + thread-per-connection + `Mutex<Store>` is the FFI shells' proven concurrency shape
  (single mutex, never held across an outcall). **No tokio** — an async-runtime choice is a design
  decision this spike must not smuggle in.

## Reconnaissance to falsify (OS claims — treat every one as a hypothesis, not a fact)

These are from documentation and prior knowledge, **not** verified on this machine. Verifying or
refuting them on macOS 15/26-era tooling is probe work, and a refutation is a finding, not a
failure:

- **R1 — launchd single-instance + on-demand.** A user LaunchAgent (label = one instance per GUI
  domain) can be socket-activated via the `Sockets` plist key: launchd owns the Unix socket, spawns
  the daemon on first connect, and re-spawns after a crash. If true, single-instance ownership
  costs zero hand-rolled lock files (rung 3: a plist the build emits). Probe rows A1–A4.
- **R2 — a sandboxed process can reach a Unix socket inside its app-group container**
  (`~/Library/Group Containers/<team>.<group>/…`). This is the cheap, portable transport surviving
  the sandbox. Recent macOS releases tightened group-container access (provisioning-profile /
  first-use-prompt behavior) — the probe must record what the OS actually demands. Probe row C1.
- **R3 — the XPC fallback ladder.** If R2 is false: (a) a mach-service XPC listener the client
  reaches with an entitlement, (b) an XPC service embedded in the eventual app bundle. Both likely
  force libxpc bindings or a Swift shim around the daemon. Only walk this ladder if R2 fails; one
  refuted rung is enough to record — do not build the whole ladder for completeness.
- **R4 — sandbox ceremony.** App Sandbox requires signed entitlements; ad-hoc signing may or may
  not satisfy the group-container path on this macOS. The recorded fallback is the owner's
  Developer ID identity. Whatever the OS demands **is the finding** — VISION's scaffolding promise
  has to pay exactly this cost per app, so price it honestly.

**Pinned assumption (record in the report):** distribution posture is **Developer ID** (LaunchAgent
in the user domain, real Dropbox-style), not Mac App Store. MAS constraints are a design-pass
variant question, not probe scope.

## The vehicle: `sync-settings` (smallest feature that exercises the whole contract over IPC)

Macro-declared via `bolted-macros` — the **framework path**, because the question is whether the
*shipped* contract crosses the wire, not whether hand-rolled code can. Shape (implementer's
latitude on details; smallest reversible choices, recorded):

- 2–3 constrained fields (e.g. a bounded label, a bounded sync interval), ≥1 tier-2 rule, and
  **one async check** driven begin/complete from the client — the check is load-bearing scope: it
  is how C13/C16 and the capability seam get probed across IPC.
- One **hand-written session-less mutation** in the spike crate: `toggle_paused()` —
  validate → `apply_canonical`, returning the fan-out. (§9 evidence; see above.)
- **No real syncing.** No FSEvents, no file IO, no fake engine on a timer. Canonical-change
  pressure for rebase probes comes from a *second client* submitting or toggling — which is the
  honest multi-process story anyway. Canonical state is seeded at daemon start and **not
  persisted** (persistence is a VISION optional battery, out of scope; record the gap).

## The wire: `sync-wire` (hand-written as-if-generated)

Phase-1 doctrine: write the generated code by hand first. The protocol crate is what
`bolted-ffi-gen` would one day emit (D28 already emits foreign source; the friction log here is
that generator's requirements document). Shape:

- Request/response verbs mirroring the store surface (`checkout`, `try_set_*`, rules,
  `begin_check`/`complete_check`, `submit`, `close`, `snapshot`, `version`, `stash`, `restore`,
  `toggle_paused`) + **push frames** (canonical version tick, per-draft rebased/orphaned tick).
- serde_json frames (newline- or length-delimited — implementer's choice) inside a **D27-style
  versioned envelope**: schema version on every frame, parse-don't-validate at the boundary,
  unknown-version = typed refusal. JSON because a spike wants debuggability; the codec is swappable
  and its cost is measured, not guessed (row D).
- **Values only.** The wire layer carries raw field values, keyed errors, ids, and versions. The
  moment it needs a validity judgement or a constraint literal to function, stop — that is kill
  criterion 3, the contract failing to cross.
- **Connection-scoped draft ownership.** The daemon tracks which connection checked out which
  draft; disconnect (including client crash) closes them — C18's `close()` duty crossing the
  process boundary. Whether *other* cleanup policies are wanted is design-pass material; the probe
  ships the strictest one and records friction.

## Non-goals (hard boundaries)

- **No UI, no FinderSync, no menu bar, no SMAppService** — step 19, on this seam.
- **No changes to `bolted-core`, `bolted-macros`, or any framework crate.** If the wire layer
  cannot be built without one, stop and record (kill 3 territory). `git diff` on `crates/` must be
  empty at exit (workspace `Cargo.toml` members edit excepted).
- **No designing the `command` verb, no resolving any §9 question.** Hand-write, measure, report.
- **No generating the wire protocol** — evidence first, extraction later (the D22/D28 road).
- **No tokio / async runtime in the daemon.** std threads + `Mutex<Store>`.
- **No peer authentication / hardening.** A real product must verify who is connecting (peer
  credentials, code-sign requirements); *pricing* that is design-pass input — one paragraph in the
  report, zero code.
- **No Linux, no Windows, no Android services** — Linux is the campaign's step after next; the
  probe must not hedge its transport choice to pre-serve it (portability is a happy accident here,
  a requirement there).
- **No installers, notarization, or update story.** The packaging silence in the docs is known;
  bank observations, build nothing.

## Deliverables

1. **`spikes/os-integration/`** as laid out above, workspace-wired; `check` green with the spike
   crates' unit/integration tests riding the standard verbs. `mise run check` gains **no** new
   external requirement (no Xcode, no codesign, no launchctl in its path).
2. **`syncd`** — socket-listening daemon owning one `Store<SyncSettingsDraft>`; connection-scoped
   drafts; push ticks on canonical change and rebase fan-out.
3. **`sync-wire`** — the envelope protocol crate, unit-tested (round-trip, unknown-version refusal,
   values-only greppable discipline: no `Validity`/`CheckState` judgement names in its source —
   the step-09 `golden.rs` trick, pinned from both sides).
4. **Rust probe client + integration tests** over a real socket: the full probe matrix rows A/B/E/F.
5. **Swift probe client** (`spikes/os-integration/apple/`, SPM executable): decodes the envelope
   with `Codable`, drives one full draft cycle, measures row D from Swift. Sandboxed variant for
   row C — signed with App Sandbox + app-group entitlements.
6. **launchd tier** — a plist (generated into the spike dir, installed by a mise task), and
   `test:os:launchd` (or a scripted, documented procedure if full automation fights the GUI
   domain): activation, single-instance, crash-respawn, idle-exit probes. **Machine-bound and
   double-gated** like `bench:android:device`; never inside `check`.
7. **`docs/steps/step-18-report.md`** + ROADMAP update: answered probe matrix, numbers with
   caveats, friction log, the §9 evidence (topology, wire, single-instance, `command` customer),
   design-pass questions.

## Milestones (walking skeleton first; the sandbox verdict early, not last)

- **M0 — scaffold.** `spikes/os-integration/` + workspace members + README (charter + disposal
  criteria). `mise run check` green, host-only, shape unchanged. Commit.
- **M1 — the vehicle.** `sync-settings` via the macros, `toggle_paused` hand-written, in-process
  unit tests (checkout/edit/submit/check/toggle). Commit.
- **M2 — daemon + wire.** `sync-wire` + `syncd` + Rust client; integration tests over a real Unix
  socket: full draft cycle, async check begin/complete, two clients (B and E rows), disconnect
  pruning. Commit.
- **M3 — the sandbox verdict (fail fast — this is the campaign's riskiest unknown).** Swift client
  unsandboxed first (proves the envelope in `Codable`), then sandboxed + app group against a
  manually-started `syncd` listening in the group container (no launchd needed). **Time-box the
  entitlement ceremony**; if blocked, record precisely where the OS said no, walk one rung of R3,
  and report honestly — an "unreachable, here's why" verdict is a *successful* probe outcome
  (kill 1). Commit.
- **M4 — launchd lifecycle.** Socket activation (R1), single-instance (second bootstrap attempt),
  crash-respawn (`kill -9` the daemon mid-session), **H6**: client stashes its dirty draft, daemon
  dies, restore into the fresh daemon over the reopened socket. Commit.
- **M5 — the numbers.** Row D measured from Rust and Swift clients; kill bar 2 checked. Commit.
- **M6 — report + ROADMAP.** Commit.

If M3 or M4 hits a wall that consumes the session, M2's matrix rows already stand on their own —
report what ran, mark the rest *not executed*, per the step-05 precedent.

## Probe matrix (each row ⇒ a test or a scripted, reproducible procedure; record observed behavior)

**A — Topology & lifecycle (launchd)** *(§9 question 3; hypothesis R1)*
- A1: socket activation — client connects to the launchd-owned socket, daemon spawns, answers.
- A2: single instance — a second `launchctl bootstrap` / manual spawn cannot create a second
  authority (record the exact refusal mode).
- A3: crash-respawn — `kill -9`, next client connect respawns; **all pre-crash state is gone**
  (assert it: fresh canonical, zero drafts — this is what makes H6 matter).
- A4: idle-exit — daemon exits after idle (if implemented) and the *next* connect still works;
  or record that idle-exit is declined and why.

**B — The contract over IPC** *(§9 question 2; H1)*
- B1: full draft cycle remotely — checkout → `try_set` (invalid → keyed error with structured
  params intact **through the envelope**) → valid → tier-2 rule → submit → canonical version bumps.
- B2: async check remotely — begin returns a token; complete with stale token is discarded
  (single-flight semantics hold when the driver is a different process); C16 refusal reaches the
  client as data.
- B3: `toggle_paused` — session-less mutation round-trip; fan-out push observed by the other client.
- B4: draft-id hygiene — a forged/stale `DraftId` from a *different connection* gets a typed
  refusal, not another client's draft (the D23 shape, now with a security flavor — record, don't
  harden).

**C — The sandbox (the roughest terrain)** *(R2/R3/R4; kill 1)*
- C1: sandboxed Swift client reaches the group-container socket. Record signing identity,
  entitlements, and any OS prompt verbatim.
- C2: the same client is *refused* outside the group container (control — proves the sandbox is
  actually on; without it C1 is vacuous, the step-10 lesson).
- C3: tick-then-fetch works sandboxed (a canonical change from the Rust client is observed).

**D — Chattiness** *(kill 2; step-05 method: floor first, then attribute)*
- D1: `ping` round-trip over the socket — the floor (framing + syscall, no core work).
- D2: `try_set` p50/p95 over ≥1000 calls; D3: `snapshot` fetch; **D4: keystroke pair
  (`try_set` + `snapshot`) p50** ← the bar. From the Rust client and the Swift client both.

**E — Multi-client (the reason the daemon exists)**
- E1: client A submits; client B's dirty draft is rebased; B receives the push tick and fetches a
  snapshot showing conflict state — live rebase across process boundaries.
- E2: disconnect pruning — drop A's connection with drafts open; store's `draft_count` falls
  (C18 across the wire; H5). Kill -9 the client process variant.

**F — Draft survival (H6; D27 in its destined role)**
- F1: stash → daemon `kill -9` → respawn → restore over new connection → dirty values and
  conflict-relevant base survive; sync/verdict state reset exactly as C20/C21 specify in-process.

## Measurements

D1–D4 (p50/p95); envelope frame sizes for a `try_set` and a `snapshot` (the codec-cost datum for
the design pass); daemon RSS after boot and after 100 draft cycles; `syncd` binary size (stripped);
wall-clock for `test:os:launchd`. Versions: macOS build, Xcode/Swift, signing identity kind, rustc.
No pass/fail thresholds except kill bar 2 — this is a baseline.

## Kill criteria (hitting one is a successful probe outcome — stop and report)

1. **The sandbox cannot reach the daemon at all** — group-container socket refused *and* the R3
   ladder's first rung refused, under a real signing identity. The extension leg of the topology is
   dead as drawn; the design session decides between mirrored read-only state, an embedded XPC
   relay, or narrowing the promise. Do not improvise a workaround here.
2. **Chattiness:** keystroke pair (D4) **p50 > 1.0 ms** over the local socket. Desktop-class
   machine, no slower-hardware discount (unlike step 05): a miss means interactive UI cannot sit on
   a remote core, and the topology answer forks (UI embeds the core; daemon serves background
   surfaces only). *(Calibration: expect tens of µs; the bar is ~20× that. It should not fire.)*
3. **The contract cannot stay values-only on the wire** — the protocol layer is forced to make a
   validity judgement, restate a constraint, or grow a judgement enum to function. That is the
   founding rule failing at the process boundary; it outranks everything here.
4. **launchd cannot own single-instance** for a plain executable without hand-rolled lock files —
   rung-4 glue at the very root of the topology. (R1 refuted. Unlikely; record the evidence if so.)

## Inherited cautions

- **Every new red watched before it is trusted** (steps 10/13/16/17): C2 is mandatory before C1
  counts; the values-only grep needs a planted positive control; the stale-token discard (B2)
  verified to fail with the guard removed.
- **A green suite is not evidence about a running system** (memory:
  `bolted-verify-in-a-real-browser`, generalized): M4 must include one by-hand session — launchctl
  bootstrap, real client, `kill -9`, restore — not only scripted assertions.
- **Machine-bound tiers are honest tiers** (step-07 KC4, `bench:android:device` precedent):
  `test:os:launchd` may be partly manual; a documented reproducible procedure beats a flaky
  automation that lies.
- **The exit-code trap** (memory: `test-android-exit-code-masks-failures`): if any tier shells out
  to `xcodebuild`/`swift test`/`launchctl`, read the actual results, not the exit status of a
  wrapper.
- Commit per milestone; never `git -C`; build/test only via `mise run …` verbs; edition 2024,
  clippy `-D warnings`, no `unwrap`/`expect`/`panic!` in library code (probe *tests* may).

## Exit checklist

- [ ] `mise run check` green, host-only, no new external tool in its path; framework crates
      untouched (`git diff crates/` empty).
- [ ] `spikes/os-integration/` self-contained and deletable; README states charter + disposal
      criteria.
- [ ] Probe matrix: every row has a test, a scripted procedure, or an explicit justified
      *not executed*.
- [ ] The sandbox verdict (C1/C2) is **empirical**, with the exact ceremony recorded — or kill 1
      is reported with the refusals verbatim.
- [ ] Numbers recorded with versions; kill bar 2 checked from both clients.
- [ ] H6 (stash across daemon death) has an empirical verdict.
- [ ] `step-18-report.md`: answered matrix, friction log, §9 evidence (topology / wire /
      single-instance / the `command` customer), the design-pass question list; ROADMAP updated
      (18 → done, campaign sketch adjusted). **ARCHITECTURE untouched** — no §9 question is
      resolved here.

## If you hit a wall

Omitted decision → smallest reversible choice, recorded. Structural conflict (framework-crate
change, new invariant, an ARCHITECTURE amendment, kill 3) → **stop and record the question** for a
design session. The sandbox ceremony (M3) and the launchd GUI-domain quirks (M4) are the expected
quagmires: time-box each, and remember a partial matrix honestly reported outranks a complete one
quietly faked — "no sandbox evidence" is a legitimate, useful verdict.
