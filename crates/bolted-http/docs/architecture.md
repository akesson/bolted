# bolted-http — architecture: one contract, shipped native adapters

> "step 02" throughout refers to the `design/core-evolution` branch's probe plan (now
> [`crates/spike-profile-ffi-stall-probe/docs/probe-plan.md`](../../spike-profile-ffi-stall-probe/docs/probe-plan.md)),
> not main's independently-run `docs/steps/step-02-boltffi-probe.md`.

**Status:** shape settled in a design session (2026-07-09); the §4 packaging cluster **ran
the same day as a standalone spike and passed** — see
[spike-packaging-report.md](spike-packaging-report.md) (one-package packaging confirmed,
round-trip + error taxonomy verified, overhead measured at nanoseconds). The contract
sketch in §2 remains unfrozen pending the rest of step 02 (streaming, §5). Evidence base:
[prior-art.md](prior-art.md) (why every alternative shape fails) and
[platform-surfaces.md](platform-surfaces.md) (what each native stack can honor).
**2026-07-18:** a second investigation round produced the full homogenized feature matrix and
contract proposal ([feature-matrix.md](feature-matrix.md) — resolves the earlier studies'
verification flags, adds the skipped dimensions; raw evidence in `research/`) and the
five-platform verification plan ([spike-plan.md](spike-plan.md)). Where they conflict with
this doc or the studies, the matrix wins.

## 1. The shape

Three layers, only the first of which is visible to facet authors:

```text
┌──────────────────────────────────────────────────────────────────┐
│ bolted-http (Rust, sans-io)                                      │
│   the CONTRACT: HttpRequest / HttpResponse / HttpError data,     │
│   the Http capability trait, optional capability traits,        │
│   the conformance suite                                          │
├──────────────────────────────────────────────────────────────────┤
│ generated glue (bolted-ffi / BoltFFI)                            │
│   the capability trait crossed as a callback interface — the    │
│   same machinery as every other capability                       │
├──────────────────────────────────────────────────────────────────┤
│ shipped adapters (Bolted's, one per platform)                    │
│   BoltedHttp.swift  → URLSession            (in the Swift pkg)  │
│   BoltedHttp.kt     → OkHttp / WorkManager  (in the Kotlin pkg) │
│   BoltedHttp.cs     → WinRT HttpClient      (in the C# pkg)     │
│   bolted-http-linux → reqwest/curl          (Rust)              │
│   bolted-http-web   → fetch via wasm-bindgen (Rust, zero FFI)   │
└──────────────────────────────────────────────────────────────────┘
```

Facet code never calls an HTTP client: `update` emits a typed `HttpRequest` **effect**, the
driver hands it to the adapter, and the completion re-enters the core as an input. Because
requests and completions are typed core inputs, HTTP participates in replay, determinism, and
cross-platform conformance for free — a uniform API that is also part of the recorded input
stream.

The adapters are **rung-2 shipped components** — the effect-side siblings of
`BoltedTextField`: written by Bolted once per platform (hand-written first, spike
discipline), shipped with the generated bindings, wired in by default at core construction.
App developers neither write nor see them; per-platform configuration (extra trust roots,
proxy overrides) happens at the composition root, never in core code.

## 2. The contract (sketch — frozen only after step-02 evidence)

Portable core, honored identically by every adapter (derived in platform-surfaces §7):

- Request: method, URL, headers, body as `Bytes | File`, **one total deadline** (the only
  timeout every surface can honor).
- Redirects are followed by the stack; the final URL is reported; no hop interception.
- **Cookie-less and cache-less by default** (the platform defaults conflict; an effect-shaped
  request carries no ambient state).
