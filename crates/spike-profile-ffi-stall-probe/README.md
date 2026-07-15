# spike-profile-ffi-stall-probe

An independent BoltFFI due-diligence probe ([plan](docs/probe-plan.md)) run 2026-07-09 on
the `design/core-evolution` branch: the step-01 profile feature exported over BoltFFI
(Apple) — the four load-bearing features (C1) and the observation contract (C2).
Headline finding — **BoltFFI 0.27.4 push-mode stream delivery stalls permanently under
concurrent load** (mechanism + fix identified): [docs/probe-report.md](docs/probe-report.md).

**Not** main's step-02 probe: main ran its own at `crates/spike-profile-ffi/` (different
wrapper design, different verdict — see the provenance banner in the report). The Rust
package inside is still named `spike_profile_ffi`, same as main's crate; the two never
build together (this one is standalone), but their generated native symbols would collide
if both packages were ever linked into one binary.

Deliberately **outside** the bolted workspace (own `[workspace]` in Cargo.toml) so the
shared `Cargo.lock` and `mise run check` are unaffected. Path-depends on `bolted-core` and
`spike-profile`, read-only (wrapper updated once during the rebase onto main's evolved
core: `rebase(entity, version)`, `commit()` returning the draft on refusal).

## Layout

- `src/lib.rs` — the hand-written as-if-generated wrapper: `#[data]`/`#[error]` mirrors,
  `ProfileFacet` + `ProfileDraftFfi` exported classes (thread-safe re-hosting of the
  prototype store's plumbing; semantics stay in `bolted-core`/`spike-profile`), stream
  probes in all three modes, the `single_threaded` side probe.
- `docs/` — the probe plan and report (moved from the branch's `docs/steps/step-02-*.md`).
- `package/` — **entirely generated** by `boltffi pack apple` (nothing committed).
- `consumer/` — Swift package depending on `package/` as ONE dependency; hosts the probe
  tests (`testC1…`, `testC2…`) and measurements.

## Run

```sh
boltffi pack apple --release          # generates package/: Package.swift, xcframework, bindings
cd consumer && swift test             # probes; PROBE lines record observed stream behavior
swift test -c release --filter Measurements   # honest overhead numbers
```
