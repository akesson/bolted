# Step 12 M5 — ergonomics batch + l10n coverage: what shipped, what converted

## Deliverable 7 — l10n key coverage (the marquee item): **delivered, but not by the doc's mechanism**

**Swift got its first localization coverage test** — the real gap the deliverable names (step-06
friction 7 shipped on Swift, and only Kotlin ever grew a test for it). It is
`apple/profile-app/Tests/ProfileFeatureTests/LocalizationCoverageTests.swift`, and it was watched red
against a planted missing template (`invalid_chars` removed → the tier-1 case fails, "no template for
'invalid_chars'").

It uses **drive-the-core**, not the doc's proposed **generator-emitted declared key list** — because
that mechanism cannot be complete:

- **Rule error keys are runtime strings inside the `#[bolted::rules]` impl bodies.** The generator
  never sees `corporate_email_domain` — it is `ErrorData::new("corporate_email_domain")` inside a Rust
  method, not a parsed attribute. A declared-key list would silently omit every rule key.
- **`required` is a `bolted-core` constant** (`field.rs:167` → `ErrorData::new("required")`), not a
  per-feature declaration.
- **`draft_orphaned` is shell-supplied** — the core reports orphaning as a typed `SubmitError`
  variant, and the shell chooses the key.

So a generator-emitted list would cover value-error and check keys and miss rule/required/shell keys —
strictly *less* than drive-the-core, which sees every key the core actually emits. Step 11 reached the
same conclusion for Kotlin; M5 confirms it and gives Apple the same design. If a declaration-driven
list is still wanted (e.g. to catch a *declared* key no test path exercises), it needs rule error keys
**hoisted to declared attributes** on the rule — an ARCHITECTURE change (it touches how rules name
their errors), and therefore a planning decision, not an M5 task.

## Deliverable 6a — checker lambda helper: **→ upstream filing**

Kotlin cannot SAM-convert BoltFFI's `interface UsernameChecker`, so a shell writes
`object : UsernameChecker { … }` instead of a lambda. The deliverable's own primary path is the
upstream ask (`fun interface`), drafted in M6. The generated-helper *fallback*
(`fun UsernameChecker(block: …)`) needs foreign-language emission `bolted-ffi-gen` does not have —
the same blocker as the codec (M4), the same resolution (step 13 or upstream).

## Deliverable 6c — name-collision refusal: **delivered as a dormant tripwire, with a scope finding**

`bolted-ffi-gen::reject_reserved_type_names` refuses, at generation time, a generated top-level type
whose name lands on a per-language reserved list (`Date`/`URL`/`Data`/`Error` for Swift, `Error`/
`Exception`/… for Kotlin), naming the offended language — no silent rename. Golden test:
`a_generated_type_named_like_a_platform_builtin_is_refused`.

**The honest scope:** every declaration-derived name the generator emits is already suffixed
(`<Entity>Snapshot`, `<Value>ErrorFfi`, `…StashFfi`), so no normal declaration can produce a bare
`Date`/`Error` — the check is a **tripwire for a future generator change**, not a guard that fires on
today's inputs. The live per-feature collision surface — hand-written custom composite types
(`PlainDate`, `AvailabilityRaw`) — is **not named by the generator**, so covering it needs a design
decision (inspect `custom`'s exports, or bring composite naming into the generator). Recorded rather
than half-built.

## Deliverable 6b — Swift `Sendable`: **assessed not needed under the current concurrency mode**

`swift-tools-version:5.9`, non-strict concurrency: `test:apple` compiles clean with no Sendable
warning, and the one place a non-Sendable handle genuinely crosses a queue already has a targeted
hand-written wrapper (`CheckDriver: @unchecked Sendable`, `CheckStateAndConstraintsTests.swift:170`).
A blanket generated `extension …: @unchecked Sendable` per `Send + Sync` class would be speculative
machinery — and it needs the step-13 emitter anyway. Deferred: it should be driven by an actual move
to strict concurrency, not added ahead of one.

## The pattern across 6a/6b and the codec (M4)

Three separate ergonomics/codec items funnel to one root cause: **`bolted-ffi-gen` emits only Rust.**
Every item that needs a *foreign* file (a codec, a Kotlin lambda helper, a Swift Sendable extension)
is blocked on the same missing capability, which step 13 is chartered to build. That is the coherent
finding — not three unrelated shortfalls — and it is exactly the "generator seam" signal KC1 gestures
at. No `dist/` was patched for any of them (KC1's actual trigger is untouched).
