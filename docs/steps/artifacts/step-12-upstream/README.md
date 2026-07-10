# Step 12 — upstream filing drafts (BoltFFI 0.27.3)

**These are drafts. Filing them is the owner's action, not the session's** (step-12 doc, deliverable
9). Each is written as a fileable report: repro, expected vs. actual, workaround, acceptance test.

The step doc asked for three (01–03). Steps M0 and M3 surfaced two more (04, 05) — both are real
BoltFFI limitations that shaped this step's design, so they are drafted here too.

| # | Title | Found | Blocks |
|---|---|---|---|
| 01 | `pack android` omits the binding-expansion env → undefined symbols | step 05 | a green `pack:android` with no workaround |
| 02 | Generated methods never consult `__boltffi_closed` → use-after-close UB | step 05 (H2) | making use-after-close a typed error, not UB |
| 03 | bindgen silently ignores macro-generated FFI items | step 10 | `#[bolted::*]` macros that emit `#[data]`/`#[export]` |
| 04 | DTO wire ser/de (`toByteArray`/`fromByteArray`) is `internal` | step 12 M0 | retiring per-feature persistence codecs (deliverable 5) |
| 05 | A throwing/`Result` method cannot return a class handle | step 12 M3 | an atomically-fallible `restore` (D27) |

02's adjacent asks (`fun interface` for Kotlin SAM conversion, an opt-in `Cleaner`) are folded into
02 rather than split, because they share the same "the generated handle class is bindgen output we
cannot reach around" root.
