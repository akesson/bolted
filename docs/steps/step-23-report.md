# Step 23 — report: **stopped on kill criterion 3**

**Status: killed (KC3 — the pinned rev introduces a new four-feature break on the C# backend).**
First step executed under the Fable-orchestrates model (planning session drives, Opus
sub-agents implement per milestone). M0 completed and is committed on
`step/23-boltffi-git-pin` (`6a34e6e`); M1 ran the ladder, delivered the verdict, hit KC3, and
correctly stopped — nothing beyond M0 is committed, the tree was left clean.

## What was built (M0 — banked, reusable)

- The four workspace `Cargo.toml` pins → `{ git = "https://github.com/boltffi/boltffi", rev =
  "23cf2ecce20327581a0d03b41aee6af9cd081ea3" }`; lock updated (7 boltffi packages moved to the
  git source; +2 transitive deps: `proc-macro-crate 3.5.0`, `toml_edit 0.25.13`).
- `setup:boltffi` git flavor: `cargo install --git … --rev …`; CARGO_HOME canonicalization
  kept; idempotence check now greps `cargo install --list` for the rev (the real output shape
  is `boltffi/boltffi?rev=<full-rev>#<short>` — the step doc's sketch omitted the `?rev=`
  query segment).
- Doctor rev cross-pin: new `BOLTFFI_PINNED_REV` const; `doctor_manifest.rs` greps the single
  `rev="…"` line from mise.toml with the same non-vacuity guard as the retired `want="…"`
  scan. `BOLTFFI_PINNED = "0.27.5"` kept as human display only.
- `gen:ffi` artifacts all byte-identical (expected — it runs the in-repo `bolted-ffi-gen`;
  CLI churn only reaches `dist/` via the packs). `mise run check` green: 414 passed / 67
  suites / 0 failed.

**All of M0 is rev-parameterized: re-pinning at a fixed rev is a one-literal change (plus the
rev const). The branch is parked, not discarded.**

## The verdict (M1) — both findings, verified

1. **Finding 06 (MarshalAs) is FIXED at the pinned rev.** The tripwire
   `TheCheckDriverIsBrokenOnThisBackend` went red for exactly the right reason (TRX:
   `Expected: <MarshalDirectiveException> But was: null`). The IR backend moved the bool
   payload to an `out` parameter (`FfiBuf …run_username_check(uint64_t, bool *return_out)` in
   `boltffi.h`); no return-MarshalAs on the envelope; the attribute survives only on the
   genuinely-bool `is_live`. Step-14's parked D23 probe would have come alive
   (`RunUsernameCheck()` throws `DraftClosedFfiException` on a closed draft) — moot under the
   kill.
2. **KC3: the same PR (#654) regressed streams on C#** — new upstream finding **07**
   (`upstream/boltffi/07-csharp-ir-backend-collapses-same-named-streams.md`). The IR backend
   dedupes `#[ffi_stream]` bindings by unqualified method name: our two `snapshots` (store +
   draft) collapse into the store's; `draft.Snapshots()` silently subscribes to the canonical
   stream with a draft handle and never delivers. Verified at every layer: C header declares
   both subscribe symbols, the dylib exports both (`nm -gU`), the generated `NativeMethods`
   lacks the draft's. `snapshots_small` survives (unique name). TRX: 14 total, 11 passed, 3
   failed — the tripwire (expected) + two StreamProbe timeouts (`Expected "zoe" … But was
   null`). Cross-controls: Swift is green at the same rev (C#-binding-only) and the probes
   were green at 0.27.5 (genuine regression).

The probe was **not** flipped and no dist was patched — KC3 fired first; flipping would have
worked around a kill criterion.

## Tier results (counts artifact-derived)

- `check` green (414/67/0). `test:apple` (+`:gen`) **green, exit 0** — Swift streams work at
  the pinned rev. `test:csharp` **red as reported above** (the verdict's evidence).
- `test:web` **environmentally blocked** — chromedriver/wasm-runner failures (SIGKILL, then
  404); the web crate is zero-FFI and never links the pin, so this is machine browser-tier
  drift, not a regression. Needs separate environment upkeep.
- `test:android` tiers **not run** — pointless expense after the kill. (Pixel 8a not
  currently attached; the Android tiers here use the headless GMD emulator anyway.)
- Kotlin dist churn **unassessed** (packs stopped at the kill).

## C# churn log (the cost-of-lagging evidence the step wanted)

The IR backend is a rewrite, not a patch: namespace + top-level class renamed `GenProfileFfi`
→ `Gen_profile_ffi` (raw crate name — broke every hand-written probe `using`; a pure
mechanical rename, applied to observe the tiers, then reverted); the monolithic
`GenProfileFfi.cs` split into ~35 per-type files plus one runtime file (`NativeMethods`,
stream runtimes, `BoltFFIResult<TOk,TErr>`, `BoltException`); all type/member names otherwise
preserved (DU records, `<Value>ErrorFfiException` family, store/draft verbs). Whenever the
resume lands, expect: the namespace rename in all probe files, the ABI change on
`run_*_check` (out-param bool), and re-verification of finding 07's fix as the first check.

## Deviations from the step doc

- M1 stopped before `test:android`/Kotlin pack (justified: KC3 already decided the step).
- No commit for M1 (correct for a killed milestone; evidence lives in this report and the
  kit).
- The report + kit updates are committed on `design/bolted-http` (planning branch), not the
  parked step branch, so they survive the pin decision either way.

## Friction log

- **Piping a tier through `tail` masks its exit code and truncates counts** — an M1 first-run
  mistake, self-caught: a compile failure looked like exit 0. Generalizes the `test:android`
  caution: never pipe a wrapper when its exit code is load-bearing; read the artifact.
- The step doc's `cargo install --list` sketch had the wrong source-line shape (`#short` vs
  `?rev=<full>`); harmless, but planning sketches of tool output should be verified or marked
  unverified.
- Doctor's human display cannot distinguish the git CLI from the 0.27.5 release (both report
  `boltffi 0.27.5` — upstream main hasn't bumped its workspace version). By design (the
  manifest rev agreement is the gate), but doctor would not warn if the release CLI were
  present instead of the git rev. Revisit when the pin decision settles.

## Open questions (for planning — the pin decision is back here)

1. **The path to the C# resume now runs through finding 07.** Options: (a) owner files 07 and
   we wait for the fix on main, then re-pin at that rev (one-literal change on the parked
   branch); (b) wait for the next release carrying #654 + a 07 fix; (c) abandon the git pin,
   return to version pins (the recorded fallback). Recommendation: (a) — the kit entry is
   ready, the M0 machinery is parked, and the MarshalAs verdict is already banked.
2. **Does the bolted-http sequence wait?** No — steps 24+ (S-CONF, S-FFI, S-LX, S-AP, S-AN)
   are C#-independent; only S-WIN.W2 rides the pin, and it was last in the order anyway.
   S-WIN.W1 (the standalone .NET probe, no FFI) is likewise unaffected.
3. Whether to leave `test:web`'s environment fix (chromedriver drift) to routine upkeep or a
   small step of its own.
