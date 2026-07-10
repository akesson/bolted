---
name: test-android-exit-code-masks-failures
description: mise run test:android exits 0 even when tests fail — trust only the JUnit XML, never the exit code / background-task "exit code 0"
metadata:
  type: project
---

`mise run test:android` (the GMD/emulator `connectedCheck` tier, device `dev34`) **returns exit code
0 even when a test fails**. Proven on 2026-07-10 during step 13's per-language planted-red: a
deliberately-broken assertion produced `tests="80" failures="1"` in the JUnit XML while the task
exited **0**.

The one true source of pass/fail and counts is the combined JUnit XML:
`android/profile-probe/build/outputs/androidTest-results/managedDevice/debug/dev34/TEST-*.xml`
(a single `<testsuite …>` whose `name` is just the first class alphabetically, e.g. `CallbackProbe`,
but which wraps every class's cases; read `failures="N"` and the `<testcase><failure>` children).
Run with `--rerun-tasks` before quoting any number.

**Why:** the exit code is a false green — and it is more dangerous now that tiers run as background
tasks whose completion notification quotes exactly that exit code ("completed (exit code 0)"). Reading
the notification as "passed" records a failure as a success. The step docs already say "counts from
the JUnit XML"; what they don't say, and this does, is that the exit code actively *lies*.

**How to apply:** after any `test:android*` run — foreground or background — parse the XML, never the
exit code. Grep for `failures="[1-9]` across the `TEST-*.xml` files; a greedy regex over `<testcase>`
blocks misattributes which case failed (it spans boundaries), so match `<testcase …>` line-adjacent to
`<failure` instead. Related: [[a-forbidding-test-can-forbid-nothing]] (why the planted-red that exposed
this exists), [[device-and-ui-tiers-are-proven-on-this-machine]] (the physical-device tier, a different
concern).
