# Step 27 — bolted-http IV: the ruled contract, implemented

**Status: ready.** The 2026-07-21 contract-review session ruled on all ten open contract
questions ([decision record](../design/contract-freeze-agenda.md)); this step implements the
rulings that change code, across the contract crate and all three shipped adapters. The
headline is the **streaming seam** ([design doc](../design/streaming-seam.md), adopted as
proposed) — the rest of the bundle rides along because each item is small and they share
verification surface: one mid-flight signal shape (Q4 + streaming §3b), the redirect-ceiling
CFG (Q2), the `content_length` wording + file-sink verified total (Q3), the
`HttpError → Into<ErrorData>` bridge (Q6), the row-11 `total` assertion (Q8), and the
priority-hint uniformity + FFI bridge-crate merge (Q10).

## Decisions already taken (do not re-litigate)

- **The seam's shape is decided** — [streaming-seam.md](../design/streaming-seam.md) §3a–3d:
  chunks re-enter as a **new typed-input family** (token-keyed, `seq`-stamped, verified
  ascending/gapless on arrival), the core owns a **bounded per-response ring** whose overflow
  is the typed `StreamOverflow { capacity, seq }` failure (never silent loss), the terminal is
  a **separate** `BodyEnd { Complete { total } | Failed(HttpError) }` re-entry with the
  completeness gate `total == ingested`, and the raw `ffi_stream` subscription is **rung-2
  internal** with driver-owned deterministic close. Names in the doc are sketches; final
  naming is this step's smallest-reversible territory. §7's upstream-RFC trigger is a
  *future* re-evaluation — build the adopted shape now, structured so enforcement can later
  delegate downward without touching contract types.
- **Back-pressure (§3b option C) is a capability-shaped extension, and it is the same surface
  as push-cancellation (Q4)**: one core→adapter mid-flight signal shape, two uses
  (pause/resume reading; cancel). The poll-watcher thread all three adapters pay today is
  deleted. The cookie per-hop re-entry (Q9) is a named *future* third instance of the
  adapter→core direction — design so it can attach, do not build it.
- **Redirect ceiling is CFG** (Q2): a core-owned value at the composition root; the adapter's
  native limit is set above the ceiling; the core counts hops from the trace and emits
  `TooManyRedirects` itself. The OkHttp exception-text match in the classifier is deleted.
- **`content_length` is advisory by protocol arithmetic** (Q3, re-verified): wording lands in
  the rustdoc; the **file sink reports verified bytes-written on completion** — that number
  is adapter-counted truth, distinct from the advisory header value.
- **`PriorityHint` goes uniform** (Q10): the CAP marker trait is deleted, `Priority` stays a
  plain request field, a no-op where the engine can't honor it (row 12's acceptance-only
  conformance). With the sole surface divergence gone, `bolted-http-apple-ffi` and
  `bolted-http-android-ffi` **merge into one multi-target bridge crate**
  (`gen-profile-ffi` packs apple+android+csharp — the precedent).
- **`HttpError: Into<ErrorData>`** (Q6): the D1-shaped bridge (variant → snake_case key,
  fields → params), same as every tier-1 error.
- **Conformance scope** (Q8): the shared suite owns what the contract types can observe;
  platform-internal invariants are named per-adapter unit obligations. Rule 11 additionally
  asserts the terminal `total`. Three new rows (feature-matrix §7 rules 12–14): slow-consumer
  completeness, terminal-exactly-once, subscription hygiene.

## Scope

`crates/bolted-http` (types, core seam, suite), `crates/bolted-http-linux`, the two FFI
bridge crates (→ one), `BoltedHttp.swift`, `BoltedHttp.kt`, and the mock implementor.
Everything else — cookie capability, background family, C#, SSE/WebSocket, the
harness-hardening track (tier-provided sink path, row hard-kill, ALPN TestServer) — is out
(§Non-goals).

## Milestones (one Opus sub-agent each; Fable reviews between)

- **M0 — the note-08 runtime probe, then the bridge-crate merge (Q10).** First, the ~15 s
  probe [upstream note 08](../../upstream/boltffi/08-bindgen-ignores-cfg-attributes.md) still
  owes (step-10 `probe.sh` style): a `#[cfg(target_os = "ios")]`-gated `#[data]` item in a
  scratch crate, packed for android — confirm the item lands in the Kotlin bindings (union
  behavior) and record the verdict in the note. Then: delete the `PriorityHint` marker trait
  (row 12 uniform — `Priority` field stays, docs updated), merge the two bridge crates into
  one multi-target crate, repoint `pack:*`/package manifests, delete the dead crate dirs.
  **Gate:** `mise run check` + both platform tiers green (Apple by count, Android by JUnit
  XML) on the merged crate before anything else builds on it.
- **M1 — contract types + the core seam (host-side).** The chunk input family
  (`deliver_chunk`-shaped re-entry, `seq` verified ascending/gapless), the bounded ring with
  core-owned capacity (a constraint value — never a shell literal), `StreamOverflow` as a
  typed `HttpError` variant, `BodyEnd` terminal + completeness gate, exactly-one-terminal
  enforced by construction where possible (the step-24 one-shot discipline extended). Plus
  the small rulings: redirect-ceiling CFG + core-counted `TooManyRedirects`,
  `Into<ErrorData>` for `HttpError`, `content_length` rustdoc wording, file-sink
  verified-total on completion. Mock implementor exercises all of it; every new host test
  watched red first.
