//! `bolted-http` — the platform-neutral HTTP capability contract.
//!
//! Sans-io: this crate defines the `Http` capability trait and the typed request /
//! response / error data that cross the effect boundary. Execution always belongs to a
//! platform adapter over the native stack (URLSession, OkHttp/Cronet, WinHTTP/WinRT
//! BackgroundTransfer, libcurl/reqwest on Linux, `fetch` on web) — never to this crate.
//!
//! **Deliberately empty for now.** The name and the contract's home are staked out; the
//! contract itself is designed after spike steps 02–03 produce friction evidence. Design
//! docs live in `docs/` next to this crate: `architecture.md` (the settled shape: contract
//! crate + Bolted-shipped shell-side adapters, and what step 02 must verify), `prior-art.md`
//! (previous cross-platform HTTP attempts and where they fail), and `platform-surfaces.md`
//! (the native API surfaces the adapters must map).
#![forbid(unsafe_code)]