- HTTPS-only; cleartext is a dev-only, platform-config-gated exception.
- HTTP version is an **observable** in the response, never a request parameter.
- Errors as typed keys + params (Bolted's error rule), with the native-failure → key mapping
  conformance-tested per adapter.

Optional capabilities, typed so an unsupporting adapter fails to compile, not at runtime:

- `FineTimeouts` (connect/read/write — absent on Darwin/web),
- `UploadProgress` (web adapter must drop to XHR),
- `Pinning` — **declarative SPKI data, never callbacks**; absent on web,
- `Metrics` (DNS/connect/TLS/first-byte timing; absent-or-coarse on web),
- `BackgroundTransfer` — a **separate effect family**, never a flag on the request effect:
  durable, serializable, file-based transfer descriptors with stable identities, handed over
  entirely (no per-chunk hooks), completion delivered as an input to a possibly-new core
  instance; force-quit loss is legal. Android's extra freedom (app code may run) must not
  leak into the contract or iOS cannot implement it. Precondition shared with interaction
  replay (ARCHITECTURE §9): effects as durable data with stable identities.

Never in the contract: proxy and trust configuration (adapter/OS-owned), cookie ownership,
streaming request bodies (web cannot). Whether **response streaming** makes the portable core
is decided by the step-02 BoltFFI stream findings, not by the platforms.

## 3. Adapter placement: shell-side, by decision

The adapters for Apple/Android/Windows are written in the shell language, not as Rust
bindings to native stacks. Rejected alternative: one Rust client binding native APIs
directly (objc2 → NSURLSession, windows-rs → WinRT, JNI → OkHttp; nyquest's approach).
Why:

- **Android decides it**: there is no maintained Rust path to OkHttp/Cronet/WorkManager;
  shell-side is the only credible Android adapter, so it is the uniform default.
- The capability-callback mechanism must exist anyway (it is the general capability pattern,
  probed in step 02); shell-side adapters add zero new machinery.
- Native APIs are used in their home idiom (URLSession delegates in Swift, WorkManager in
  Kotlin) — the platform-surfaces study shows how much of each surface is delegate/lifecycle
  shaped.
- One sliver is shell-mandatory regardless: iOS's `handleEventsForBackgroundURLSession`
  must live in the app delegate (`bolted new` scaffolding territory; Bolted cannot own it).

The cost accepted: the request→native→error mapping exists once per shell language, so the
**conformance suite runs per shipped adapter** (same request ⇒ same typed response/error on
every platform). That suite is not optional — it is what kept even Google's lean four-
implementation contract honest (prior-art §3b) — and the divergence matrix should be
generated from the capability types, not hand-written.

**Recorded retreat:** the contract is placement-blind. If step-02 measurements (callback-
trait ergonomics, FFI payload cost at request frequency) come back ugly, Darwin/Windows
adapters can move to Rust-side native bindings as an implementation change, not a design
change. Android stays shell-side in every world.

## 4. What step 02 must verify (the spike)

The packaging cluster, added to the step-02 sketch in ROADMAP:

1. **Packaging (the load-bearing question).** `boltffi pack` output must coexist with
   hand-written adapter source in **one consumable artifact per platform**: a Swift package
   containing both the generated bindings and `BoltedHttp.swift`, importable as a single
   dependency. If shipped-hand-written-source-next-to-generated-code is not expressible in
   BoltFFI's packaging model, the adapter story needs a design session before step 03.
2. **The capability round-trip.** A minimal `Http` capability as a BoltFFI callback trait;
   the Swift adapter executes a real URLSession request; the completion re-enters the core
   as a typed input (single-flight token pattern). This is the effect-side analog of the
   observation-contract cluster.
3. **Measurements.** Callback-trait call overhead at request frequency; request/response
   payload cost across the boundary (headers + body bytes); which thread the completion
   callback arrives on.
4. **Error taxonomy probe.** Map three real URLSession failures (timeout, DNS, TLS) to typed
   error keys through the boundary — the first rows of the conformance suite.

Not in scope for step 02: background transfer (needs durable effects — Phase 2 at the
earliest), Kotlin packaging (step 05's analog probe), the full contract design.

## 5. Open after the spike

- Response streaming in or out of the portable core (BoltFFI stream semantics).
- Cookie capability shape, if any facet ever needs cookie values.
- Whether Android's declarative `<pin-set>` binds OkHttp/Cronet (undocumented — affects
  whether `Pinning` is declarative-only or needs adapter code on Android).
- The `BackgroundTransfer` contract in full (durable-effect design shared with stash/restore
  and replay).
