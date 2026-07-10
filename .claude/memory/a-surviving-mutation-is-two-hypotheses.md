---
name: a-surviving-mutation-is-two-hypotheses
description: "A mutation that survives means either the suite is blind or the mutation was vacuous; confirm the mutant differs from the original before you go hunting for a missing test"
metadata:
  node_type: memory
  type: feedback
---

When a mutation survives the test suite, there are **two** hypotheses, and they point in opposite
directions:

1. The suite is blind to a real behaviour. (Write the missing test.)
2. **The mutation changed nothing.** (Write a better mutation.)

Reporting (1) without eliminating (2) is worse than not mutating at all: you go looking for a hole
that does not exist, and you may "fix" it by adding a test that asserts the mutant's behaviour.

Step 09 hit this. A mutation repointed `Checked::check_pins` at `fields.first()` — and `Profile`'s
first field **is** `username`, the checked one. The mutant was identical to the original. It
"survived" the way a tautology passes. Repointed at `fields.last()`, three tests caught it at once.

**How to apply:** before treating a survivor as a finding, prove the mutant is observably different
from the original — read the mutated code against the specific input the tests use, or assert the
difference directly. Only then ask why nothing noticed. Prefer mutations whose effect does not depend
on incidental facts about the fixture (field order, name collisions, a constant that happens to be
right).

The genuine survivor in the same pass was `commit_gates` reordered to check conflicts before orphaned:
it passed 22 invariants on four features because every `c07_*` assertion built a draft failing exactly
**one** gate. That is the real disease — see [[a-suite-with-one-implementor-is-shaped-like-it]] and
[[a-missing-prop-assume-asserts-the-bug]]: a suite is silent about the states it never constructs.
