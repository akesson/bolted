//! `profile-web` — the hand-written stand-in for a future generated Leptos shell (step 04).
//!
//! Same "write what the codegen would emit" discipline as steps 01–03, now for a Rust web face:
//! the profile feature consumed **directly as crates** (zero FFI, no BoltFFI, no codegen), running
//! in the browser as a single-threaded wasm module.
//!
//! Split for the two test tiers (see the step doc, Deliverable C):
//! - [`controller`] + [`l10n`] are framework-light and host-safe — the semantics live here and are
//!   tested with plain `cargo test` (no browser, no Leptos).
//! - [`app`] is the Leptos view layer, compiled only for wasm32; the headless `test:web` tier
//!   proves its DOM binding is wired.
#![forbid(unsafe_code)]

pub mod controller;
pub mod l10n;

#[cfg(target_arch = "wasm32")]
pub mod app;
