# The wire, priced — what `bolted-ffi-gen` would emit for the daemon topology

**Artifact of the topology design pass (D31).** Every number below is `wc -l` on the spike code
under `spikes/os-integration/` at the close of step 20 — hand-written once, exercised by the
whole probe matrix (steps 18–20), and therefore the honest estimate of what a wire emitter has
to produce per feature. This is a *price list*, not a build order: D31 records why the emitter
is not built yet.

## The inventory

| Artifact | Spike original | Lines | Emitted per | Notes |
|---|---|---|---|---|
| Wire protocol (envelope, request/response frames, push frames, blocking Rust client) | `crates/sync-wire/src/lib.rs` | 486 | feature | D27-versioned envelope; **zero bolted deps** — pinned by `values_only.rs` with a planted control |
| Daemon body (accept loop, thread-per-connection, connection-scoped draft ownership, check-token registry, push ticks two-phased around the one `Mutex<Store>`) | `crates/syncd/src/lib.rs` | 672 | feature | The store loop the FFI wrapper already taught (D16): collect fan-out under the lock, flush after it |
| launchd activation shim | `syncd/src/main.rs`, `mod launchd` | ~40 | OS | One C call (`launch_activate_socket`); the bin's only `unsafe` |
| systemd activation shim | `syncd/src/main.rs`, `mod systemd` | ~40 | OS | Pure std over the `LISTEN_PID`/`LISTEN_FDS` env protocol (sd_listen_fds(3)); guards are ordinary unit tests |
| Shared serve loop (`serve_adopted`: `Vec<UnixListener>` + idle-exit) | `syncd/src/main.rs` | ~80 | once | Both shims reduce to it — asymmetric in kind, identical in shape |
| Foreign wire mirror (Codable frames + refusal decode) | `apple/finder-citizen/Sources/SyncWireKit/Wire.swift` | 298 | feature × language | The D28 road: committed generated source, byte-compared |
| Blocking foreign client | `SyncWireKit/LineClient.swift` | 128 | language | Probes, CLIs, one-shot surfaces |
| Push demultiplexer client (reader thread, correlated responses to blocked requesters, pushes to a callback queue) | `SyncWireKit/WireConnection.swift` | 178 | language | What a push-driven UI turns out to require; ~180 lines of subtle threading that should be written once, correctly |
| launchd agent plist / systemd units | step-19 plist · `linux/syncd.socket` + `syncd.service` | ~20 / 23 | OS × product | Scaffold output (`bolted new`), not codegen proper |
| Bundle assembly (plists, entitlements, inside-out signing) | `apple/finder-citizen/scripts/assemble-app.sh` | 79 | product (macOS) | The R1 pricing artifact from step 19 — everything scaffolding must emit for the tray/badges promise |

What stays hand-written per product: the surfaces themselves (SwiftUI views, the FinderSync
handler, a tray menu) and the ViewModel glue — exactly the split the in-process FFI targets
already have. `SyncViewModel.swift` (287 lines) is the measured cost of that glue for a
three-field feature with echo rule, conflicts, async check, submit, and continuous stash.

## Requirements the friction logs pinned (the emitter's spec, not advice)

1. **`CheckToken` never crosses.** It is private by design; the connection layer holds it and
   issues its own correlation id (step 18 friction 1). The emitter owns this registry because it
   owns the declaration.
2. **Verdicts cross as closed data.** The wire carries `ok: bool`; the daemon maps failure to the
   *declared* `failed_key` — a client cannot invent an error key (step 18 friction 2).
3. **Object shapes, never serde tuples.** `(String, String)` params forced hand-written Codable
   decoders (step 18 friction 3).
4. **Open-then-verify is unconditional.** `connect(2)` success proves only that some init system
   holds a listener — on both OSes (steps 19/20). Ping before believing; treat systemd's ~45 ms
   queued-accept as the good day, not the contract (the unaccepted-limbo pathology is
   launchd-only so far).
5. **Two client shapes** (blocking + demultiplexer) — the table above prices both.
6. **The continuous-stash idiom**: the client refreshes its stash after every mutation, or a
   daemon `kill -9` leaves it nothing to restore (step 19, U4/H6). Measured cost: one µs-scale
   round-trip per keystroke — U5's three-round-trip pair still cleared its bar ~150×.
7. **Push ordering across concurrent mutators is not globally serialized**; version numbers on
   every tick make it safe (clients dedupe/fetch), and the generated client library must say so
   (step 18 friction 5).
8. **`AlreadySubmitted` flattens to `UnknownDraft`** at the connection-ownership gate — ruled
   acceptable by D31: a transport-layer refusal, checked before the store is asked, distinct
   from the contract's own taxonomy. Recorded, not hidden.
9. **The stash blob lives client-side** (the process that survives a daemon death) and re-enters
   through the D27 version gate; stash-at-rest versioning is D27's envelope, unchanged.
10. **The client library's draft-session entry points take the declared capabilities as explicit
    optional arguments** (D34, step 21) — the checker never crosses the wire (req. 1), so the
    capability is client-side state, and the generated client owes the same
    forgetting-is-a-compile-error shape the in-process FFI wrapper now has.

## The totals, for scale

Per feature ≈ **1 160 lines of Rust** (wire + daemon body) + **~600 lines per foreign language**
(mirror + both clients); per OS ≈ **40 lines** of activation shim + unit/plist files; per product
≈ the scaffold ceremony (79-line assembly script class). Latency of the result, measured:
keystroke pair 26–45 µs native macOS, ~100 µs through a real SwiftUI VM with stash, 120 µs
in-container Linux — every figure 8–150× inside its kill bar.