- **M2 — the mid-flight signal + the Linux adapter + the host-side rows.** The one
  core→adapter signal surface (pause/resume + cancel push); `bolted-http-linux` onto the full
  seam (chunked delivery via `bytes_stream`, socket read-pacing on pause, poll-watcher
  deleted); new suite rows 12 (slow-consumer completeness) and 13 (terminal-exactly-once) on
  mock + Linux; rule 11's `total` assertion on all implementors; the redirect-exhaustion row
  re-pointed at core counting. Each row watched red per implementor.
- **M3 — the Apple adapter graduates.** `BoltedHttp.swift` onto `deliver_chunk`/`finish_body`
  (delegate `didReceive data` per chunk), driver-owned close as shipped code (step 25's
  explicit `close_chunk_stream()` fix becomes the contract path), cancel/pause via the new
  signal, poll-watcher deleted. Rows 12/13 on the macOS tier + row 14 (subscription hygiene —
  the F-M3-1 leak is the red case). A1's probe machinery graduates into the rows.
- **M4 — the Android adapter graduates.** Same shape on `BoltedHttp.kt` (OkHttp source →
  JNI push), N2's machinery graduates, `StreamProbe.kt`'s stale pre-0.28.0 `trySend` comments
  cleaned in passing, the OkHttp redirect text-match deleted (now core-counted). Rows
  12/13/14 on the instrumented tier — counts read from the JUnit XML, never the exit code.
- **M5 — mutation pass + report** (mutations: ring bound, seq check, completeness gate,
  terminal-exactly-once, ceiling counting, signal wiring; two-hypotheses discipline on every
  survivor). `step-27-report.md` + ROADMAP row (planning session writes the report).

## Kill criteria (real; if hit, stop and report)

1. The M0 probe shows bindgen behavior **other than** the union claim in a way that breaks
   the single-crate merge (e.g. cfg-gated items abort the scan) → stop M0, revise note 08,
   report; the merge decision returns to planning.
2. The seam cannot be expressed within `bolted-http`'s sans-io/no-lock discipline (the ring
   forces a lock or an executor into the contract crate) → stop; that is a design session,
   not a workaround.
3. A new suite row cannot be made to go red on some implementor (no reachable positive
   control) → stop that row and report — a forbidding test that can forbid nothing is the
   known trap; do not ship the row green-and-vacuous.
4. Any change that would touch ARCHITECTURE §1–§7 or resolve a §9 question → stop, record,
   leave for a design session.

## Non-goals (→ elsewhere)

Cookie capability implementation (Q9 defined the shape; a future feature triggers it).
`BackgroundTransfer`. Anything C# (the S-WIN resume is its own step; SkipReason's
keep-or-delete decision rides it). SSE/WebSocket. The harness-hardening track (tier-provided
sink path, row hard-kill, ALPN TestServer) — its own step. Upstream RFC work and upstream
filings (Henrik's alone). Re-evaluating the seam against the stream RFC (streaming-seam §7 —
*after* an upstream release ships it, not now).

## Inherited cautions

- Build/test only via `mise run …`; the instrumented tier's exit code has masked failures —
  gate on the JUnit XML. Force `--rerun-tasks` before quoting a Gradle number.
- Sub-agent runs of long tiers: synchronous foreground (`run_in_background: false`), tee to a
  log, on timeout read the log — never re-launch over a live run.
- `cargo install --list` must show `boltffi_cli v0.28.0` with no `?rev=` before any pack.
- The F-M3-1/F-M0-5 leak is **unfixed at 0.28.0** — row 14's red case is real; a GC-dependent
  probe needs a `ReferenceQueue`, not a `WeakReference` poll.
- Kotlin/Swift: no constraint literals (ring capacity, ceiling — all from the core); adapters
  map errors, never invent keys.
- Rust: edition 2024, clippy `-D warnings`, no `unwrap`/`expect`/`panic!` in library code;
  `bolted-http` stays dependency-free on its default build.
- Regenerated FFI surfaces: dists are **gitignored** — verify changes in freshly-generated
  output, not via git diff; inspect C# symbols under `target/aarch64-apple-darwin/debug/`.
- If a decision is missing here: smallest reversible choice, record it in the report. If
  structural: stop, record, leave for a design session.

## Exit checklist

- [ ] `mise run check` and `mise run test` green; Apple tier green by count; Android tier
      green **by JUnit XML**.
- [ ] One bridge crate; `PriorityHint` trait gone; note 08 carries the probe verdict.
- [ ] Rows 12/13 green on mock + Linux + Apple + Android; row 14 green on Apple + Android;
      every new row watched red first, per implementor.
- [ ] Poll-watcher threads deleted on all three adapters; OkHttp redirect text-match deleted.
- [ ] `step-27-report.md` (built / deviations / friction log / open questions) + ROADMAP row.
