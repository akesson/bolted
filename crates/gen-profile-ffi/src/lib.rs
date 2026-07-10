//! `gen-profile-ffi` — `spike-profile-ffi`, generated.
//!
//! `spike-profile-ffi` is 1 407 hand-written lines whose comments say, over and over, that this is
//! what a generator would emit. This crate is the cash-out: `src/generated.rs` is written from
//! `gen-profile/src/lib.rs` by `mise run gen:ffi` and byte-checked on every `mise run check` (D22).
//!
//! **`spike-profile-ffi` is not deleted, and must not be.** It is the reference. A step that edits its
//! own reference proves nothing.
//!
//! What is hand-written here, and only this:
//!
//! - `src/custom.rs` — the composite value object's projection. The generator refuses to guess it.
//! - `ping` — the walking-skeleton probe. Not derivable from any declaration; it is scaffolding the
//!   Swift `SkeletonTests` still assert on.

pub mod custom;
pub mod generated;

/// **Load-bearing, and nothing else says so.**
///
/// `boltffi pack` builds the crate with `BOLTFFI_BINDING_EXPANSION` set, which makes the first
/// `#[data]`/`#[export]` the compiler expands emit a whole-crate metadata blob. That blob names every
/// exported type **from the crate root**, wherever it happens to be injected. Without these
/// re-exports, `pack` dies with `cannot find type ProfileStoreFfi in this scope`, pointing at a
/// `#[data]` attribute in `custom.rs` that has nothing to do with it.
///
/// `mise run check` cannot catch it: the blob only exists under the pack's environment variable. See
/// `docs/steps/artifacts/step-10-boltffi-visibility/`.
pub use custom::*;
pub use generated::*;

/// Walking-skeleton probe (step 02, milestone 1). Kept so `SkeletonTests` stays green.
#[boltffi::export]
pub fn ping(input: String) -> String {
    format!("pong: {input}")
}
