# Spike report — capability adapter packaging (step-02 third cluster, run early)

**Date:** 2026-07-09 · **Where:** `crates/spike-http-ffi/` (standalone crate, outside the
workspace) · **Toolchain:** boltffi 0.27.3 CLI / 0.27.4 crates, Rust 1.95, Swift 6.3.3,
arm64 macOS host. All probes from [architecture.md §4](architecture.md) ran; **both step-02
kill criteria relevant to this cluster were cleared.**

## Verdict table

| Probe | Result |
|---|---|
| 1. Packaging (load-bearing) | **Yes** — one SwiftPM package holds hand-written adapter + generated bindings + xcframework, importable as a single dependency |
| 2. Capability round-trip | **Works** — trait → URLSession → typed completion re-enters core; single-flight token honored |
| 3. Callback overhead | Rust→Swift trait call ≈ **8 ns**; Swift→Rust method ≈ **1.3 ns** (release); 1 MB payload round-trip ≈ **30 µs** |
| 4. Error taxonomy | timeout / DNS / TLS all map to typed keys through the boundary against real failures |

## 1. Packaging — how "one artifact" actually works

The `bundled` SPM layout is the shipped-adapter story, but its shape was undocumented and
took one wrong guess to find. The working configuration:

```toml
[targets.apple]
output = "package"                     # ← your EXISTING package root, not a dist dir

[targets.apple.spm]
layout = "bundled"
wrapper_sources = "Sources/SpikeHttp"  # ← your target's source dir, relative to output
```

`boltffi pack apple --release` then writes into `package/`: `Package.swift` (binary target
+ one Swift target whose `path` is `Sources/SpikeHttp`), the xcframework, and the generated
bindings **injected as `Sources/SpikeHttp/BoltFFI/…swift` next to the hand-written
`BoltedHttp.swift`**. Hand-written and generated code compile into the *same module* — the
adapter needs no `import`. A consumer package adds `package/` as one path dependency and
one product; `swift test` proves it end to end.

The wrong guess (worth recording): leaving `output = "dist/apple"` and pointing
`wrapper_sources` at a source dir elsewhere produces a Package.swift whose target path
dangles — pack does **not** copy wrapper sources into the output. `output` must *be* the
package root you ship.

## 2. The capability round-trip

Exactly the architecture's shape, verified against real endpoints:

- `SpikeCore::fetch` emits a typed `HttpRequest` (token, method, url, headers, body,
  `deadline_ms`) through `#[export] trait HttpAdapter` → Swift protocol.
- `BoltedHttp.swift` (hand-written, cookie-less/cache-less ephemeral URLSession, one total
  deadline via `timeoutInterval`) executes and calls `core.completeOk/-Err` — the
  completion re-enters as a typed input.
- Single-flight: unknown tokens are dropped; the first completion wins; duplicates are
  ignored (test-verified).
- Redirects: stack follows silently, final URL reported (`https://apple.com/` →
  `https://www.apple.com/`).
- Composition-root wiring is a three-line dance (adapter → core(adapter) → weak back-ref
  `adapter.core = core`); the back-reference must be weak or it cycles across the FFI.

## 3. Measurements (release; debug in parentheses)

| Measure | Result |
|---|---|
| Swift→Rust no-op method | 1.3 ns/call (78 ns) |
| Rust→Swift callback trait (`ping`) | 8.3 ns/call (11 ns) |
| 1 KB bytes round-trip (`echo_len`) | 0.42 µs (0.83 µs) |
| 64 KB | 2.8 µs (3.5 µs) |
| 1 MB | 30 µs (31 µs) |
| Completion thread | **background** NSThread (URLSession delegate queue) — never main |

At request frequency (per-user-action, not per-frame) all of this is noise. The completion
arriving off-main means (a) the core's input entry points must tolerate non-main callers —
`SpikeCore` used a `Mutex`; the real core's threading contract is a step-02/06 question —
and (b) the driver owns the hop to the main thread before snapshots touch UI.

## 4. Error taxonomy — first conformance rows

`URLError` → typed key, verified against live failures:

| Native failure | Typed key |
|---|---|
| `.timedOut` (non-routable 10.255.255.1, 1.5 s deadline) | `Timeout { deadline_ms }` |
| `.cannotFindHost` (`*.invalid` host) | `DnsFailure { host }` |
| `.serverCertificateUntrusted` −1202 (self-signed.badssl.com) | `TlsFailure { reason }` |

Bonus finding for the **second** cluster: payload-carrying `#[data]` enums (undocumented in
BoltFFI) generate clean Swift enums with associated values (`case timeout(deadlineMs:
UInt64)`), `Hashable/Equatable/Sendable`. `Vec<u8>` ↔ `Data`. Rust doc comments propagate
into the generated Swift.

## Friction log

- **F1** Hyphenated crate names break BoltFFI symbol generation (`invalid native symbol
  name`); the crate had to be named `spike_http_ffi`. `bolted new` scaffolding should
  underscore crate names for FFI crates.
- **F2** The `bundled` layout's `output`-is-the-package-root semantics are undocumented
  (see §1); cost one failed iteration.
- **F3** `#[data]` does not derive `Clone`; a manual `#[derive(Clone)]` alongside works.
- **F4** `boltffi init` enables android/wasm by default; they fail on machines without
  NDK unless disabled.
- **F5** Test-network quirk: this network resets TLS to `example.com`; probes use
  `apple.com`/`badssl.com`. Conformance-suite proper should use a local test server.

## Open questions routed onward

- The core's **threading contract** for capability completions (Mutex? single-threaded
  driver queue? — touches ARCHITECTURE §6's synchronous reduce loop; do not resolve here).
- Generated `Package.swift` is overwritten on every pack — where do app-added targets go
  when the app's own package is also the pack output? (Probably: apps depend on the packed
  package rather than packing into their own — `bolted new` decides.)
- Streaming bodies, cookie capability, `<pin-set>`: unchanged, still step-02/05 questions.
