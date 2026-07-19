# C# IR backend collapses same-named `#[ffi_stream]` methods across exported classes — the later receiver's stream is silently lost

**Reported against:** boltffi git main `23cf2ecce20327581a0d03b41aee6af9cd081ea3` (2026-07-18,
post-#654 "Migrate C# to the new IR backend") · **Severity:** high — a *silent
wrong-stream subscription*, not a compile error · **Regression:** streams on two classes
worked at 0.27.5 (old backend); the collapse arrives with the IR migration. **NOT FILED —
owner files (hard rule).**

## Summary

When two exported classes each declare an `#[ffi_stream]` method with the **same unqualified
name** (here: `snapshots` on both `ProfileStoreFfi` and `ProfileDraftFfi`), the C# IR backend
deduplicates the stream bindings by method name. Only one `NativeMethods` binding family is
emitted (the store's), and **both** generated `Snapshots()` extension overloads route to that
single stream runtime. Calling `draft.Snapshots()` therefore subscribes to the *store's*
canonical stream while passing a *draft* handle — draft mutations never deliver, and the
subscriber waits forever. A uniquely-named sibling (`snapshots_small`) survives intact.

The C side is correct; only the C# render layer loses the method:

| Layer | `…profile_draft_ffi_snapshots_subscribe` present? |
|---|---|
| `boltffi.h` (generated C header) | **yes** (alongside `…profile_store_ffi_snapshots_subscribe`) |
| dylib exports (`nm -gU`) | **yes** |
| generated C# `NativeMethods` | **no** — only `profile_store_ffi_snapshots_*` and `profile_draft_ffi_snapshots_small_*` |

## Observed failure (this repo)

`mise run test:csharp` at the pinned rev: TRX `total=14 passed=11 failed=3` — the intentional
step-14 tripwire (now red because #654 *fixed* the MarshalAs bug, separate finding 06) plus
**two StreamProbe tests** that time out at their 10 s cancellation token with `Expected "zoe"
… But was null`: the draft-stream subscription is bound to the wrong stream, so the pushed
snapshots never arrive.

Cross-controls: the **same rev** drives the Swift bindings' stream tests green
(`test:apple` exit 0), so the defect is C#-binding-only; and the same probes were green on the
C# backend at 0.27.5 (step-14 StreamProbe) — a genuine regression, not a pre-existing gap.

## Suspected mechanism

The IR backend appears to key stream-runtime/binding emission by unqualified method name
rather than by `(receiver class, method)`. First receiver wins; later same-named streams are
dropped, and their public extension methods are silently re-routed to the surviving binding.
Same render family as the finding-06 fix: `boltffi_backend/src/target/csharp/render/`
(likely `stream.rs`).

## Minimal repro sketch (for the filing)

Two exported classes, each with an identically-named stream:

```rust
#[export]
impl Store { #[ffi_stream] fn snapshots(&self) -> /* stream of */ Snapshot { … } }

#[export]
impl Draft { #[ffi_stream] fn snapshots(&self) -> /* stream of */ Snapshot { … } }
```

Generate C# and inspect `NativeMethods`: only one `*_snapshots_subscribe` binding is present;
both public `Snapshots()` methods compile and route to it. Subscribing on the second class
yields the first class's stream (or nothing), with no diagnostic. Kotlin/Swift backends emit
both bindings from the same declaration (verified here via the green Apple tier); a fix should
key stream emission by receiver + method, or fail generation loudly on the collision.

## Evidence in this repo

- Step-23 M1 run at rev `23cf2ec` (2026-07-19): TRX counts above; `boltffi.h` line ~176;
  `nm -gU` export list; generated `dist/csharp` `NativeMethods` inventory.
- Contrast surfaces: `crates/gen-profile-ffi` declares both `snapshots` methods and
  `snapshots_small`; only the draft's `snapshots` is lost.
