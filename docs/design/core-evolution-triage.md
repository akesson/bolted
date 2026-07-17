# Triage: `design/core-evolution` deltas against ARCHITECTURE v1.8

> **What this is.** The per-delta merge triage for
> [`architecture-core-evolution.md`](architecture-core-evolution.md) (the branch's preserved
> ARCHITECTURE snapshot, base `8ecc1c9`), prepared as input to the design session that decides
> the branch's fate. Each delta is classified against main's authoritative **v1.8** and the
> evidence steps 02–16 produced after the snapshot was written. Classifications:
> **merge-worthy** (compatible, low conflict), **live-with-conflict** (a real design decision
> is required first), **park in §9** (real but premature to make normative),
> **superseded** (dropped, with the superseding decision named), **owner-decision**
> (Henrik's alone). Nothing here changes ARCHITECTURE.md; that is the design session's output.
>
> **Adjudicated (July 2026 design session → ARCHITECTURE v1.11).** T3c → **D35** (no ambient
> nondeterminism; no C-ID minted — the deny-list is the static rung, the runtime face is replay's
> first artifact) · T5 → the restored §9 replay item, preconditions updated for D16 and the daemon
> topology · T3b → **D36** (frame loop never crosses the FFI) · T1 → **D37** (`observe` is
> watch-shaped; the `[Pending, Passed]` delivery downgraded to a driver fact — the wrapper test
> rescoped, assertions unchanged) · T2 → parked as §9's windowed-collections item · T7 → **D38**
> plus the §9 freeze-gate item, unscheduled until a feature needs HTTP · T6 (`facet`) → split into
> its own PR.

## T1 — Watch-shaped observation (`Latest<T>`) · **live-with-conflict**

The snapshot's claim: observation has exactly one operation — read/await the newest value —
so coalescing is *legal by type*, intermediates are unobservable, renderers decouple from
emission rate, and delta protocols are unrepresentable.

**What v1.8 says instead: nothing.** D11 settled *where* observation lives per target (a
stream across FFI, a direct read + tick in Rust shells) but not its *semantics* — v1.8 nowhere
states whether a consumer may depend on seeing intermediate snapshots. That question is
currently answered by accident of mechanism, not by contract.

**The conflict that must be adjudicated before merging.** v1.8's §9-closed item "a real
`Pending` across FFI" rests on `gen-profile-ffi`'s
`a_check_in_flight_is_observably_pending`, which asserts the **sequence** `[Pending, Passed]`
off the subscription — an intermediate-observability guarantee. Under `Latest`, coalescing a
fast check's `Pending` away is *legal*, so adopting the contract as written either

- (a) downgrades that test (a spinner is guaranteed only while the check is genuinely
  in flight at read time), or
- (b) splits the contract: `Latest` for state snapshots, a stronger delivery guarantee for
  check sub-state transitions.

Neither is obviously right; this is the design session's first question.

**Evidence since the snapshot.** Both step-02 probes bear on implementability, and they
agree the contract is achievable either way: main's probe confirmed the two-layer shape
(bounded drop-newest ring drained eagerly into an unbounded Swift `AsyncStream` — effectively
a lossless queue under normal load) plus the D7/C15 version-stamped reconcile for the
subscribe race; the branch probe (re-run at 0.27.5, `crates/spike-profile-ffi-stall-probe/`)
confirmed both a default-capacity stream and a capacity-1 wake-and-read encoding converge on
the final value. So BoltFFI constrains nothing here — the question is purely **what shells
may rely on**, which is exactly what a contract is for. Note the secondary win if adopted:
D7/C15's reconcile dance simplifies (read the latest, done).

## T2 — Windowed collections (`CollectionWindow`) · **park in §9**

No counterpart exists anywhere in main's docs, and no spike has a collection feature — zero
examples. Merging the sketch (`open_window(range)`, `WindowSnapshot { version, total_count,
range, rows }`, stable `RowId`s, deltas-never-cross-FFI) as normative §1 text would violate
the doctrine main itself sharpened twice since the snapshot (D20, D29: never design from zero
examples). But the sketch is real prior thinking with a real rejected-alternative argument
(delta protocols die on coalescing/reordering), and losing it to a dead branch would be waste.

**Recommendation:** a §9 OPEN item — "collection observation: designed when the first real
collection feature lands" — carrying the snapshot's sketch as the candidate answer, the same
move D29 made for `command`. Note the dependency: the sketch presupposes T1 (`Latest` per
window); if T1 resolves against watch-shaping, the window design reopens wider.

## T3 — The synchronous reduce loop · **superseded (D29)**

The `FacetCore::dispatch` sketch — `update(state, msg)`, recompute snapshot, value-diff,
publish — is written against the `Facet (State/Msg/Effect/update)` trait, which is precisely
what D29 **struck** after six spikes implemented it zero times. The store-owned shape v1.8 §1
describes is the code that actually shipped. Effects-as-data, the good half, already survives
in v1.8's sans-io row. Drop the loop; nothing to merge. (The `Ctx` question dies with it —
see T3c for what survives without `Ctx`.)

