# The streaming body's core-side seam — design (Q1)

**Status: adopted as proposed (session ruling, 2026-07-21) — with a standing
re-evaluation trigger: the upstream `ffi_stream` delivery-contract RFC (§7).** These are
working decisions, not immutable ones: Bolted is unreleased, single-user software, and
the API is expected to evolve as we learn — the value of deciding now is coherence and
recorded rationale, not lock-down. Re-open any of this the moment evidence disagrees.

Step 24 fixed the *mechanism* (row 16: CORE, `ffi_stream` async push — F1) and deliberately
left the seam itself open (step-24 report, open question 2): how a chunk re-enters the core
as a typed input, back-pressure, and the end-of-body signal. Steps 25/26 added the missing
evidence and one more obligation: the subscription lifecycle. This doc maps those four
sub-questions, states the evidence, and proposes a shape.

## 1. Evidence base (three platforms deep, plus upstream movement)

- **Transport is proven.** F1 push is lossless and ordered across the FFI on Apple (A1:
  200/200, 14-core saturation) and ART (N2: 200/200, 2-core saturation, both pacings).
  Native-side ingest never lost an item in any run on any platform. Latency is real but
  body-appropriate: p50 ≈ 25µs (Swift) vs ≈ 0.5–2.3ms (Kotlin, JNI + dispatcher hop).
