---
name: device-and-ui-tiers-are-proven-on-this-machine
description: bench:android:device and test:apple:ui both ran green on this Mac (2026-07-10) — don't re-diagnose the environment; the USB cable is CONFIRMED dead (2026-07-23), replace it before any device run
metadata:
  type: project
---

Both previously-unrun verification tiers were proven working on this machine on 2026-07-10, at
`332fe58` (hand-written bindings):

- **`mise run bench:android:device`** — physical Pixel 8a (API 36), USB serial `4B091JEKB25623`,
  already authorized (trust persists). Baseline numbers in
  `docs/steps/artifacts/step-11-bench-before.md`.
- **`mise run test:apple:ui`** — 9/9 green in ~90 s. The terminal **already holds Accessibility
  permission**; the "Timed out while enabling automation mode" failure mode does not apply here.

**Why:** step 11 owes the "after" halves of both runs. Diagnosing the environment from scratch cost
a whole session once; it is known-good now.

**Update 2026-07-23:** the cable prediction below came true — during bolt-driver's step-01 M1
the Pixel 8a was entirely absent from `ioreg` (not just adb) and an emulator fallback was used.
**The cable is confirmed dead; replace it before the next device-tier run.** Wireless debugging
remains the interim path.

**How to apply:** if `adb devices` shows nothing, the cause is almost certainly **the USB cable**
— a charge-only/failing cable made the Pixel vanish from `ioreg` entirely (below adb; no software
fix helps). Swap the cable and verify with `ioreg -rc IOUSBHostDevice` *before* touching adb.
Wireless debugging also works as a fallback (phone reaches this LAN; pairing dialogs expire in
~1 min, so pair immediately). A wireless serial (`192.168.x.x:port`) passes the verb's
anti-emulator gate honestly.
