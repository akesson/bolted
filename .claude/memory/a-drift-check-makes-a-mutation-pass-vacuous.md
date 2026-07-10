---
name: a-drift-check-makes-a-mutation-pass-vacuous
description: "When generated code is committed and drift-checked, every mutation of the generator fails the drift test for free; regenerate inside the harness, assert the output changed, and exclude the drift test"
metadata:
  node_type: memory
  type: feedback
---

If a repo commits generated code and has a test comparing it against the generator's output (a **drift
check**), then a mutation pass over the generator reports **100% caught** — and means nothing. The drift
test fires on every mutation, by construction, for a reason that says nothing about whether the
generated code's *behaviour* is tested anywhere.

**Why:** the drift check is a perfect detector of "the generator changed". Mutation testing asks a
different question: "does anything notice when the *behaviour* changes?" Conflating them turns the
strongest evidence in the repo into a rubber stamp.

**How to apply** — each mutation must:
1. apply the mutation to the generator;
2. **regenerate**, so the committed file matches the mutant;
3. **assert the regenerated output actually differs** from the baseline — if it does not, report the
   mutation as *vacuous*, not as caught (this is [[a-surviving-mutation-is-two-hypotheses]] enforced by
   the harness rather than by memory);
4. run the behavioural suite with the **drift tests excluded**.

Step 10's harness (`docs/steps/artifacts/step-10-mutations.py`) does exactly this. Run naively: 14/14
caught. Run honestly: **8 caught, 6 survived** — and every survivor was a *projection* property nothing
asserted (`any_dirty` pinned `false`, the conflict list reversed, `resolve_take_theirs` keeping mine, a
`Pending` check rendering as `Unchecked` so no spinner ever shows). The suite tested the wrapper's
lifecycle thoroughly and never once asked what the snapshot *said*.

Two supporting notes. A mutant that **hangs** is caught — a deadlock is an observation — but only if the
harness has a timeout. And a mutant caught by a **compile error** is not evidence about the tests;
replace it with one that compiles (a mutation that dropped `complete_check` tripped E0499 instead of a
test, so it was rewritten as "begins the check but never completes it").
