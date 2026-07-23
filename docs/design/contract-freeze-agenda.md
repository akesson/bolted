# bolted-http contract review — agenda & decision record

**Compiled 2026-07-21** from the step-24/25/26 report §Open-questions lists (deduplicated;
step 26's numbering kept where they overlap) plus items that arrived after step 26. All
reachable implementors are in (mock, Linux/reqwest, Apple/URLSession, Android/OkHttp);
BoltFFI is at 0.28.0. Pointers only — the cited docs carry the evidence.

**Session held 2026-07-21; rulings recorded inline below.** Stance (Henrik): this is
unreleased, own-use software — "freeze" overstates it. These are working decisions,
valuable for coherence and recorded rationale, and expected to evolve as we learn;
re-open any of them when evidence disagrees. Two standing re-evaluation triggers are
upstream RFCs in draft (Q1, Q7).

## Contract questions

1. **The streaming seam** — **ruled: adopted as proposed** ([streaming-seam.md](streaming-seam.md):
   typed-input chunk re-entry, bounded ring + fail-loud with back-pressure as capability,
   `BodyEnd` terminal, driver-owned lifecycle). **With a standing re-evaluation trigger:**
   the upstream `ffi_stream` delivery-contract RFC (draft, Henrik) moves several
   enforcement layers into the binding when it lands — mapping and what-changes-then in
   streaming-seam.md §7. Also records the RFC's evidence correction (cross-subscriber
   starvation withdrawn; the leak stands).
2. **Redirect ceiling as CFG** at the composition root — also removes the classifier's
   one unavoidable exception-text match (OkHttp) and the honest-limit gap (all
   platforms). (24 #Q2 / 25 #2 / 26 #2.) **Ruled: adopted** — core-owned value,
   core-counted exhaustion.
3. **`content_length` semantics per sink kind** — the honesty split (gzip strips it;
   file sink can count) holds on all three platforms. (25 #3 / 26 #3.) **Ruled: adopted,
   after re-verification** that reliability is impossible in principle, not a platform
   gap: `Content-Length` frames the *encoded* content (RFC 9110), so the decoded length
   of a compressed or chunked response is unknowable up front on any client (live
   control 2026-07-21: wire `Content-Length: 94760`, decoded 611471 — 6.5×; reqwest
   documents the same; OkHttp strips the header on transparent gzip; Apple cannot
   disable transparent decode short of owning `Accept-Encoding`, and chunked responses
   carry no length at all). Wording: always advisory `Option`; the file sink reports
   verified bytes-written on completion.
4. **Push-cancellation seam** on the capability trait — three platforms now pay the
   poll-watcher thread; note the shared shape with streaming back-pressure
   (streaming-seam §3b: both are core→adapter mid-flight signals). (25 #4 / 26 #4.)
   **Ruled: adopted** — designed together with §3b's signal as one surface.
5. **`PermissionDenied` reachability** — two platforms gated identically
   (`AdapterOnly`, cause-mapping unit-proven); decide whether the contract states it as
   inherently device/app-bundle-tier. (24 #4 / 25 #5 / 26 #5.) **Ruled: adopted** — the
   contract states it; each adapter owes a unit-proven cause mapping with negative
   controls.
6. **`HttpError → Into<ErrorData>` bridge** (v1.14 residue). (24 #1 / 25 #6 / 26 #6.)
   **Ruled: ratified**; scheduled into the implementation step.
7. **Adapter packaging conventions → `bolted new` scaffolding rules** — SwiftPM and
   Gradle shapes both proven; plus the FFI-bridge-crate drift check (F-M0-1: bindgen
   reads source text, so bridge crates are per-target copies — but see Q10: if the
   surface goes uniform, the copies collapse). (25 #7 / 26 #7.) **Ruled: adopted
   (conventions as scaffolding rules; drift check made moot via Q10). With a standing
   re-evaluation trigger:** the upstream companion-sources RFC (draft, Henrik — capability
   crates shipping their own native implementations as metadata-channel records, with
   module-name placeholder substitution). If it lands, bolted-http's packaging story
   inverts: the contract crate ships its adapters as companions and the per-platform
   package assembly largely dissolves. Parked upstream behind RFC #665; re-evaluate when
   that chain moves.
8. **Conformance-scope boundary** — which invariants are shared-suite obligations vs
   per-adapter unit obligations (F-M4-2 pin ordering); should row 11 assert upload
   `total` (F-M4-3, unasserted on every implementor)? (26 #8.) **Ruled: adopted** —
   shared suite owns what the contract types can observe; platform-internal invariants
   are named per-adapter unit obligations; row 11 asserts `total` when `content_length`
   is known; plus the three new streaming rows (streaming-seam §5).
9. **The cookie capability's seam obligation** (row 26 stays open, but the shape does
   not): the per-hop consultation is the same mid-flight adapter→core re-entry as the
   streaming seam — define the shape once (streaming-seam §4, feature-matrix §5.20),
   even if the capability itself stays deferred. **Ruled: adopted.**
10. **Surface uniformity across platforms** — `PriorityHint` (apple-only) is the sole
    per-platform surface divergence, and it alone forces the two bridge crates: bindgen
    evaluates no `#[cfg]` at 0.28.0, so the union of items lands in every target's
    bindings (upstream note 08 — source-verified, probe pending), while one crate
    packing multiple targets is proven (`gen-profile-ffi`). Declare the hint uniform —
    a no-op where the engine can't honor it, which OkHttp already can't — and
    apple/android merge into one multi-target bridge crate. (Raised 2026-07-21,
    crate-consolidation review.) **Ruled: adopted** — with the precedent stated:
    uniform-with-no-op is preferred only when ignoring is legal per the capability's own
    contract (as row 12 makes it here); otherwise a divergence is real and gets a real
    seam. Note 08's runtime probe still owed before anything is *built* on cfg behavior.

## Smaller decisions — all ruled as recommended (2026-07-21)

- C3 divergence matrix: docs stay the record until a second CFG divergence exists —
  no schema growth for one row. (24 #5.)
- `SkipReason`: keep until S-WIN lands (C# may need it); delete if still unused then.
  (24 #6.)
- The web-evaluation trio (`HttpVersion::Unknown`, CAP-demotion markers for
  `UploadProgress`/`HopTrace`, the pins-refusal rule): ratified as matrix notes.
- The `MaybeSend` both-ways compile claim: stays deferred by precondition (any wasm
  target). (24 #7.)

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
