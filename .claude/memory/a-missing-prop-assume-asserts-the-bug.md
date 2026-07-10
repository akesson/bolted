---
name: a-missing-prop-assume-asserts-the-bug
description: "A proptest `assume` set that omits a precondition does not weaken the property — it silently asserts the bug, and the generator will never sample the counterexample"
metadata:
  node_type: memory
  type: feedback
---

A property test whose `prop_assume!` set is missing a precondition **does not test a weaker
property. It asserts the bug.** And because the generator draws inputs independently, it will very
likely never sample the case that would fail.

Bolted's C03 read: *a dirty field whose value differs from `theirs` conflicts.* Its proptest assumed
`mine != base` and `theirs != mine` — but never `theirs != base`. Two independently-drawn 3–20
character lowercase strings are essentially never equal, so `theirs == base` was **never generated in
six steps**, and the suite spent that whole time asserting that a field the server never touched must
conflict. Two example-based tests (`c08_rebase_reruns_tier2_rule`, and the web shell's echo-rule test)
were *producing* the spurious conflict the whole time and passing, because neither asserted on it.

**Why:** a missing `assume` is invisible in review — the property still reads true, because a human
supplies the omitted precondition unconsciously while reading. Only the generator is honest, and only
if the counterexample is reachable in its distribution.

**How to apply:** when writing or reviewing a `proptest!`, enumerate the *state space* the property
ranges over, not the sentence. For any three-way comparison (`base`/`mine`/`theirs`), ask what each
of the pairwise equalities means and whether the property still holds. If a case is meant to be
excluded, exclude it with an `assume` **and write the sibling test that covers it** — C19 exists
because C03's excluded case was the interesting one. See `docs/steps/step-07-report.md`.
