//! Note-08 runtime probe — does BoltFFI's bindgen evaluate `#[cfg]`?
//!
//! Two `#[data]` items: one unconditional (the control), one gated on
//! `#[cfg(target_os = "ios")]`. This crate is generated/packed for the **android** target.
//! rustc, compiling for `aarch64-linux-android`, excludes `IosOnlyHint` from the object code.
//! The question note 08 asks: does bindgen — which scans SOURCE TEXT, not expanded code —
//! still emit a Kotlin binding for `IosOnlyHint`?
//!
//! - If `IosOnlyHint` APPEARS in the generated Kotlin: bindgen ignores cfg → the **union claim**
//!   (a gated item joins every target's surface). This is what enables the single-crate merge.
//! - If it is ABSENT (cfg honoured), or generation ABORTS on the gated item: the union claim is
//!   wrong and kill-criterion 1 fires.

use boltffi::*;

/// The control: always present, on every target.
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlwaysHere {
    pub a: u32,
}

/// iOS-only at the Rust level — the `PriorityHint` stand-in from note 08. rustc excludes it from
/// an android build; whether bindgen still emits it is the whole question.
#[cfg(target_os = "ios")]
#[data]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IosOnlyHint {
    pub b: u32,
}

/// An exported fn so the generated surface is non-trivial (bindings actually get written).
#[export]
pub fn always_fn(x: u32) -> u32 {
    x + 1
}
