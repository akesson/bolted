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

/// D28: the committed Kotlin stash codec is exactly what the declaration generates. **Byte** equality
/// (not code equality, as above) — nothing formats a foreign generated file, which is what makes the
/// comparison honest. A hand-edit, or an `.editorconfig`/ktlint hook touching the path, fails here.
///
/// The composite half (`ProfileStashCustom.kt`) is hand-written and not drift-checked: it is the D25
/// escape hatch, one language out, and its correctness is the Kotlin compiler's job (rung 2).
#[test]
fn the_committed_kotlin_stash_codec_matches_the_declaration() {
    let source = include_str!("../../gen-profile/src/lib.rs");
    let committed = include_str!(
        "../../../android/profile-app/src/main/kotlin/dev/bolted/profileapp/generated/ProfileStashCodec.kt"
    );
    if let Err(e) = bolted_ffi_gen::check_kotlin_codec_drift(
        source,
        "com.example.gen_profile_ffi",
        "dev.bolted.profileapp.generated",
        committed,
    ) {
        panic!("{e}");
    }
}
