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

// ======================================================================================
// The Kotlin stash codec (D28) — the first foreign artifact on this pipeline.
// ======================================================================================

use crate::field::FieldProj;
use bolted_decl::Feature;
use bolted_decl::naming::upper_camel;

/// A declared `Raw = String` field — the only kind the codec (de)serialises itself. Everything else
/// is a composite (D20/D25) delegated to the hand-written custom object.
fn is_text_field(p: &FieldProj<'_>) -> bool {
    p.value.is_some_and(|v| v.is_text())
}

/// Emit the Kotlin stash codec for `feature`: `<Entity>StashFfi`/`<Entity>Values` ⇄ a JSON string, so
/// a shell can persist an edit session across process death (C20).
///
/// **String-building in plain Rust — no template engine** (D28): a template file would be a second
/// source of truth with no compiler on it. Text fields are (de)serialised here; a composite field is
/// delegated to the hand-written `<Entity>StashCustom` object in the same package, referenced by name
/// so a missing helper is a Kotlin compile error (D25, one language out), never a silent gap.
pub(crate) fn emit_kotlin_stash_codec(
    feature: &Feature,
    binding_pkg: &str,
    codec_pkg: &str,
) -> String {
    let entity = feature.entity.name.to_string();
    let stash_ffi = format!("{entity}StashFfi");
    let values_ffi = format!("{entity}Values");
    let codec = format!("{entity}StashCodec");
    let custom = format!("{entity}StashCustom");

    let fields = FieldProj::all(feature);
    let has_text = fields.iter().any(is_text_field);

    let (mut encode_fields, mut decode_fields) = (String::new(), String::new());
    let (mut encode_values, mut decode_values) = (String::new(), String::new());
    for p in &fields {
        let key = p.ident().to_string();
        let camel = upper_camel(p.ident());
        if is_text_field(p) {
            encode_fields += &format!("            put(\"{key}\", encodeText(stash.{key}))\n");
            decode_fields +=
                &format!("                {key} = decodeText(o.getJSONObject(\"{key}\")),\n");
            encode_values += &format!("            put(\"{key}\", v.{key})\n");
            decode_values += &format!("                {key} = o.getString(\"{key}\"),\n");
        } else {
            encode_fields +=
                &format!("            put(\"{key}\", {custom}.encode{camel}Stash(stash.{key}))\n");
            decode_fields += &format!(
                "                {key} = {custom}.decode{camel}Stash(o.getJSONObject(\"{key}\")),\n"
            );
            encode_values +=
                &format!("            put(\"{key}\", {custom}.encode{camel}Raw(v.{key}))\n");
            decode_values += &format!(
                "                {key} = {custom}.decode{camel}Raw(o.getJSONObject(\"{key}\")),\n"
            );
        }
    }

    let banner = r#"// @generated by bolted-ffi-gen. DO NOT EDIT.
//
// Regenerate with `mise run gen:ffi`. `mise run check` byte-compares this file against the
// declaration it was generated from (D28); a hand-edit fails that drift check, and nothing may
// reformat it — the byte comparison is honest only because no formatter owns a foreign file.
//
// The composite fields the generator cannot serialise are delegated to the hand-written object in
// this package (D25, one language out).
"#;
    let text_import = if has_text {
        format!("import {binding_pkg}.TextFieldStashFfi\n")
    } else {
        String::new()
    };
    let text_helpers = if has_text {
        r#"
    private fun encodeText(s: TextFieldStashFfi): JSONObject =
        JSONObject().apply {
            putOpt("raw", s.raw)
            putOpt("base", s.base)
        }

    private fun decodeText(o: JSONObject): TextFieldStashFfi =
        TextFieldStashFfi(raw = o.optNullableString("raw"), base = o.optNullableString("base"))

    /** `org.json` stores an absent key and a JSON `null` differently; a stash needs `null` back. */
    private fun JSONObject.optNullableString(key: String): String? =
        if (isNull(key)) null else optString(key)
"#
    } else {
        "\n"
    };

    let template = r####"@@BANNER@@
package @@CODEC_PKG@@

import @@BINDING_PKG@@.@@STASH_FFI@@
import @@BINDING_PKG@@.@@VALUES_FFI@@
@@TEXT_IMPORT@@import org.json.JSONObject

/**
 * `@@STASH_FFI@@` / `@@VALUES_FFI@@` ⇄ a JSON string, so an edit session can live in a
 * `SavedStateHandle` and survive process death (C20).
 *
 * Generated by `bolted-ffi-gen` from the `@@ENTITY@@` declaration (D28). Text fields it (de)serialises
 * itself; composite fields (D20/D25) it delegates to the hand-written `@@CUSTOM@@` in this package.
 * Decoding is total: any malformed or structurally-incomplete input yields `null`, and the caller
 * checks out a fresh draft.
 */
object @@CODEC@@ {

    fun encode(stash: @@STASH_FFI@@): String =
        JSONObject().apply {
            put("schema_version", stash.schemaVersion.toLong())
@@ENCODE_FIELDS@@            put("baseVersion", stash.baseVersion.toLong())
            put("orphaned", stash.orphaned)
        }.toString()

    fun decode(json: String): @@STASH_FFI@@? =
        runCatching {
            val o = JSONObject(json)
            @@STASH_FFI@@(
                schemaVersion = o.getLong("schema_version").toUInt(),
@@DECODE_FIELDS@@                baseVersion = o.getLong("baseVersion").toULong(),
                orphaned = o.getBoolean("orphaned"),
            )
        }.getOrNull()

    fun encodeValues(v: @@VALUES_FFI@@): String =
        JSONObject().apply {
@@ENCODE_VALUES@@        }.toString()

    fun decodeValues(json: String): @@VALUES_FFI@@? =
        runCatching {
            val o = JSONObject(json)
            @@VALUES_FFI@@(
@@DECODE_VALUES@@            )
        }.getOrNull()
@@TEXT_HELPERS@@}
"####;

    template
        .replace("@@BANNER@@", banner)
        .replace("@@CODEC_PKG@@", codec_pkg)
        .replace("@@BINDING_PKG@@", binding_pkg)
        .replace("@@STASH_FFI@@", &stash_ffi)
        .replace("@@VALUES_FFI@@", &values_ffi)
        .replace("@@ENTITY@@", &entity)
        .replace("@@CUSTOM@@", &custom)
        .replace("@@CODEC@@", &codec)
        .replace("@@TEXT_IMPORT@@", &text_import)
        .replace("@@ENCODE_FIELDS@@", &encode_fields)
        .replace("@@DECODE_FIELDS@@", &decode_fields)
        .replace("@@ENCODE_VALUES@@", &encode_values)
        .replace("@@DECODE_VALUES@@", &decode_values)
        .replace("@@TEXT_HELPERS@@", text_helpers)
}
