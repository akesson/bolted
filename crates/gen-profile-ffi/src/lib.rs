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

/// Walking-skeleton probe (step 02, milestone 1). Kept so `SkeletonTests` stays green.
#[boltffi::export]
pub fn ping(input: String) -> String {
    format!("pong: {input}")
}
