//! The two rung-3 pins that keep doctor honest (step 22). Doctor aggregates requirements the
//! `mise.toml` task guards already state, and an aggregate that can drift from its sources is
//! the two-contracts failure D25/D28 exist to kill. So, inside `mise run check`:
//!
//! 1. **The coverage manifest, both directions**: every `mise.toml` task maps to ≥1 doctor row
//!    or carries a recorded exemption reason — and every mapped/exempted task name still exists.
//!    Adding a machine-bound verb without deciding its doctor row fails the build; so does a
//!    row or exemption going stale.
//! 2. **The version cross-pin**: doctor's [`BOLTFFI_PINNED`] literal equals `setup:boltffi`'s
//!    `want="…"`.
//!
//! Both extractions are pinned from the other side too (the step-10 vacuous-needle lesson): the
//! task scan must find a known task and a count floor, the `want=` scan exactly one line.

use bolted_check::doctor::{BOLTFFI_PINNED, EXEMPT, ROWS};
use std::collections::BTreeSet;

/// `mise.toml`, compile-time embedded: the test re-runs whenever the file changes, and the
/// comparison needs no path resolution at runtime.
const MISE_TOML: &str = include_str!("../../../mise.toml");

/// Every task name declared in `mise.toml`: the lines shaped `[tasks."name"]` / `[tasks.name]`.
fn declared_tasks() -> BTreeSet<String> {
    MISE_TOML
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let inner = line.strip_prefix("[tasks.")?.strip_suffix(']')?;
            Some(inner.trim_matches('"').to_owned())
        })
        .collect()
}

/// The scan itself, held from both sides so it can never green vacuously: it must see a known
/// task by name and at least the count `mise.toml` had when this was written.
#[test]
fn the_task_scan_is_not_vacuous() {
    let tasks = declared_tasks();
    assert!(
        tasks.contains("test:android"),
        "the scan no longer finds `test:android` — the [tasks.] line shape changed?"
    );
    assert!(
        tasks.len() >= 25,
        "the scan found only {} tasks; mise.toml had 29 when this was written",
        tasks.len()
    );
}

#[test]
fn every_mise_task_is_mapped_to_a_doctor_row_or_exempted_with_a_reason() {
    let declared = declared_tasks();
    let mapped: BTreeSet<&str> = ROWS.iter().flat_map(|r| r.tasks.iter().copied()).collect();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(t, _)| *t).collect();

    let uncovered: Vec<&String> = declared
        .iter()
        .filter(|t| !mapped.contains(t.as_str()) && !exempt.contains(t.as_str()))
        .collect();
    assert!(
        uncovered.is_empty(),
        "mise.toml tasks with no doctor row and no recorded exemption: {uncovered:?} — \
         add a Row (what does the machine need?) or an EXEMPT reason (why nothing?) in \
         crates/bolted-check/src/doctor.rs"
    );
}

#[test]
fn no_doctor_mapping_or_exemption_names_a_task_that_no_longer_exists() {
    let declared = declared_tasks();
    for row in ROWS {
        for task in row.tasks {
            assert!(
                declared.contains(*task),
                "doctor row `{}` maps task `{task}`, which mise.toml no longer declares",
                row.name
            );
        }
    }
    for (task, _) in EXEMPT {
        assert!(
            declared.contains(*task),
            "doctor exemption names task `{task}`, which mise.toml no longer declares"
        );
    }
}

#[test]
fn the_boltffi_pin_matches_setup_boltffi_exactly_and_the_extraction_is_not_vacuous() {
    let wants: Vec<&str> = MISE_TOML
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("want=\"")?;
            rest.split('"').next()
        })
        .collect();
    assert_eq!(
        wants.len(),
        1,
        "expected exactly one `want=\"…\"` line in mise.toml (setup:boltffi's pin), found {}",
        wants.len()
    );
    assert_eq!(
        wants[0], BOLTFFI_PINNED,
        "doctor::BOLTFFI_PINNED ({BOLTFFI_PINNED}) != setup:boltffi's want= ({}) — a version \
         bump must move both, in one commit",
        wants[0]
    );
}