## T3b — "The frame loop never crosses the FFI" · **merge-worthy**

Cleanly separable from T3 and not Elm-shaped at all: continuous gestures keep the value
native while live and commit at boundaries; scroll uses overscan + threshold refetch, never
per-frame calls; core-driven churn is conflated at the driver; sparse snapshots animate via
native implicit animation. It is the **generalization of the echo rule v1.8 already has**
(D9, §6) and is validated by the same spike evidence — the 12–13 µs per-keystroke measurement
justifies per-*event* crossings, not per-*frame* ones, and the distinction deserves to be
stated. No conflict with anything in v1.8. Candidate landing: a §6 platform note + a §8 row
(the snapshot has both drafted, including the rejected per-frame-round-trip alternative and
the TCA citation).

## T3c — No ambient nondeterminism in the core · **merge-worthy, with a dangling reference to fix**

The rule: no clocks, no randomness, no ID generation inside core crates — time and identities
arrive as inputs, so core state evolution is a pure function of its input sequence. v1.8's
sans-io paragraph covers runtime/effects but **not** this; yet the rule is *already true of
the code* and *already enforced*: the branch ships the workspace `clippy.toml` deny-list
(`SystemTime::now` / `Instant::now` disallowed) riding `-D warnings`, green on the enlarged
workspace. Main's `CheckToken` begin/complete flow is this rule in action already.

**The dangling reference:** the committed `clippy.toml` justifies itself with "ARCHITECTURE
§5" — but v1.8 §5 says nothing about determinism or time; the text it cites exists only in
the snapshot. Whatever the session decides, this must be reconciled: merge the rule text into
v1.8 (recommended — it costs nothing and is replay precondition (1), see T5), or repoint the
comment. The snapshot also proposed promoting the rule to an invariant (a C-ID with a test);
that is a CONFORMANCE.md change and is the session's call.

The snapshot tied the rule to a `Ctx` argument on the struck `update` fn; the rule survives
without it — inputs arrive as verb arguments and effect completions, which is how the shipped
code already works.

## T4 — "Rules as artifacts" table · **superseded as written**

The doctrine (every load-bearing rule is carried by an artifact on the verification ladder;
prose is commentary) is VISION's founding rule, and post-snapshot main *lives* it harder than
the table does — the conformance drift check, byte-compared generated source (D22/D28), the
C-ID↔function↔macro mapping verified by the build. The table's specific rows meanwhile name
artifacts that don't exist in v1.8 (`Latest`, `CollectionWindow`, `Msg`, `FieldBinding`).
Don't merge it. If T1/T2 are adopted, their carried-by rows get written then, against the
artifacts that actually land.

## T5 — Interaction-replay preconditions · **merge-worthy (restore to §9), strengthened since**

The snapshot's §9 item is a *protected possibility*: replay is unscheduled, but three
preconditions must survive other decisions — (1) no ambient nondeterminism, (2) stable
logical identities for handles/tokens, (3) a total order over inputs. v1.8's §9 **does not
carry the item**, so nothing currently stops a future decision from foreclosing it — which is
the only job the item has.

