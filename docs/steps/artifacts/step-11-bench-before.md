# Step 11 — hardware benchmark, the "before" half

The baseline `bench:android:device` run against the **hand-written** `spike-profile-ffi`, captured
while `pack:android` still built it. Step 11's M5 repoints `pack:android` at `gen-profile-ffi`;
after that, reproducing these numbers requires checking out the commit below. The "after" run on
generated bindings goes in `step-11-report.md`, compared against this table.

- **Date**: 2026-07-10
- **Commit**: `55b7faf` (step-10 completion; only docs uncommitted on top)
- **Bindings**: hand-written `spike-profile-ffi`, freshly packed by this run (`boltffi pack android
  --release`, boltffi 0.27.3)
- **Device**: Google Pixel 8a (API 36), physical, attached over USB (serial `4B091JEKB25623` — not
  `emulator-*`; the verb's gate and the in-test `Build.FINGERPRINT` guard both passed honestly)
- **Suite**: `dev.bolted.profileapp.PhysicalChattinessProbe` — 4 tests, 0 failures, n=2000 per
  measurement

| Measurement | p50 | p95 |
|---|---|---|
| `HW.KEYSTROKE` (try_set + snapshot round-trip) | **0.0363 ms** | 0.0466 ms |
| `HW.try_set_username` (half 1) | 0.0070 ms | 0.0093 ms |
| `HW.snapshot_readback` (half 2) | 0.0175 ms | 0.0238 ms |
| `HW.noop.kotlin` (System.nanoTime baseline) | 0.0005 ms | 0.0006 ms |
| `HW.keystroke.cold_first` (first call, one-shot) | 0.7910 ms | — |

Read against the bars:

- **KC5's kill bar is 1.0 ms per keystroke.** The hand-written layer sits at 0.036 ms p50 — a
  ~27× margin. Step 05's emulator figure is now retired as the reference; this is the number the
  generated bindings must not regress past.
- The two halves sum to ~0.025 ms against a round-trip p50 of 0.036 ms; the remainder is harness
  overhead, consistent with the 0.0005 ms noop baseline not being the bottleneck.
- Even the **cold first keystroke** (0.79 ms) is under the kill bar, though it is a one-shot
  number and should be read as an order of magnitude, not a statistic.

Interpretation for step 11: the migration has ~27× headroom before KC5 trips. A regression that
matters would be a factor-of-ten event — visible, not marginal.
