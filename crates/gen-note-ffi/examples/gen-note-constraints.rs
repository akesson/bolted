//! Regenerates `crates/gen-note/constraints.snap` — the committed constraint-surface snapshot
//! (step 16, D28 artifact; byte-checked by `mise run check`). Run **only** via `mise run gen:ffi`,
//! never by the drift test: a verb that rewrites the file it verifies cannot verify it (step 10).
//!
//! It lives here, in the `-ffi` crate, rather than in `bolted-check`, because building the snapshot
//! needs two things only linked here — the feature's runtime `FieldId::constraints()` and its
//! `STASH_SCHEMA_VERSION`. `bolted-check` itself stays a pure function of the parsed declaration
//! (step-16 kill criterion 2); this generator hands it the runtime half.

use bolted_check::{RuntimeField, RuntimeSurface, render_constraint_snapshot};
use bolted_decl::Feature;
use gen_note::NoteField;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: gen-constraints <out-path>");
    let source = include_str!("../../gen-note/src/lib.rs");
    let feature = Feature::from_file(&syn::parse_file(source).expect("gen-note source parses"))
        .expect("gen-note scans");
    let runtime = RuntimeSurface {
        schema_version: gen_note_ffi::STASH_SCHEMA_VERSION,
        fields: vec![
            RuntimeField::new("title", NoteField::Title.constraints()),
            RuntimeField::new("body", NoteField::Body.constraints()),
        ],
    };
    let snapshot = render_constraint_snapshot("gen-note", &feature, &runtime)
        .expect("runtime surface covers the declaration");
    std::fs::write(&out, snapshot).expect("write constraints.snap");
}
