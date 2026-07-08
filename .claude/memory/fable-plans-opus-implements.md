---
name: fable-plans-opus-implements
description: "Henrik splits work by model tier — Fable does architecture/planning, fresh Opus sessions implement steps from repo docs"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 7740d6ed-53e9-428a-9a10-15e80340e990
---

In the bolted repo, Henrik hands implementation to fresh Opus sessions while Fable handles
architecture, planning, and design decisions. The handoff medium is committed markdown:
CLAUDE.md (read order + rules), docs/ROADMAP.md (step status), docs/steps/step-XX-*.md
(detailed specs), and step-XX-report.md files flowing findings back to planning.

**Why:** docs must be self-sufficient — the implementer has no access to the planning
conversation.

**How to apply:** as Fable, author step docs with concrete signatures, exit checklists, and
kill criteria; read step reports before planning the next step; keep ARCHITECTURE.md's OPEN
questions (§9) as the list of things implementers must not resolve ad hoc. Update ROADMAP
status when authoring or freezing steps.
