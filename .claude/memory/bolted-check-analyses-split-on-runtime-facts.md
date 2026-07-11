---
name: bolted-check-analyses-split-on-runtime-facts
description: A bolted-check analysis needing runtime facts (composites, schema version) can't be a pure source-scan bin — the generator lives in the -ffi crate; and constraint bounds are invisible to the FFI drift layer
metadata:
  type: project
---

Step 16's `bolted-check` renders a **constraint-surface snapshot** — the third emitter over the one
parser (D25), a pure function of `bolted_decl::Feature` (deps: `bolted-decl` + `bolted-core` only).
But two facts it must show exist **only when the feature crate is linked**, never in the source text:
a composite value object's constraints (`DateRange::Custom("start_le_end")`, invisible to a source
scan by D20 — `Feature::value()` returns `None` for it) and `STASH_SCHEMA_VERSION` (in the generated
module). Kill criterion 2 forbids `bolted-check` itself from linking/executing features.

So the split that shipped: the **pure renderer** stays in `bolted-check`; the **generator** that
writes the committed `crates/gen-*/constraints.snap` lives in each `-ffi` crate as a cargo
**example** (`gen-<feature>-constraints`, on **dev-deps** so `bolted-check`/`syn` stay out of the
shipped cdylib and `boltffi pack` never builds it). The example builds a `RuntimeSurface` from the
linked `FieldId::constraints()` + `STASH_SCHEMA_VERSION` and hands it to the renderer; a
`tests/constraint_snapshot.rs` rebuilds the same surface and byte-compares (D28: nothing formats a
`.snap`). This deviates from the step doc's "gen-constraints bin in bolted-check reads src/lib.rs" —
that letter is inconsistent with M2/M3 (composites + version), and the more-fundamental kill
criteria win. `render_constraint_snapshot` also gained a `feature_name: &str` arg (the declaration
knows the entity name `Note`, not the crate name `gen-note`).

**The crux that makes the whole analysis necessary:** a constraint bound (`max = 30`) **never reaches
`gen-*-ffi/src/generated.rs`** (grep count 0) — it lives only in the feature's macro output +
runtime `constraints()`. So the D22/D28 FFI drift checks are structurally **blind** to a constraint
*tightening*; the snapshot catches it precisely because it reads the *runtime* `constraints()`.
Proven in M4: `PersonName max 30→29` fails the snapshot check at the exact line while the FFI drift
check stays green (4/4).

**Lesson for the next bolted-check analysis:** first ask whether it is a pure source function or needs
runtime facts. A pure one (a source-only lint) could be a real `bolted-check` bin; one that needs
runtime facts is forced into the per-feature `-ffi`-example shape. Don't let the example precedent
drag a pure analysis into it. The WASM size budget is a *different* axis again — it needs
`trunk`/wasm32 and cannot live at rung 3 inside host-only `mise run check`, so it wants its own tier.
See [[thin-macros-push-behavior-into-the-core]], [[a-drift-check-makes-a-mutation-pass-vacuous]],
[[the-core-ships-no-lock]].
