---
name: bolt-driver-sibling-repo
description: "bolt-driver (~/Developer/akesson/bolt-driver) exists and its spike is proven — bolted's obligations live in docs/design/bolt-driver-seam.md; bolted must never depend on it"
metadata: 
  node_type: memory
  type: project
  originSessionId: 03b4a9ec-39dc-4865-a0ab-2b5182ae9330
  modified: 2026-07-23T13:56:44.550Z
---

**bolt-driver** — in-process, permission-free app driving/inspection (semantics tree + tap/
set_text + screenshots over a local socket; Claude is the primary client) — lives at
`~/Developer/akesson/bolt-driver` as its own repo (decided 2026-07-23; full implementation,
spike, docs all there). Its step 01 proved both arms (Compose semantics owner; SwiftUI via
self-enabled `ApplicationAccessibilityEnabled`) driven blind end-to-end, KC1/KC2 not hit.

**Why:** bolted sessions must know the seam exists so no decision forecloses it, and must not
re-plan driving/replay tooling that already lives there.

**How to apply:** anything bolted owes the seam (generated automation ids, the FFI tap,
`bolted-trace`, a dev-mode agent host, the `DriveTarget` mapping) is recorded in
`docs/design/bolt-driver-seam.md` — design input, unscheduled; never resolve it ad hoc. The
dependency law: bolted never depends on bolt-driver; the one glue crate (`driver-bolted`)
lives in the bolt-driver repo. Session-replay's two-track model is documented there in
`docs/design/session-replay.md`; the core track (`bolted-trace`) stays a bolted feature
([[the-core-ships-no-lock]] and D35 are why a bolted core passes its replayability contract by
construction).
