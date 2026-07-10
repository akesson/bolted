//! `gen-note-ffi` — the FFI layer of `gen-note`, generated.
//!
//! There is nothing else in this crate. `generated.rs` is a real module file, because BoltFFI's
//! bindgen reads source text and would silently ignore anything a macro produced (step 10, M0).

pub mod generated;

/// **Load-bearing, and nothing else says so.**
///
/// `boltffi pack` builds the crate with `BOLTFFI_BINDING_EXPANSION` set, which makes the first
/// `#[data]`/`#[export]` the compiler expands emit a whole-crate metadata blob. That blob names every
/// exported type **from the crate root**, wherever it happens to be injected. Without this re-export,
/// `pack` dies with `cannot find type NoteStoreFfi in this scope`, pointing at a `#[data]` attribute
/// on an unrelated enum.
///
/// `mise run check` cannot catch it: the blob only exists under the pack's environment variable. See
/// `docs/steps/artifacts/step-10-boltffi-visibility/`.
pub use generated::*;
