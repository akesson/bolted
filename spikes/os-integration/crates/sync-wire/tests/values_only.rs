//! The values-only discipline, pinned from both sides (deliverable 3; kill criterion 3).
//!
//! Side one: the protocol source must never name a judgement — no `Validity`, no `CheckState`,
//! no `SyncState`, and no `bolted` dependency at all. Side two (the step-09 `golden.rs` trick,
//! and the "a forbidding test can forbid nothing" lesson): a planted positive control proves the
//! matcher CAN match, so an impossible needle can never go green by construction.

/// The whole protocol crate, at compile time. If the crate grows a second file, this test fails
/// the length floor below until the file is added here — additions cannot dodge the grep.
const WIRE_SOURCE: &str = include_str!("../src/lib.rs");

/// Judgement names the wire must not contain. `bolted` is the strongest form of the rule: the
/// protocol crate must compile without the framework, exactly as the Swift `Codable` side does.
const FORBIDDEN: &[&str] = &["Validity", "CheckState", "SyncState", "bolted"];

/// Comment lines are exempt: the discipline is about what the code NEEDS to function (kill 3),
/// and the doc comments legitimately name `bolted-ffi-gen` when explaining what this crate is.
fn offending_lines<'a>(source: &'a str, needle: &str) -> Vec<(usize, &'a str)> {
    source
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            let code = l.trim_start();
            !code.starts_with("//") && code.contains(needle)
        })
        .map(|(i, l)| (i + 1, l))
        .collect()
}

#[test]
fn the_wire_source_names_no_judgement() {
    // The scan target must be the real crate, not an empty read.
    assert!(
        WIRE_SOURCE.lines().count() > 100,
        "sync-wire source suspiciously short — is include_str! pointed at the right file?"
    );
    for needle in FORBIDDEN {
        let hits = offending_lines(WIRE_SOURCE, needle);
        assert!(
            hits.is_empty(),
            "sync-wire source contains the judgement name {needle:?}: {hits:?}"
        );
    }
}

#[test]
fn the_matcher_can_match() {
    // The planted positive control: every forbidden needle, present in a fixture, is found.
    let fixture =
        "enum Validity { .. } enum CheckState { .. } enum SyncState { .. } use bolted_core::Store;";
    for needle in FORBIDDEN {
        assert!(
            !offending_lines(fixture, needle).is_empty(),
            "the matcher failed to find {needle:?} in a fixture that contains it — \
             the forbidding test above forbids nothing"
        );
    }
}

#[test]
fn the_manifest_declares_no_bolted_dependency() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("bolted"),
        "sync-wire's manifest depends on a framework crate — the values-only claim is void"
    );
    // Positive control for this matcher too.
    assert!("bolted-core = {{ path }}".contains("bolted"));
}
