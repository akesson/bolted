---
name: a-forbidding-test-can-forbid-nothing
description: "A test that asserts a needle is ABSENT passes trivially when the needle can never match; pin it from both sides with a positive control that proves each needle can fire"
metadata:
  node_type: memory
  type: feedback
---

An assertion of the form *"the emitted code must not contain X"* is green in two very different worlds:
the code is clean, or **X can never match anything**. Nothing distinguishes them, and the second world
is the default when the needle is a string.

Step 10 shipped this bug. `bolted-macros`'s golden test forbade `"Validity ::"` — correct, because it
searched a `TokenStream::to_string()`, and `quote` prints paths with spaces around `::`. I copied the
needles into `bolted-ffi-gen`, which formats with `prettyplease`, which prints paths **tight**
(`Validity::`). Every needle silently matched nothing. The test was green, and asserting *nothing*, for
as long as it existed.

**Why:** a negative assertion has no witness. A passing positive test at least proves its code ran.

**How to apply:** for every "must not contain" test, write its positive control — construct a fragment
that *is* guilty, push it through **the same pipeline** (the same formatter, the same serializer), and
assert each needle matches. `the_forbidden_needles_can_actually_fire` in `bolted-ffi-gen/src/golden.rs`.
Then, once, plant the forbidden thing in the real generator and watch the real test fail; a control that
tests the needle is not a control that tests the wiring. (My first attempt at the control was itself
wrong — I pointed it at a file that happened not to contain the needles either.)

The generalisation: **a vacuous check and a passing check look identical from the outside.** Same
disease as [[a-surviving-mutation-is-two-hypotheses]] and [[a-drift-check-makes-a-mutation-pass-vacuous]],
and the same cure — before believing a green result, prove the mechanism can go red.
