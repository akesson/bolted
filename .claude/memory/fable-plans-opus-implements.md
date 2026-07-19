---
name: fable-plans-opus-implements
description: "Henrik's bolted workflow — Fable plans AND drives execution, delegating implementation to Opus sub-agents (since 2026-07-19; before: separate fresh Opus sessions)"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 7740d6ed-53e9-428a-9a10-15e80340e990
  modified: 2026-07-19T07:45:09.550Z
---

In the bolted repo, Fable handles architecture, planning, and design decisions, **and drives
step execution in the same session by delegating implementation to Opus sub-agents** (Agent
tool with `model: "opus"`). Changed by Henrik 2026-07-19; before that, implementation ran in
separate fresh Opus sessions with committed markdown as the only handoff.

The file interface survives the change: CLAUDE.md (read order + rules), docs/ROADMAP.md (step
status), docs/steps/step-XX-*.md (detailed specs), and step-XX-report.md flowing findings back
to planning.

**Why:** step docs stay self-sufficient — a sub-agent has no access to the planning
conversation, same constraint as a fresh session had. Fable in the loop means deviations and
structural questions get caught between slices instead of at report time.

**How to apply:** as Fable, author the step doc first (concrete signatures, exit checklists,
kill criteria), then execute it by spawning Opus sub-agents per coherent slice; review each
slice's result against the step doc; write the report yourself. Sub-agents inherit the
implementation rules (smallest reversible choice on omissions; stop on structural questions;
never resolve ARCHITECTURE §9 ad hoc; kill criteria are real). Update ROADMAP status when
authoring and when done.
