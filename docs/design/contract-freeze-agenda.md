# bolted-http contract freeze — session agenda

**Compiled 2026-07-21** from the step-24/25/26 report §Open-questions lists (deduplicated;
step 26's numbering kept where they overlap) plus items that arrived after step 26. All
reachable implementors are in (mock, Linux/reqwest, Apple/URLSession, Android/OkHttp);
BoltFFI is at 0.28.0. Pointers only — the cited docs carry the evidence.

## Contract questions (the freeze proper)

1. **The streaming seam** — proposal ready: [streaming-seam.md](streaming-seam.md)
   (chunk re-entry, back-pressure/overflow, end-of-body terminal, subscription
   lifecycle; three-platform evidence; post-0.28.0 upstream state).
2. **Redirect ceiling as CFG** at the composition root — also removes the classifier's
   one unavoidable exception-text match (OkHttp) and the honest-limit gap (all
   platforms). (24 #Q2 / 25 #2 / 26 #2.)
3. **`content_length` semantics per sink kind** — the honesty split (gzip strips it;
   file sink can count) holds on all three platforms. (25 #3 / 26 #3.)
4. **Push-cancellation seam** on the capability trait — three platforms now pay the
   poll-watcher thread; note the shared shape with streaming back-pressure
   (streaming-seam §3b: both are core→adapter mid-flight signals). (25 #4 / 26 #4.)
5. **`PermissionDenied` reachability** — two platforms gated identically
   (`AdapterOnly`, cause-mapping unit-proven); decide whether the contract states it as
   inherently device/app-bundle-tier. (24 #4 / 25 #5 / 26 #5.)
6. **`HttpError → Into<ErrorData>` bridge** (v1.14 residue). (24 #1 / 25 #6 / 26 #6.)
7. **Adapter packaging conventions → `bolted new` scaffolding rules** — SwiftPM and
   Gradle shapes both proven; plus the FFI-bridge-crate drift check (F-M0-1: bindgen
   reads source text, so bridge crates are per-target copies). (25 #7 / 26 #7.)
8. **Conformance-scope boundary** — which invariants are shared-suite obligations vs
   per-adapter unit obligations (F-M4-2 pin ordering); should row 11 assert upload
   `total` (F-M4-3, unasserted on every implementor)? (26 #8.)
9. **The cookie capability's seam obligation** (row 26 stays open, but the shape does
   not): the per-hop consultation is the same mid-flight adapter→core re-entry as the
   streaming seam — define the shape once (streaming-seam §4, feature-matrix §5.20),
   even if the capability itself stays deferred.

## Smaller decisions (fold in if time permits)

- C3 divergence matrix: grow a CFG column for row-25 proxy/env divergence, or keep the
  docs as the record? (24 #5.)
- `SkipReason`: still-unused harness API — keep for platform-only rows or remove? (24 #6.)
- The web-evaluation trio recorded during the web assessment: `HttpVersion::Unknown`,
  CAP-demotion markers for `UploadProgress`/`HopTrace`, the pins-refusal rule — ratify
  as matrix notes so a future web leg inherits them.
- Deferred by precondition: the `MaybeSend` both-ways compile claim awaits any wasm
  target (24 #7).

## Harness/tooling track (not contract questions; schedule as a hardening step)

- File-sink path must be tier-provided, not Rust-chosen (`/tmp` unwritable on Android,
  F-M2-1).
- Harness hard-kills a row instead of leaning on the 5s `recv_timeout` (a leaked
  `/stall` call starves the ART instrumentation, F-M4-1).
- ALPN-capable TestServer before any h2/h3 or engine-matrix work (F-M3-2, F-M3-3).
- `StreamProbe.kt` comments still describe the pre-0.28.0 `trySend` drop behavior —
  stale since the upgrade; cleanup alongside whatever 3b's suite row becomes.

## Standing inputs

- S-WIN: **unparked.** Verified against released 0.28.0 (2026-07-21): finding 07 is fixed
  (distinct per-class stream runtimes + native symbols; the draft's `Snapshots()` routes
  to its own subscription) and the MarshalAs bug is fixed (out-param shape). The step-23
  resume is schedulable and small: a mechanical `GenProfileFfi` → `Gen_profile_ffi`
  namespace rename (~9 sites, 7 files) plus the tripwire's designed flip (it goes red →
  delete it, emit the real C13/C16 callback tests). The step-23 git-pin machinery is
  obsolete — the registry release suffices.
- Upstream issue (Defect 2, lifecycle/isolation) remains Henrik-files; the freeze design
  must not depend on it (streaming-seam §2).
