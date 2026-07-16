---
name: pr-based-workflow
description: Since 2026-07-16 this repo uses a PR-based workflow — work on a branch, open a PR; never push or commit directly to main
metadata:
  type: feedback
---

Henrik (2026-07-16, mid-step-18): "we will start using a PR-based workflow."

**Why:** review boundary — implementation work should land on `main` only through a reviewed PR.

**How to apply:** at the start of any session that will commit, create/reuse a step branch (e.g.
`step-18-os-topology-probe`), commit milestones there, push the branch, and open a PR with `gh pr
create` when the step completes. Do not push to `main` directly (the permission layer also blocks
it). Committed planning docs made on main before 2026-07-16 predate the rule. Step 18's PR:
https://github.com/akesson/bolted/pull/5.