What changed since the snapshot, all favorable:

- **(2) is now structurally satisfied**: D16's `DraftId`s are `Copy`, monotonically issued,
  never reused — exactly the stable-logical-identity the item demanded, decided for
  independent reasons. `CheckToken`'s private seq is the same shape.
- **(3) is satisfied behind FFI** (the one `Mutex` serializes) but **not for lock-free Rust
  shells** holding the store by value — the restored item should record this nuance instead
  of the snapshot's pre-D16 actor argument.
- **(1)** is T3c: enforced by the deny-list, unstated in v1.8.

Restoring the item is additive prose in §9; recommended regardless of T1/T2 outcomes.

## T6 — The `facet` vocabulary · **owner-decision**

Glossary admission is Henrik's alone; this entry only records the argument state. New since
the branch wrote it: v1.8 itself now documents that the name `Feature` is **taken** — D25/D29
assign it to `bolted_decl::Feature`, the declaration model — while §1 simultaneously uses
"feature" for the domain unit shells observe. One word, two load-bearing meanings, in the
same frozen document. The branch's `facet` (approved on the branch; `docs/GLOSSARY.md` entry
rides this branch) breaks that collision. Cost if adopted: a vocabulary sweep across
ARCHITECTURE/CONFORMANCE/VISION and the *forward-looking* docs only (historical step
docs/reports keep their words); the sweep is mechanical and greppable. Cost if declined: the
collision stands and every future doc disambiguates by context.

## T7 — `bolted-http` (§8 row + §9 freeze-gate item) · **merge-worthy, evidence improved**

The crate, its design docs (`crates/bolted-http/docs/`), and the proven packaging spike ride
this branch; the snapshot's §8 decision row (sans-io contract crate + Bolted-shipped
shell-side adapters — URLSession/OkHttp/WinRT, Rust adapters only for Linux/web) and §9
freeze-gate item exist nowhere in main. Two triggers named in the row have since resolved
*in its favor*:

- The "recorded retreat if step-02 callback measurements come back ugly" clause is dead:
  main's step-02 confirmed callback traits cheap and reentrancy-safe (no deadlock, no lock
  held across outcalls).
- The freeze-gate's response-streaming question has fresh evidence: stream machinery
  converges at 0.27.5 on both probes (T1).

Remaining §9-item content still genuinely open: cookie capability shape, Android `<pin-set>`
binding, `BackgroundTransfer` (whose durable-effects precondition is shared with T5 and draft
stash — the item's cross-link is worth keeping). Scheduling is a ROADMAP question: the 17+
range is explicitly unplanned; `bolted-http` is a candidate alongside the listed harness
items, or parks until a feature needs HTTP.

## Not triaged here

- **The stall-probe crate's fate** (keep as historical evidence vs delete now the finding is
  recorded and moot at 0.27.5) — housekeeping, not architecture; decide at merge time.
- **The branch's ROADMAP edits** — dropped at rebase as superseded; nothing to triage.
- **Step 14's C# resume and the upstream `MarshalAs(I1)` filing** — main's own thread,
  untouched by this branch.

## Suggested session agenda (smallest-decision-first)

1. **T3c + T5** — additive, no conflicts, fixes the dangling `clippy.toml` reference:
   merge the nondeterminism rule into §5 and restore the replay item to §9. (Decide the
   invariant-promotion question while there.)
2. **T3b** — additive: the frame-loop note into §6 + its §8 row.
3. **T1** — the real decision: adjudicate `Latest` vs the `[Pending, Passed]` guarantee.
   Everything else that mentions observation semantics waits on this.
4. **T2** — park in §9 (shape depends on T1's outcome).
5. **T6** — glossary call (the `Feature`-collision argument is new input).
6. **T7** — merge the row + item; decide ROADMAP placement.
7. Merge `design/core-evolution` → main; version-bump ARCHITECTURE accordingly.
