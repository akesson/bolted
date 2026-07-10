# Step 12 M4 — codec deletion: the outcome is a **conversion**, not a deletion

**Deliverable 5 ("delete the hand-written codec") converts** (KC1's mechanism). One conversion —
under the "more than two and the step stops" budget. `StashCodec.kt` is **not** deleted this step.
This is a place the step doc was optimistic in a way an implementer-author would have caught, which
is exactly what the process note asked the report to surface.

## Why

Deliverable 5 offered two branches: BoltFFI stamps `@Parcelize`/`Codable` on its DTOs, or "the codec
becomes a generated file beside the bindings, drift-checked like the rest of D22." M0 killed the
first branch (no annotation passthrough in 0.27.3). The second branch — *generate the codec* — reads
like a modest M4 task in the step doc, but it is not:

- `bolted-ffi-gen` emits **Rust** (`generated.rs`). It has never emitted a foreign language.
- A codec generator is a **type-directed Kotlin emitter**: it must map every wire type (String,
  UInt, ULong, Bool, nested records like `AvailabilityRaw`/`PlainDate`, optionals, the composite
  value object's stash) to JSON (de)serialization Kotlin. That is a parallel type system to the
  Rust one the generator has today.
- It raises unresolved infrastructure: **where generated Kotlin lives** (not in BoltFFI's `dist/` —
  KC1 forbids writing our files into its output), how it is **built** (a new generated source set the
  app depends on), and how it is **drift-checked** across a language boundary the D22 mechanism has
  only ever crossed for Rust.

That is the **foreign-language-emission capability step 13 is chartered to build** ("informed by what
this step learns about emitting foreign-language source at all"). The step doc itself splits
codec-*generation* into M4 and test-*generation* into step 13 — but both sit on the same missing
foundation. Standing that foundation up as a one-off M4 hack would front-run step 13's design and
ship a lower-quality precursor step 13 would redo. An implementation session should not do that
(CLAUDE.md: a structural capability the design reserved elsewhere is a stop-and-record, not a
work-around).

## What M4 did do, and what is already true

- **The load-bearing D27 goal was met in M3, codec-independent.** The schema version rides the
  generated `ProfileStashFfi.schemaVersion`; `store.acceptStash` is the typed parse gate; `restore`
  takes only the accepted token. None of that lives in `StashCodec`. The version authority is
  **already off the hand-written codec** — which was deliverable 5's real point ("the D27 version
  gate is inside whatever replaces it").
- **`StashCodec` is now pure structural marshalling.** M3 removed its `FORMAT_VERSION` gate; it only
  carries `schema_version` through. There is no policy left in it to get wrong — it is the
  "measurement" its own header calls it, and the measurement is now: *~130 lines of type-directed
  JSON that a generator must one day emit.*
- The exit-checklist line "the version gate is typed and observable, with a test distinguishing 'no
  stash' from 'stash refused'" **is satisfied** (M3: `stashWasRefused`, and the app test
  `d27_aStashFromAnUnknownSchemaIsRefusedAndTheVmStartsFresh`). Only "`StashCodec.kt` deleted" is not.

## The two paths that retire it (both out of M4's scope)

1. **Upstream (smallest):** BoltFFI exposes its already-existing DTO wire ser/de as public (or
   opt-in-public). Then the shell base64s `stash.toByteArray()` into `SavedStateHandle` and the codec
   deletes itself with **no generated replacement at all**. Drafted as a step-12 M6 filing.
2. **Step 13:** `bolted-ffi-gen` gains a drift-checked foreign-language codec emitter. This note is
   the scoping input: step 13's doc should own codec-*generation*, not only test-generation, and
   decide the where-it-lives / how-it-builds / how-it-drift-checks questions above.

## Scoping handoff to step 13

Step 13 inherits: (1) the codec emitter above; (2) M0's finding that BoltFFI's wire ser/de is
`internal` (so step 13's per-language contract tests cannot lean on it to round-trip DTOs either —
same blocker, same two paths); (3) the `StashRefusedFfi` / `StashAcceptedFfi` surface D27 added,
which a generated codec and generated tests both consume.
