//! D22: the committed FFI layer is exactly what the declaration generates.
//!
//! `include_str!` rather than `std::fs`: the check is hermetic and runs inside `mise run check` on a
//! box with no boltffi CLI, no Xcode and no NDK. Generation is source text in, source text out.
//!
//! Byte equality, because BoltFFI reads source text — the committed file *is* the FFI surface, and a
//! surface that drifts from its declaration is a surface no one declared.

#[test]
fn the_committed_ffi_layer_matches_the_declaration() {
    let source = include_str!("../../gen-profile/src/lib.rs");
    let committed = include_str!("../src/generated.rs");
    if let Err(e) = bolted_ffi_gen::check_drift(source, "gen_profile", committed) {
        panic!("{e}");
    }
}
