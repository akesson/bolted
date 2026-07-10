//! Step 13, M0: the observability map and `docs/CONFORMANCE.md` cannot drift apart.
//!
//! `bolted-conformance/tests/manifest.rs` does this for the Rust suite (doc ↔ `cNN_*` functions);
//! this is the same discipline one tier out (doc ↔ `foreign::BOUNDARY_MAP`). The map decides which
//! C-IDs the per-language emitter emits, so a map that silently disagreed with the normative document
//! would emit — or skip — a contract test nobody sanctioned.
//!
//! Four claims:
//!  1. every normative `CNN` in the document has a disposition in `BOUNDARY_MAP`;
//!  2. every id in `BOUNDARY_MAP` is a normative `CNN`;
//!  3. the document's per-language accounting table agrees with `BOUNDARY_MAP` — id present, and
//!     emitted/exempt matching — in both directions;
//!  4. `BOUNDARY_MAP` lists each id once.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use bolted_ffi_gen::foreign::{BOUNDARY_MAP, Boundary};

fn conformance_md() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONFORMANCE.md");
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{path} must be readable: {e}"))
}

/// `"C07"` and the like: a `C` and exactly two digits.
fn is_id(s: &str) -> bool {
    s.len() == 3 && s.starts_with('C') && s[1..].chars().all(|c| c.is_ascii_digit())
}

/// The nth pipe-delimited cell of a table row, trimmed. Cell 1 is the first real column (cell 0 is
/// the empty string before the leading `|`).
fn cell(line: &str, n: usize) -> Option<&str> {
    line.split('|').nth(n).map(str::trim)
}

/// Every normative `CNN` mentioned in a table's first column — the invariant table and the
/// per-language accounting both use `| CNN | … |`, and they name the same ids, so the union is the
/// documented set.
fn documented_ids(doc: &str) -> BTreeSet<String> {
    doc.lines()
        .filter_map(|l| cell(l, 1))
        .filter(|s| is_id(s))
        .map(str::to_owned)
        .collect()
}

/// The accounting rows: `| CNN | emitted | … |` / `| CNN | exempt | … |`, as `id -> "emitted"|"exempt"`.
/// Distinguished from the invariant table purely by the second column being a disposition word — no
/// dependence on where the section starts.
fn accounting(doc: &str) -> BTreeMap<String, String> {
    doc.lines()
        .filter_map(|l| {
            let id = cell(l, 1).filter(|s| is_id(s))?;
            let disposition = cell(l, 2)?;
            (disposition == "emitted" || disposition == "exempt")
                .then(|| (id.to_owned(), disposition.to_owned()))
        })
        .collect()
}

fn map_disposition(b: Boundary) -> &'static str {
    match b {
        Boundary::Emitted => "emitted",
        Boundary::Exempt(_) => "exempt",
    }
}

fn map_ids() -> BTreeSet<String> {
    BOUNDARY_MAP.iter().map(|b| b.id.to_owned()).collect()
}

#[test]
fn every_normative_id_has_a_disposition() {
    let documented = documented_ids(&conformance_md());
    assert!(!documented.is_empty(), "parsed no IDs from CONFORMANCE.md");
    let mapped = map_ids();
    for id in &documented {
        assert!(
            mapped.contains(id),
            "{id} is normative in docs/CONFORMANCE.md but has no entry in foreign::BOUNDARY_MAP"
        );
    }
}

#[test]
fn every_mapped_id_is_normative() {
    let documented = documented_ids(&conformance_md());
    for b in BOUNDARY_MAP {
        assert!(
            documented.contains(b.id),
            "foreign::BOUNDARY_MAP names `{}`, which is not a normative row in docs/CONFORMANCE.md",
            b.id
        );
    }
}

#[test]
fn the_accounting_table_matches_the_map_both_directions() {
    let doc = conformance_md();
    let accounting = accounting(&doc);
    assert!(
        !accounting.is_empty(),
        "parsed no per-language accounting rows from CONFORMANCE.md — the table format changed"
    );

    // doc row -> map
    for (id, disposition) in &accounting {
        let mapped = BOUNDARY_MAP
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("the accounting names `{id}`, absent from BOUNDARY_MAP"));
        assert_eq!(
            map_disposition(mapped.boundary),
            disposition,
            "`{id}`: CONFORMANCE.md says `{disposition}`, BOUNDARY_MAP says `{}`",
            map_disposition(mapped.boundary),
        );
    }

    // map -> doc row
    for b in BOUNDARY_MAP {
        let disposition = accounting
            .get(b.id)
            .unwrap_or_else(|| panic!("`{}` is in BOUNDARY_MAP but has no accounting row", b.id));
        assert_eq!(
            disposition,
            map_disposition(b.boundary),
            "`{}`: BOUNDARY_MAP says `{}`, CONFORMANCE.md says `{disposition}`",
            b.id,
            map_disposition(b.boundary),
        );
    }
}

#[test]
fn the_map_lists_each_id_once() {
    let mut seen = BTreeSet::new();
    for b in BOUNDARY_MAP {
        assert!(
            seen.insert(b.id),
            "`{}` appears twice in BOUNDARY_MAP",
            b.id
        );
    }
    assert_eq!(seen.len(), 23, "expected C01..C23; got {}", seen.len());
}
