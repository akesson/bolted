//! The foreign-language emitter (D28): per-language contract tests and the stash codec, emitted as
//! **committed generated source** and byte-compared inside `mise run check` — D22, one language out.
//!
//! This module lands in slices. Step 13 **M0** seeds it with the *observability map*: the single list
//! of which conformance IDs cross the FFI boundary and which cannot, each exemption with a stated
//! reason. `tests/manifest.rs` ties this list to `docs/CONFORMANCE.md`'s per-language accounting in
//! both directions, so the document and the emitter's intent cannot drift apart — the same discipline
//! `bolted-conformance/tests/manifest.rs` holds over the Rust suite. Later milestones grow the Kotlin
//! and Swift emitters that *consume* this map; the map is what they emit from, so it lives here rather
//! than only in prose.

/// Whether a conformance invariant can be observed through the **public generated surface** — the
/// `#[export]` verbs and `#[data]` DTOs, and nothing internal (kill criterion 2). That is the only
/// thing an emitted per-language test may touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// The surface can both construct the precondition and observe the outcome: the emitter emits a
    /// per-language contract test for this ID.
    Emitted,
    /// The surface cannot. The string is the reason, and it is load-bearing: kill criterion 4 counts
    /// these, and each is a claim the report has to stand behind. An ID that is *observable* but only
    /// lacks a verb is **not** exempt — the generator gains the verb (it is our output) instead.
    Exempt(&'static str),
}

/// One conformance ID's disposition at the per-language tier.
#[derive(Clone, Copy, Debug)]
pub struct BoundaryOf {
    /// The `CNN` id, exactly as it appears in `docs/CONFORMANCE.md`.
    pub id: &'static str,
    pub boundary: Boundary,
}

use Boundary::{Emitted, Exempt};

/// The observability map (step 13, M0). Every normative `CNN` in `docs/CONFORMANCE.md` appears here
/// exactly once, and `docs/CONFORMANCE.md`'s "per-language tier" table mirrors it row for row. The
/// *how* of each observation lives in that table; this list is the machine-checkable disposition.
///
/// 22 emitted, 1 exempt (C10) — inside the "no more than a third exempt" gate by a wide margin.
pub const BOUNDARY_MAP: &[BoundaryOf] = &[
    BoundaryOf {
        id: "C01",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C02",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C03",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C04",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C05",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C06",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C07",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C08",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C09",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C10",
        // The one exemption. "A superseded completion is discarded" presupposes two checks in flight;
        // the generated `run_*_check` driver begins, calls the checker, and completes one token within
        // a single atomic FFI call over one taken checker, so a second token can never exist to be
        // superseded. Driven directly in the Rust tier (`SingleFlight`); emitting it would mean
        // exposing raw single-flight tokens across the FFI — a D18 contract change, not an accessor.
        boundary: Exempt(
            "the superseded-token race needs two checks in flight at once; the atomic single-checker \
             run_*_check driver makes a second token unreachable at the boundary (see CONFORMANCE.md)",
        ),
    },
    BoundaryOf {
        id: "C11",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C12",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C13",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C14",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C15",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C16",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C17",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C18",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C19",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C20",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C21",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C22",
        boundary: Emitted,
    },
    BoundaryOf {
        id: "C23",
        boundary: Emitted,
    },
];

/// The ids the per-language emitter emits a contract test for, in declaration order.
pub fn emitted_ids() -> impl Iterator<Item = &'static str> {
    BOUNDARY_MAP
        .iter()
        .filter(|b| matches!(b.boundary, Boundary::Emitted))
        .map(|b| b.id)
}
