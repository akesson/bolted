# A throwing / `Result`-returning `#[export]` method cannot return a class handle

**Reported against:** boltffi 0.27.3 · **Severity:** medium · **Disposition at 0.27.5: NOT
REPRODUCIBLE — DO NOT FILE without first reconstructing the original failure.**

## Original claim (step 12 M3, 0.27.3)

The natural D27 shape `restore(stash) -> Result<Draft, StashRefused>` was reported not to compile:
`Result<Handle, E>` was said to require `WireEncode`, which a class handle does not implement —

```
the trait bound `DraftFfi: WireEncode` is not satisfied
required for `Result<DraftFfi, StashRefusedFfi>` to implement `WireEncode`
required by a bound in `FfiBuf::wire_encode`
```

This drove the D27 two-step workaround (`accept_stash → token → infallible restore`).

## Re-verification at 0.27.5 (step 15 M4) — could NOT reproduce, at 0.27.3 OR 0.27.5

The reported compile failure does **not** reproduce. Four faithful controls of the exact shape
(`&self` method returning `Result<class handle, error>`) all **compile cleanly at both boltffi 0.27.3
and 0.27.5** (Cargo.lock version verified each time):

1. A minimal standalone crate (`repro-05/`, included here): `Store::try_make(&self) -> Result<Draft, MakeError>`.
2. The real `spike-profile-ffi`: `ProfileStoreFfi::try_restore(&self, ..) -> Result<ProfileDraftFfi, ErrorData>`.
3. The same, with a real *throwing-error* enum: `-> Result<ProfileDraftFfi, SubmitErrorFfi>`.
4. Control 3 under `--cfg boltffi_binding_expansion` + the full pack-android expansion env (the CLI
   codegen path, not just `rustc`).

All four printed `Finished` with no `WireEncode`/E0277 error. The `FfiBuf::wire_encode` bound the
original error cites is not emitted at type-check time in either version.

## Recommendation

**Do not file.** Either the original step-12 M3 diagnosis was imprecise (the real blocker may have
been a narrower signature — a generic, or the specific `StashRefused` type as it existed pre-D27) or
it was already resolved at/before 0.27.3. Upstream 0.27.5 **#647** ("lower `Result<Class, E>` returns
as object handle, not wire-encoded record") is topically adjacent and may well have addressed this
class of problem — but there is **no red control** here proving the reproducible shape was ever broken,
so filing (or claiming "#647 fixed it") would risk a non-reproducing report.

The D27 token workaround stays **by choice** — it is the stronger parse-don't-validate shape (the token
proves the gate ran), per the step-12 rationale — not because a live toolchain limitation forces it.

## Reproduction of the (negative) result

`repro-05/` in this folder. `cargo build` compiles; flip the `boltffi` pin between `"0.27.5"` and
`"=0.27.3"` and it still compiles. See `repro-05/README.md`.
