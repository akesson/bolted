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

/// D28: the committed Kotlin contract suite is exactly what the declaration generates. Byte equality,
/// same as the codec — the suite is emitted; the values-only `ProfileConformanceFixture.kt` beside it
/// is hand-written and not drift-checked (its correctness is the Kotlin compiler's + the emulator's).
#[test]
fn the_committed_kotlin_contract_suite_matches_the_declaration() {
    let source = include_str!("../../gen-profile/src/lib.rs");
    let committed = include_str!(
        "../../../android/profile-probe/src/androidTest/kotlin/dev/bolted/profileprobe/generated/ProfileConformanceSuite.kt"
    );
    if let Err(e) = bolted_ffi_gen::check_kotlin_contract_suite_drift(
        source,
        "com.example.gen_profile_ffi",
        "dev.bolted.profileprobe.generated",
        committed,
    ) {
        panic!("{e}");
    }
}

/// D28: the committed Swift contract suite is exactly what the declaration generates. Byte equality;
/// the values-only `ProfileConformanceFixture.swift` beside it is hand-written and not drift-checked.
#[test]
fn the_committed_swift_contract_suite_matches_the_declaration() {
    let source = include_str!("../../gen-profile/src/lib.rs");
    let committed = include_str!(
        "../../../apple/profile-probe/Tests/ProfileProbeTests/Generated/ProfileConformanceSuite.swift"
    );
    if let Err(e) =
        bolted_ffi_gen::check_swift_contract_suite_drift(source, "GenProfileFfi", committed)
    {
        panic!("{e}");
    }
}

/// D28: the committed C# contract suite is exactly what the declaration generates (step 29). Byte
/// equality, same as the Kotlin/Swift suites — the suite is emitted; the values-only
/// `ProfileConformanceFixture.cs` beside it is hand-written and not drift-checked (its correctness is
/// the C# compiler's + `dotnet test`'s). `Gen_profile_ffi` is the binding namespace the 0.28.0 IR
/// backend names after the raw crate; `ProfileProbe.Generated` is the emitted suite's namespace.
#[test]
fn the_committed_csharp_contract_suite_matches_the_declaration() {
    let source = include_str!("../../gen-profile/src/lib.rs");
    let committed =
        include_str!("../../../csharp/profile-probe/Generated/ProfileConformanceSuite.cs");
    if let Err(e) = bolted_ffi_gen::check_csharp_contract_suite_drift(
        source,
        "Gen_profile_ffi",
        "ProfileProbe.Generated",
        committed,
    ) {
        panic!("{e}");
    }
}