- **Binding delivery was backend-defined.** F-M0-4: the generated Kotlin `callbackFlow`
  dropped silently on overflow at 0.27.5 (slow collector: 171/132/125 of 200). boltffi
  0.28.0 (#703) replaced `trySend` with a suspending `send` — re-measured on-device after
  the upgrade: 200/200 in every slow/under-load configuration. Swift remains
  `.unbounded` (lossless by memory growth). **No backend documents or lets the author
  declare its policy** (upstream issue R2, open) — today's safety is an implementation
  detail of two backends, not a contract.
- **The lifecycle defect is unfixed and cross-platform.** F-M3-1 (Apple) / F-M0-5 (ART):
  an abandoned consumer's subscription survives — native-side, proven to survive ART GC
  of every Kotlin-side referent. `awaitClose`-style hooks never fire for an abandoned
  consumer. Still present at 0.28.0 (re-confirmed during the upgrade: run-2 after an
  abandoned consumer delivers 0/200). **Re-diagnosis (upstream RFC analysis, 2026-07-21):
  the "starvation" of the *next* subscriber is not shared re-delivery state** — the
  native side is per-subscription isolated (own ring/condvar/continuation slot). The
  observed 0–90/200 decomposes into the silent-loss defect amplified by CPU contention
  from the stale subscription's busy drain loop, plus (on Swift) the lost-wake race
  fixed upstream in #678. The *leak* stands as measured; the cross-subscriber mechanism
  we inferred was wrong, and §3d's justification rests on the leak alone.

## 2. The design principle the evidence forces

**The seam must be correct against the weakest documented binding behavior, not the
currently-observed one.** 0.28.0 made the observed Kotlin behavior safe, but nothing
upstream promises it stays safe (R2/R5 open), and the lifecycle defect is live on both
platforms. So bolted assumes only what it can enforce itself:

1. per-subscription FIFO order (observed everywhere, and cheap to *verify* per chunk),
2. everything else — completeness, termination, lifecycle — is enforced core-side or by
   shipped rung-2 code, never delegated to the generated binding.

This is the same stance as the error and validation rules: the contract's guarantees are
bolted's to keep, adapters and bindings are implementation.

## 3. The four sub-questions, with proposed shapes

### 3a. Chunk re-entry: a typed input, token-keyed, seq-stamped

The adapter delivers each body chunk the same way it delivers a completion — a plain
callback-trait re-entry into the store-owned core, token-keyed to the in-flight request:

```rust
// adapter → core, per chunk (shape sketch; names for the session to settle)
fn deliver_chunk(&self, token: RequestToken, chunk: BodyChunk);
struct BodyChunk { seq: u64, bytes: Vec<u8> }
```

- Chunks are **inputs**: they enter the recorded input stream, so a streamed response
  participates in replay/determinism exactly like a completed one. (This is why the seam
  is re-entry, not an adapter-owned buffer handed over at the end.)
- `seq` is stamped by the core-side ingest counter contract, verified on arrival
  (ascending, gapless) — order becomes a *checked* invariant instead of a trusted one.
- The core owns the per-response ring (the harness bridge's `deliver_chunk` +
  `EventSubscription` shape, graduated from probe to contract). Capacity comes from the
  core — a constraint value, never a shell literal.

**Open for the session:** does `deliver_chunk` return a value (see 3b), and is `BodyChunk`
a new effect-input family or a variant of the completion input?

### 3b. Back-pressure and overflow: bounded ring, loud failure, adapter pause as the ceiling

Options considered:

- **A — unbounded ring.** Never loses, memory-unbounded on a stalled consumer. Rejected:
  it converts a slow consumer into an invisible leak, the exact failure mode the
  platforms' own `.unbounded` choice has.
- **B — bounded ring + fail-loud.** On ring-full, the response fails with a typed error
  (`StreamOverflow { capacity, seq }`). Silent loss impossible by construction; the
  failure is an enum with params (error rule), observable like any other typed failure.
- **C — B plus adapter back-pressure.** `deliver_chunk` returns a pause/resume signal (or
  a watermark callback) so a conformant adapter can stop reading the socket (every
  platform can: URLSession delegate suspend, OkHttp source read-pacing, reqwest stream
  polling). Overflow then only fires on a *broken* adapter.

**Recommend: freeze B as the contract obligation; C as a capability-shaped extension**
(same pattern as the push-cancellation seam, Q4 — and consider deciding them together,
since both are "the core signals the adapter mid-flight"). B alone is honest and small;
C removes the failure in practice without changing what the contract promises. The
conformance suite gets a slow-consumer row: delivered == ingested, or the typed overflow
error observed — green by drop is impossible (the N2 probe machinery becomes this row).

### 3c. End-of-body: a terminal input, distinct from the last chunk

A separate terminal re-entry, not a `last` flag on `BodyChunk`:

```rust
fn finish_body(&self, token: RequestToken, end: BodyEnd);
enum BodyEnd { Complete { total: u64 }, Failed(HttpError) }
```

- `Complete { total }` closes the completeness gate: the core checks
  `total == ingested count` and fails the response otherwise — truncation cannot
  masquerade as success (the step-24 one-shot-completion discipline, extended to
  streams: chunks then exactly one terminal, enforced by construction where possible).
- `Failed` gives the typed ended-vs-failed terminal the upstream spec lacks (issue R5)
  — mid-body transport errors arrive as data, not as a stream that just stops.
- A `last`-flag design was what the probes used; it worked, but it cannot carry the
  failure arm and makes "terminal chunk lost" indistinguishable from "still streaming".

### 3d. Lifecycle: driver-owned close; `ffi_stream` never reaches app code

The two-platform leak (F-M3-1/F-M0-5) is unfixable from our side and `awaitClose`-style
hooks are structurally the wrong tool (§1). Proposal:

- **The raw `ffi_stream` subscription is rung-2 internal.** It appears only inside
  shipped code (the driver/shell adapter pair), never in the app-developer surface —
  the facet observes typed state derived from chunks, through the ordinary observe
  triad. App code therefore *cannot* abandon a subscription it never holds.
- **Close is deterministic and driver-owned**: one live subscription per streaming
  response; the driver closes it on terminal (`BodyEnd`, error, cancel) via the explicit
  close path — `close(id)` as the only release path, per the store's no-lock discipline.
  This is exactly the discipline that made A1 hold under saturation (step 25's explicit
  `close_chunk_stream()` fix); it becomes a stated obligation instead of a probe fix.
- **A conformance row enforces it**: after N streamed responses, live-subscription count
  is back to baseline (the leak reproduces reliably enough on both platforms to make the
  row's red case real).

If upstream later ships scope-bound lifecycle (issue R3), it becomes a backstop under
this design, not a dependency of it.

## 4. Shared shape with the cookie capability (row 26)

`deliver_chunk`/`finish_body` and the cookie jar's per-hop consultation (feature-matrix
§5.20) are the same new thing: **mid-flight adapter→core re-entry on an in-flight
request**, token-keyed, between effect dispatch and completion. The freeze should define
that shape once (naming, token discipline, threading/ordering guarantees, replay
semantics of mid-flight inputs) and instantiate it twice — even if the cookie capability
itself stays deferred, the seam should be designed so it can attach without re-opening
the contract.

## 5. What this adds to the suite

- Slow-consumer completeness row (3b): delivered == ingested or typed overflow, on every
  adapter, under load.
- Terminal-exactly-once row (3c): chunks-then-one-terminal; truncation ⇒ failure.
- Subscription-hygiene row (3d): baseline live-count restored after streamed responses.
- The existing A1/N2 probes graduate from probe-grade to these C1 rows; the
  ingest-counter + corruption-control technique comes with them (non-vacuity stays).

## 6. Explicitly out of scope here

Streaming *request* bodies (excluded by design, feature-matrix §5.3); SSE/WebSocket
(parked with row 16's fallback note); changing the F1 mechanism choice (row 16 is
decided); anything resolving row 26 beyond the shared-shape requirement in §4.

## 7. Re-evaluation trigger: the upstream `ffi_stream` delivery-contract RFC

An upstream RFC is in draft (2026-07-21; drafted by Henrik, currently local to the
boltffi checkout as `RFC-stream.md`) that, if it lands, moves several of this design's
enforcement layers into the binding itself. **When any phase of it ships in a release,
re-evaluate this seam against it.** The mapping, per sub-decision:

- **§3b (bounded ring, fail-loud).** The RFC makes the subscription ring *the only
  buffer* (no backend-private stage), with author-declared `capacity` and `overflow`
  including `overflow = "fail"` — a typed terminal *failed* event on overflow. That is
  our `StreamOverflow` semantics, provided by the declaration itself. Once available:
  declare the body stream `fail` and demote the core-side gate to defense-in-depth
  (the seq check stays — it is what *verifies*). The RFC's producer `push` return value
  (delivered/dropped counts) also gives `deliver_chunk` synchronous loss visibility.
- **§3b (adapter back-pressure capability).** The RFC's named open question — the
  producer wake-on-space hook — cites exactly our HTTP-body producer as the use case
  that would justify it. Our capability-shaped pause/resume is the interim; if the hook
  lands, the capability can ride it instead of a bespoke signal.
- **§3c (BodyEnd terminal).** The RFC adds the wire-level ended-vs-failed terminal
  distinction (its one ABI change). That covers the *FFI* leg; `BodyEnd` remains ours —
  it is domain data (total-count completeness gate, `HttpError` taxonomy) on the ingest
  seam, which the RFC does not and should not know about.
- **§3d (lifecycle).** The RFC specifies a consumer-lifetime backstop per backend
  (JVM `Cleaner`, Swift iterator `deinit`, C# finalizer) plus a normative registration
  guarantee (delivery-start defined — a defect our probes dodged with sleeps without
  noticing). Driver-owned deterministic close stays our primary path (the RFC itself
  keeps explicit cancellation as the deterministic path); the backstop turns our
  "app code never holds the raw stream" rule from load-bearing into belt-and-braces —
  worth revisiting *then* whether the rule is still worth its API-shape cost.
- **§1 evidence correction.** The RFC's analysis withdraws the cross-subscriber
  starvation claim (per-subscription isolation already holds); see the amended
  evidence bullet above. Our upstream issue's Defect 2 remains real as a *leak*.

Net: nothing in the adopted shape is wasted — the contract-level guarantees stay
identical; what changes is which layer enforces them. Design the implementation step so
the enforcement can be delegated downward without touching the contract types.
