# spike-http-ffi

Standalone probe for the step-02 **third cluster** — capability adapter packaging
(`crates/bolted-http/docs/architecture.md` §4). Findings:
[`crates/bolted-http/docs/spike-packaging-report.md`](../bolted-http/docs/spike-packaging-report.md).

Deliberately **outside** the bolted workspace (own `[workspace]` in Cargo.toml) so the
shared `Cargo.lock` and `mise run check` are unaffected.

## Layout

- `src/lib.rs` — spike core: `#[data]` request/response/error types, the `HttpAdapter`
  callback trait, `SpikeCore` (token-issuing fetch + completion re-entry).
- `package/Sources/SpikeHttp/BoltedHttp.swift` — the hand-written URLSession adapter
  (the only committed file under `package/`; the rest is generated).
- `consumer/` — a Swift package that depends on `package/` as ONE dependency and runs
  the probe tests (network required).

## Run

```sh
boltffi pack apple --release   # generates package/: Package.swift, xcframework, bindings
cd consumer && swift test      # correctness probes
swift test -c release --filter testMeasurements   # honest overhead numbers
```
