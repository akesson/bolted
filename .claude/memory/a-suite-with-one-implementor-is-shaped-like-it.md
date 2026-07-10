---
name: a-suite-with-one-implementor-is-shaped-like-it
description: "A 'generic' test suite with one implementor silently grows that implementor's shape; write a second, deliberately opposite one — then mutate both"
metadata:
  node_type: memory
  type: feedback
---

A test suite that is **generic over a trait with exactly one implementor** is not generic. It is the
concrete suite with extra ceremony, and the trait grew whatever shape that implementor happened to
have. Reading it will not reveal this: the code type-checks, the names sound abstract, every test
passes.

Two moves catch it, and only both together:

1. **Write a second implementor that is deliberately the opposite** in every optional dimension. In
   step 08, `spike-note` has two plain text fields where `spike-profile` has a composite value object,
   a tier-2 rule and an async check. What the second one *cannot* implement is what the suite had no
   business demanding.
2. **Mutate both implementors.** The second fixture passing on the first try tells you nothing — it
   might mean the suite is generic, or that it asserts nothing.

The second step is what paid. Mutating `StoreDraft::is_based` to consult a *single* field passed all
21 other invariants **on both features**, because every draft the suite built had an ancestor in all
of its fields or in none. A draft misjudged create-flow is never rebased, never orphaned, and silently
overwrites the server on submit — and step 09's macro will *generate* `is_based`.

**Why:** a suite's blind spots live in the states it never constructs, and a passing suite is silent
about exactly those. The same disease as [[a-missing-prop-assume-asserts-the-bug]]: the generator
never samples the counterexample, so the property asserts the bug.

**How to apply:** when extracting a suite to be generic, budget for a second fixture *and* a mutation
pass over both. When a mutation survives, the fix is a new test, not a note — see C12's second
sentence in `docs/CONFORMANCE.md` and `docs/steps/step-08-report.md`.
