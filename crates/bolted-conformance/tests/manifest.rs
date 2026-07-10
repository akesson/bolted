//! The suite's own rung-3 check (VISION's verification ladder): the document, the suite and the
//! stampers must not drift apart. Verified by the build, not by review.
//!
//! Three claims, not the two the step-06 version made. The third is new, and it exists because the
//! extraction created a place for a test to hide: a generic `cNN_*` function that no `*_suite!` macro
//! stamps compiles, type-checks, is documented — and never runs.

use std::collections::BTreeSet;
use std::fs;

fn src(file: &str) -> String {
    let path = format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} must be readable: {e}"))
}

/// Every `pub fn cNN_*` defined in the suite, by full name.
fn suite_functions() -> BTreeSet<String> {
    ["value.rs", "field.rs", "feature.rs"]
        .iter()
        .flat_map(|f| {
            src(f)
                .lines()
                .filter_map(|l| l.trim().strip_prefix("pub fn c")?.split('<').next())
                .filter(|name| name.len() >= 3 && name[..2].chars().all(|c| c.is_ascii_digit()))
                .map(|name| format!("c{name}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The C-ID each suite function claims, e.g. `c19_rebase_is_…` → `C19`.
fn id_of(function: &str) -> String {
    format!("C{}", &function[1..3])
}

/// Every normative `CNN` row in the document.
fn documented_ids() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONFORMANCE.md");
    let doc = fs::read_to_string(path).expect("docs/CONFORMANCE.md must exist");
    doc.lines()
        .filter_map(|l| l.strip_prefix("| C"))
        .filter_map(|rest| rest.split('|').next())
        .map(|id| format!("C{}", id.trim()))
        .filter(|id| id.len() == 3 && id[1..].chars().all(|c| c.is_ascii_digit()))
        .collect()
}

#[test]
fn every_normative_id_has_a_test() {
    let implemented: BTreeSet<String> = suite_functions().iter().map(|f| id_of(f)).collect();
    let documented = documented_ids();
    assert!(!documented.is_empty(), "parsed no IDs from CONFORMANCE.md");

    for id in &documented {
        assert!(
            implemented.contains(id),
            "{id} is normative in docs/CONFORMANCE.md but has no `{}_*` function",
            id.to_lowercase()
        );
    }
}

#[test]
fn every_test_is_a_normative_id() {
    let documented = documented_ids();
    for function in suite_functions() {
        let id = id_of(&function);
        assert!(
            documented.contains(&id),
            "`{function}` exists but {id} is not a normative row in docs/CONFORMANCE.md"
        );
    }
}

/// A test nobody stamps is a test nobody runs. Before the extraction this could not happen — the
/// tests *were* `#[test]` functions. Now they are generic functions a macro turns into tests, and
/// forgetting the macro line is a silent, permanent loss of coverage.
#[test]
fn every_suite_function_is_stamped_by_exactly_one_macro() {
    let macros = src("macros.rs");
    for function in suite_functions() {
        let call = format!("$crate::{function}::<");
        let stamped = macros.matches(&call).count();
        assert_eq!(
            stamped, 1,
            "`{function}` is stamped {stamped} times in macros.rs; a suite function must be \
             stamped by exactly one `*_suite!` macro, or no fixture will ever run it"
        );
    }
}
