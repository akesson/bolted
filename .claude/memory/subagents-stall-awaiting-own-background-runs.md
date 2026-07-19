---
name: subagents-stall-awaiting-own-background-runs
description: "Opus sub-agents end their turn 'waiting for a notification' from their own background run — require synchronous foreground runs in the prompt; a warning alone does not work"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: ddcc2f3b-af09-4980-882e-723913127f3b
  modified: 2026-07-19T23:15:56.968Z
---

Three Opus sub-agents (step 25 M4; step 26 M0 and M1) ended their turn mid-milestone saying
they would "wait for the notification" from a background emulator/test run they had launched.
No such notification ever reaches a sub-agent — the run finishes and the agent sits completed
with work unrecorded and sometimes a mutation still applied.

**Why:** the sub-agent harness re-invokes on background-task completion for the *main* session,
not inside a sub-agent's own loop; agents pattern-match the main-session workflow.

**How to apply:** in every sub-agent prompt that will run long emulator/CI-style invocations,
do not merely warn — **remove the option**: require the run be executed synchronously in the
foreground (`run_in_background:false`, generous timeout, output captured to a log file), and
state that if a foreground call times out mid-run the agent must immediately issue the next
tool call to check the log rather than end its turn. A prominent warning alone failed (step 26
M1 stalled anyway); the synchronous requirement held for M2–M4 (three stall-free milestones,
including a 68-minute mutation pass). If an agent still stalls: resume THAT agent (SendMessage
to the same id — context intact), never spawn a fresh replacement; instruct it to check the
finished run directly, record only observed results, and revert any in-flight mutation.

Related: [[test-android-exit-code-masks-failures]] (the results the agent must read are the
JUnit XML, which is also what makes "check the run yourself" reliable).
