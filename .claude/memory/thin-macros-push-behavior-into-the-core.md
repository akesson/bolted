---
name: thin-macros-push-behavior-into-the-core
description: "'Macros only stamp names' is not a style rule — it is the forcing function that moves judgements down to rung 1; and a uniform macro is not automatically a cheap one"
metadata:
  node_type: memory
  type: project
---

ARCHITECTURE §5 says *generics carry behavior, macros only stamp names*. Until step 09 that read like
taste. Writing `bolted-macros` showed what it actually does: **three judgements that were about to be
emitted per feature had to move into `bolted-core` instead.**

| Would-be generated | Now, rung 1 | The judgement |
|---|---|---|
| `match field.validity() { .., Unset => "required" }` | `Field::required_error` | D13: is an empty field an error? |
| `if orphaned {..} if conflicted {..} if !report.is_ok() {..}` | `commit_gates` | C07: when is a commit refused, and why |
| `match check.state() { Pending, Done(Err), Idle if dirty }` | `SingleFlight::violation` | C13 + C16: when does a verdict block |

The trap is that emitting them would have been **correct**. Every conformance test would pass. The
problem is that the correctness would live where no reviewer reads and no type-checker constrains.
`bolted-macros/src/golden.rs::the_emitted_code_makes_no_judgement_of_its_own` now fails the build if
emitted code mentions `Validity::`, `CheckState::`, `CommitError::Conflicted/Orphaned` or `is_ok()`.

**The second half, learned the hard way:** *uniformity is not free.* The first `#[bolted::entity]`
routed every `try_set_*` through the C13 verdict guard — which clones each checked field's value and
compares it afterwards. Generated `try_set_name` therefore cloned the `Username` on every keystroke of
the *name* box; hand-written `spike-profile` did not. **No conformance test could see it: the
behaviour was identical, only the work differed.** It landed on precisely the path the framework's
central bet ("the core validates every keystroke") and step 07's kill criterion 4 live on.

**How to apply:** when a macro emits the same line for every field, ask what the hand-written reference
did *per field* and why. A conformance suite compares observable behaviour; it is structurally blind to
cost. Compare emitted code against the reference for **work**, not only for answers — and write the
report from the emitted code, not from memory. Both false claims caught in steps 08 and 09 were caught
that way. Related: [[a-surviving-mutation-is-two-hypotheses]], [[the-core-ships-no-lock]].
