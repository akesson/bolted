---
name: ide-diagnostics-lie-after-subagent-work
description: Post-sub-agent IDE diagnostics are stale mid-edit snapshots; only the gates re-run on the branch are evidence
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ddcc2f3b-af09-4980-882e-723913127f3b
  modified: 2026-07-24T07:07:09.738Z
---

On four of six step-27 milestones (M0, M1, M2, M3), IDE diagnostics (rust-analyzer /
SourceKit) reported hard errors — unresolved imports, missing modules, "no such module" —
immediately after a sub-agent finished, while the agent claimed green. All four times the
diagnostics were stale: mid-edit snapshots, or (SourceKit) indexes against **old gitignored
generated bindings** that the next pack regenerates. The decisive M3 case: eleven SourceKit
errors against Swift files that compiled and passed the full tier minutes later.

**Why:** the diagnostic index lags the working tree, and for FFI shells it indexes committed
or previously-generated sources, not the freshly-packed dist. A sub-agent's report can also
be wrong, in the other direction — so neither source alone is evidence.

**How to apply:** after any sub-agent hands back work, ignore both the diagnostics and the
report's green claims; verify on the branch: `git status` clean, grep for claimed
deletions/wirings, then re-run the real gates (`mise run check` / `mise run test` / the
platform tier). If a gate compiles the code the diagnostics called broken, the diagnostics
were stale — don't spend time "fixing" them. Related: [[test-android-exit-code-masks-failures]],
[[subagents-stall-awaiting-own-background-runs]].
