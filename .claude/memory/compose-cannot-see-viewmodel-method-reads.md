---
name: compose-cannot-see-viewmodel-method-reads
description: "A Compose shell must take core state as a parameter — `vm.conflict(field)` reading a StateFlow is invisible to Compose, and strong skipping then skips the row forever"
metadata:
  node_type: memory
  type: project
---

In a Compose shell, **never read core state by calling a method on the ViewModel**. Take it as a
parameter, or read it through `collectAsStateWithLifecycle`.

Compose only observes `State` reads that happen *during composition*. `vm.conflict(field)` reaches
into a `StateFlow`, which is not a `State` — Compose sees nothing. And **strong skipping** (on by
default since the Compose compiler moved into Kotlin 2.x) makes a composable skippable when its
parameters compare equal, comparing unstable params by *instance*. `vm` is the same instance forever,
so the row is skipped and never re-reads anything.

Symptom in step 07: the core conflicted, the ViewModel knew, and the conflict banner never appeared.
Two Compose UI tests timed out waiting for a node that could not exist. Everything else in the row
kept working, because typing changes the `value` parameter and forces a recompose — so only the
*rebase-driven* state (conflicts, dirty-when-the-buffer-did-not-change) was silently dead.

**Why:** the sibling shells cannot teach you this. Swift's `@Observable` tracks property reads at
runtime; Leptos has signals. Compose is the only one of Bolted's three shells where a plain method
call is invisible to the reactivity system.

**How to apply:** thread the snapshot through as a parameter (`TextFieldRow(vm, field, …, snapshot,
onChange)`), and give the VM's readers an explicit `snap: ProfileSnapshot` argument. It is also why
step 07 fought for a **headless Compose UI tier**: this class of bug is invisible to unit tests and
only a real render tree catches it. Same reflex as [[bolted-verify-in-a-real-browser]] and
[[art-gc-probes-need-a-control]]: the code was plausible, reviewed, and wrong until a test ran in the
real environment.
