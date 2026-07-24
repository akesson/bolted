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
use syn::Ident;

/// A declared `Raw = String` field — the only kind the codec (de)serialises itself. Everything else
/// is a composite (D20/D25) delegated to the hand-written custom object.
fn is_text_field(p: &FieldProj<'_>) -> bool {
    p.value.is_some_and(|v| v.is_text())
}

/// `username` → `username`, `display_name` → `displayName`. The Kotlin/Swift binding spells a verb or
/// property in lower camel; bindgen derives it from the same snake ident this reads.
fn lower_camel(ident: &Ident) -> String {
    let camel = upper_camel(ident).to_string();
    let mut chars = camel.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => camel,
    }
}

/// `username` → `USERNAME`, `display_name` → `DISPLAY_NAME`. bindgen screams a fieldless-enum variant;
/// the snake ident already carries the word boundaries, so uppercasing it matches.
fn screaming(ident: &Ident) -> String {
    ident.to_string().to_uppercase()
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

// ======================================================================================
// The Kotlin contract suite (D28) — every emitted C-ID projected through the public surface.
// ======================================================================================

/// The Kotlin names a text-field role resolves to on the binding surface.
struct Role {
    /// `trySetName`
    setter: String,
    /// `name` — the snapshot/stash property, and the `copy()` argument.
    prop: String,
    /// `ProfileFieldId.NAME`
    id: String,
    /// `PersonNameErrorFfi` — the setter's typed refusal.
    error: String,
}

impl Role {
    fn of(p: &FieldProj<'_>, field_id_ty: &str) -> Role {
        let camel = upper_camel(p.ident());
        let value = p
            .field
            .value_ident()
            .map(|i| i.to_string())
            .unwrap_or_default();
        Role {
            setter: format!("trySet{camel}"),
            prop: lower_camel(p.ident()),
            id: format!("{field_id_ty}.{}", screaming(p.ident())),
            error: format!("{value}ErrorFfi"),
        }
    }
}

/// The extra names the async-checked role resolves to (present only when a field carries `#[check]`).
struct Checked {
    setter: String,
    prop: String,
    id: String,
    check_prop: String,
    checker_ty: String,
    checker_run: String,
    /// The l10n key C16 raises — a *declaration* fact (`#[check(required_key = ..)]`), emitted literal.
    required_key: String,
    /// The rule name the check reports under — likewise from the declaration.
    rule_name: String,
}

/// Emit the Kotlin per-language contract suite for `feature`: one test per emitted C-ID, generic over
/// a hand-written **values-only** fixture (deliverable 4). The emitter assigns roles deterministically
/// from the declaration — *checked* is the field carrying `#[check]`, *primary*/*secondary* are the
/// other text fields in declaration order — and emits concrete verb calls, so the fixture never makes
/// a judgement (kill criterion 3): it holds example values and nothing else.
///
/// String-building in plain Rust, no template engine (D28). The Kotlin binding is produced by BoltFFI's
/// bindgen from the same `#[export]` surface, so a method this suite names that the wrapper does not
/// emit is a Kotlin compile error at `test:android` — never a silent gap.
pub(crate) fn emit_kotlin_contract_suite(
    feature: &Feature,
    binding_pkg: &str,
    suite_pkg: &str,
) -> String {
    let entity = feature.entity.name.to_string();
    let store = format!("{entity}StoreFfi");
    let draft = format!("{entity}DraftFfi");
    let values = format!("{entity}Values");
    let field_id = format!("{entity}FieldId");
    let stash = format!("{entity}StashFfi");
    let fixture_ty = format!("{entity}ConformanceFixture");
    let suite_ty = format!("{entity}ConformanceSuite");
    let factory = format!("{}ConformanceFixture", lower_camel(&feature.entity.name));

    let fields = FieldProj::all(feature);
    let text_fields: Vec<&FieldProj<'_>> = fields.iter().filter(|p| is_text_field(p)).collect();
    let checked_field = text_fields
        .iter()
        .copied()
        .find(|p| p.field.check.is_some());
    let plain: Vec<&FieldProj<'_>> = text_fields
        .iter()
        .copied()
        .filter(|p| p.field.check.is_none())
        .collect();

    // Roles. A feature needs two editable text fields the suite can play primary/secondary against; a
    // draft with fewer is degenerate. Guarded (not indexed) so the emitter never panics (rung 1).
    let (Some(primary), Some(secondary)) = (plain.first().copied(), plain.get(1).copied()) else {
        return format!(
            "// @generated by bolted-ffi-gen. DO NOT EDIT.\n// {entity} has fewer than two editable \
             text fields; the contract suite needs two.\n"
        );
    };
    let primary = Role::of(primary, &field_id);
    let secondary = Role::of(secondary, &field_id);

    let checked = checked_field.and_then(|p| {
        let c = p.field.check.as_ref()?;
        let camel = upper_camel(p.ident());
        Some(Checked {
            setter: format!("trySet{camel}"),
            prop: lower_camel(p.ident()),
            id: format!("{field_id}.{}", screaming(p.ident())),
            check_prop: format!("{}Check", lower_camel(p.ident())),
            checker_ty: format!("{camel}Checker"),
            checker_run: format!("run{camel}Check"),
            required_key: c.required_key.clone(),
            rule_name: c.rule.clone(),
        })
    });

    // D34: the capability is a checkout/restore argument. Three per-feature argument spellings —
    // declared absence, the passing checker, and restore's trailing absence — empty when the
    // feature declares no check, so a check-less feature's call sites do not move.
    let co_none = if checked.is_some() { "null" } else { "" };
    let co_cap = if checked.is_some() {
        "passingChecker()"
    } else {
        ""
    };
    let rs_none = if checked.is_some() { ", null" } else { "" };

    // --- helpers (dispatch + fill), built from the field list with real names ---------------------
    let mut set_text_branches = String::new();
    for p in &text_fields {
        set_text_branches += &format!(
            "            {field_id}.{} -> draft.trySet{}(raw)\n",
            screaming(p.ident()),
            upper_camel(p.ident()),
        );
    }
    let mut fill_lines = String::new();
    for p in &fields {
        fill_lines += &format!(
            "        draft.trySet{}(fixture.seed().{})\n",
            upper_camel(p.ident()),
            lower_camel(p.ident()),
        );
    }
    let (passing_checker, fill_check) = match &checked {
        Some(c) => (
            format!(
                "    private fun passingChecker(): {ty} =\n        object : {ty} {{\n            \
                 override fun check(value: String): CheckVerdictFfi = CheckVerdictFfi.PASS\n        }}\n\n",
                ty = c.checker_ty,
            ),
            format!(
                "        // a create-flow checked field is dirty, so C16 demands its check has run\n        \
                 // (the checker itself arrived at checkout — D34)\n        check(draft.{}())\n",
                c.checker_run,
            ),
        ),
        None => (String::new(), String::new()),
    };

    let mut helpers = String::new();
    helpers += &format!(
        "    private fun seeded(): {store} = {store}.new().also {{ it.applyCanonical(fixture.seed()) }}\n\n"
    );
    helpers += &passing_checker;
    helpers +=
        "    /** Dispatch a raw to the right text setter — mechanical, from the field list. */\n";
    helpers +=
        &format!("    private fun setText(draft: {draft}, id: {field_id}, raw: String) {{\n");
    helpers += "        when (id) {\n";
    helpers += &set_text_branches;
    helpers += "            else -> throw IllegalArgumentException(\"not a text field: $id\")\n";
    helpers += "        }\n    }\n\n";
    helpers += "    /** Leave a create-flow draft committable: every field valid, any demanded check satisfied. */\n";
    helpers +=
        &format!("    private fun fillValid(draft: {draft}) {{\n{fill_lines}{fill_check}    }}\n");

    // --- the emitted body: fixture interface + RuleFlip + the suite ------------------------------
    let mut out = String::new();
    out.push_str(SUITE_BANNER);
    out.push_str("\npackage @@SUITE_PKG@@\n\nimport @@BINDING_PKG@@.*\n");
    out.push_str(
        "import org.junit.Assert.assertEquals\nimport org.junit.Assert.assertFalse\n\
         import org.junit.Assert.assertNull\nimport org.junit.Assert.assertTrue\n\
         import org.junit.Assert.fail\nimport org.junit.Assume\nimport org.junit.Test\n\n",
    );
    out.push_str(FIXTURE_INTERFACE);
    out.push_str(RULE_FLIP);
    out.push_str("class @@SUITE@@ {\n\n    private val fixture: @@FIXTURE@@ = @@FACTORY@@()\n\n");
    out.push_str(&helpers);
    out.push('\n');
    out.push_str(SANITY_TEST);
    out.push_str(CORE_TESTS);
    if checked.is_some() {
        out.push_str(CHECKED_TESTS);
    }
    out.push_str("}\n");

    // --- resolve markers -------------------------------------------------------------------------
    let fixture_checked = match &checked {
        Some(_) => {
            "\n    /** The CHECKED text field's values: the one an async single-flight check guards. */\n    val checkedBase: String\n    val checkedMine: String\n    val checkedTheirs: String\n"
        }
        None => "",
    };
    let seed_checked = match &checked {
        Some(_) => "        assertEquals(fixture.checkedBase, fixture.seed().@@C_PROP@@)\n",
        None => "",
    };
    let c20_check = match &checked {
        Some(_) => {
            "\n                // a restored checked field is Unchecked: no verdict survives the stash (C20)\n                assertEquals(CheckStateFfi.Unchecked, snap.@@C_CHECK@@)"
        }
        None => "",
    };

    let mut s = out
        .replace("@@SUITE_PKG@@", suite_pkg)
        .replace("@@BINDING_PKG@@", binding_pkg)
        .replace("@@CO_NONE@@", co_none)
        .replace("@@CO_CAP@@", co_cap)
        .replace("@@RS_NONE@@", rs_none)
        .replace("@@FIXTURE_CHECKED@@", fixture_checked)
        .replace("@@SEED_CHECKED@@", seed_checked)
        .replace("@@C20_CHECK@@", c20_check)
        .replace("@@STORE@@", &store)
        .replace("@@DRAFT@@", &draft)
        .replace("@@VALUES@@", &values)
        .replace("@@STASH@@", &stash)
        .replace("@@FIELD_ID@@", &field_id)
        .replace("@@FIXTURE@@", &fixture_ty)
        .replace("@@SUITE@@", &suite_ty)
        .replace("@@FACTORY@@", &factory)
        .replace("@@ENTITY@@", &entity)
        .replace("@@P_SET@@", &primary.setter)
        .replace("@@P_PROP@@", &primary.prop)
        .replace("@@P_ID@@", &primary.id)
        .replace("@@P_ERR@@", &primary.error)
        .replace("@@S_SET@@", &secondary.setter)
        .replace("@@S_PROP@@", &secondary.prop)
        .replace("@@S_ID@@", &secondary.id)
        .replace("@@S_ERR@@", &secondary.error);

    if let Some(c) = &checked {
        s = s
            .replace("@@C_SET@@", &c.setter)
            .replace("@@C_PROP@@", &c.prop)
            .replace("@@C_ID@@", &c.id)
            .replace("@@C_CHECKERRUN@@", &c.checker_run)
            .replace("@@C_CHECK@@", &c.check_prop)
            .replace("@@C_REQKEY@@", &c.required_key)
            .replace("@@C_RULE@@", &c.rule_name);
    }
    s
}

const SUITE_BANNER: &str = r#"// @generated by bolted-ffi-gen. DO NOT EDIT.
//
// Regenerate with `mise run gen:ffi`. `mise run check` byte-compares this file against the
// declaration it was generated from (D28); a hand-edit fails that drift check, and nothing may
// reformat it — the byte comparison is honest only because no formatter owns a foreign file.
//
// The per-language contract tests (step 13): every conformance ID the public generated surface can
// express (docs/CONFORMANCE.md), each generic over the hand-written, values-only fixture beside this
// file. What this verifies is the BOUNDARY — that the binding and wrapper preserve the core's
// semantics across JNI — not the algebra, which the Rust suite already proves against four features.
// A failing test here names a binding or wrapper bug, never the core's.
"#;

const FIXTURE_INTERFACE: &str = r#"/**
 * Everything the emitted suite needs that the declaration cannot know: example values. The suite emits
 * the field-specific verb calls itself; this supplies a valid raw, a distinct second, and a raw that
 * fails tier-1 — never a judgement (kill criterion 3). Hand-written, one impl per feature per language.
 */
interface @@FIXTURE@@ {
    /** A fully-valid canonical entity to seed every store from. */
    fun seed(): @@VALUES@@

    /** The PRIMARY text field's values — the one the suite edits. `base` is its value in [seed]. */
    val primaryBase: String
    val primaryMine: String
    val primaryTheirs: String
    val primaryOther: String
    val primaryInvalid: String

    /** The SECONDARY text field's values — the one the suite moves on the server (C19). */
    val secondaryBase: String
    val secondaryTheirs: String
    val secondaryInvalid: String
@@FIXTURE_CHECKED@@
    /**
     * C08's tier-2 rule arrangement, as values — or null if this feature declares no `#[bolted::rules]`
     * rule (the Rust `RuleFeature` bound, one language out). The declaration never sees a rule body, so
     * unlike a `#[check]` its name and pins cannot be projected; the fixture supplies them.
     */
    fun ruleFlip(): RuleFlip?
}

"#;

const RULE_FLIP: &str = r#"/**
 * C08 as data. [dirtyEdits] are applied to a draft checked out from [@@FIXTURE@@.seed], leaving the
 * rule satisfied; [flippedCanonical] is a canonical whose rebase moves an *unpinned* field so the rule
 * fires, pinning [pins]. No branching, no judgement — the relationship lives in human-chosen constants.
 */
class RuleFlip(
    val ruleName: String,
    val dirtyEdits: List<Pair<@@FIELD_ID@@, String>>,
    val flippedCanonical: @@VALUES@@,
    val pins: List<@@FIELD_ID@@>,
)

"#;

const SANITY_TEST: &str = r#"    /** The fixture's constants must describe the seed it returns, or every test below is built on sand. */
    @Test
    fun theFixtureDescribesItsSeed() {
        assertEquals(fixture.primaryBase, fixture.seed().@@P_PROP@@)
        assertEquals(fixture.secondaryBase, fixture.seed().@@S_PROP@@)
@@SEED_CHECKED@@    }

"#;

const CORE_TESTS: &str = r#"    /** C01 — holding a value loses no validity; the canonical raw re-parses to the same value. */
    @Test
    fun c01_roundtripHoldsValidity() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            val v = draft.snapshot().@@P_PROP@@.validity
            assertTrue("a set valid raw reads back Valid{value}", v is TextValidity.Valid && v.value == fixture.primaryMine)
            draft.@@P_SET@@((v as TextValidity.Valid).value) // idempotent
            val v2 = draft.snapshot().@@P_PROP@@.validity
            assertTrue(v2 is TextValidity.Valid && v2.value == fixture.primaryMine)
        } }
    }

    /** C02 — a clean field adopts theirs on rebase and stays InSync. */
    @Test
    fun c02_aCleanFieldFollowsCanonical() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            val f = draft.snapshot().@@P_PROP@@
            val v = f.validity
            assertTrue(v is TextValidity.Valid && v.value == fixture.primaryTheirs)
            assertTrue(f.sync is TextFieldSync.InSync)
            assertFalse(f.dirty)
        } }
    }

    /** C03 — a dirty field whose canonical moved is never overwritten: it conflicts, naming theirs. */
    @Test
    fun c03_aDirtyFieldIsNeverSilentlyOverwritten() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            val snap = draft.snapshot()
            val v = snap.@@P_PROP@@.validity
            assertTrue("your value survives", v is TextValidity.Valid && v.value == fixture.primaryMine)
            val sync = snap.@@P_PROP@@.sync
            assertTrue(sync is TextFieldSync.Conflicted)
            assertEquals(fixture.primaryTheirs, (sync as TextFieldSync.Conflicted).theirs)
            assertEquals("the recorded ancestor did not move", fixture.primaryBase, sync.base)
            assertEquals(listOf(@@P_ID@@), snap.conflicts)
        } }
    }

    /** C04 — a dirty field whose value already equals theirs rebases clean. */
    @Test
    fun c04_convergentRebaseIsClean() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryMine))
            val f = draft.snapshot().@@P_PROP@@
            assertTrue(f.sync is TextFieldSync.InSync)
            assertFalse("two edits that agree are not a conflict", f.dirty)
        } }
    }

    /** C05 — setting a field back to its base clears dirty; dirtiness is value-based. */
    @Test
    fun c05_revertForFree() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            assertTrue(draft.snapshot().@@P_PROP@@.dirty)
            draft.@@P_SET@@(fixture.primaryBase)
            assertFalse("dirty is a function of the data, not of touch history", draft.snapshot().@@P_PROP@@.dirty)
        } }
    }

    /** C06 — a failed try_set is typed, records Invalid{raw}, blocks submit, and never commits the stale value. */
    @Test
    fun c06_noStaleValueSubmit() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            var rejected = false
            try { draft.@@P_SET@@(fixture.primaryInvalid) } catch (e: @@P_ERR@@) { rejected = true }
            assertTrue("an invalid raw is refused, typed", rejected)
            val v = draft.snapshot().@@P_PROP@@.validity
            assertTrue("and recorded as Invalid{raw}", v is TextValidity.Invalid && v.raw == fixture.primaryInvalid)
            try {
                draft.submit()
                fail("an invalid field must block submit")
            } catch (e: SubmitErrorFfi.Validation) {
                assertTrue(e.report.fieldErrors.any { it.field == @@P_ID@@ })
            }
            val canon = store.canonical()?.@@P_PROP@@?.validity
            assertTrue("the previous valid value was NOT silently committed", canon is TextValidity.Valid && canon.value == fixture.primaryBase)
        } }
    }

    /** C07 — precedence: a deleted canonical outranks a conflict (Orphaned wins). */
    @Test
    fun c07_orphanedOutranksConflicted() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            assertTrue(draft.snapshot().@@P_PROP@@.sync is TextFieldSync.Conflicted)
            store.deleteCanonical() // the conflict survives the orphaning, or this proves nothing
            try {
                draft.submit()
                fail("orphaned outranks conflicted")
            } catch (e: SubmitErrorFfi.Orphaned) { /* expected */ }
        } }
    }

    /** C07 — precedence: a conflict outranks a validation error (Conflicted wins). */
    @Test
    fun c07_conflictedOutranksValidation() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // conflict on primary
            try { draft.@@S_SET@@(fixture.secondaryInvalid) } catch (e: @@S_ERR@@) { /* invalid secondary */ }
            try {
                draft.submit()
                fail("conflicted outranks validation")
            } catch (e: SubmitErrorFfi.Conflicted) {
                assertEquals(listOf(@@P_ID@@), e.fields)
            }
        } }
    }

    /** C08 — a rebase re-runs tier-2: moving an unpinned field can flip a rule pinned to a field it did not touch. */
    @Test
    fun c08_rebaseRerunsTier2() {
        val flip = fixture.ruleFlip()
        Assume.assumeTrue("this feature declares no tier-2 rule", flip != null)
        val f = flip!!
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            f.dirtyEdits.forEach { (id, raw) -> setText(draft, id, raw) }
            assertTrue("the arrangement must leave the rule satisfied", draft.validate().ruleErrors.none { it.rule == f.ruleName })
            store.applyCanonical(f.flippedCanonical)
            val report = draft.validate()
            val violation = report.ruleErrors.firstOrNull { it.rule == f.ruleName }
            assertTrue("the rebase must make the rule fire", violation != null)
            assertEquals(f.pins, violation!!.pins)
            assertTrue("a pinned field whose own canonical did not move is not conflicted (C19)", draft.snapshot().conflicts.isEmpty())
        } }
    }

    /** C09 — resolve_keep_mine: value stays yours, base becomes theirs, dirty, InSync. */
    @Test
    fun c09_resolveKeepMine() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            assertTrue(draft.snapshot().@@P_PROP@@.sync is TextFieldSync.Conflicted)
            draft.resolveKeepMine(@@P_ID@@)
            val snap = draft.snapshot()
            val v = snap.@@P_PROP@@.validity
            assertTrue("value stays mine", v is TextValidity.Valid && v.value == fixture.primaryMine)
            assertTrue("and returns to InSync", snap.@@P_PROP@@.sync is TextFieldSync.InSync)
            assertTrue("still dirty", snap.@@P_PROP@@.dirty)
            assertEquals("base became theirs", fixture.primaryTheirs, draft.stash().@@P_PROP@@.base)
        } }
    }

    /** C09 — resolve_take_theirs: value and base become theirs, clean, InSync. */
    @Test
    fun c09_resolveTakeTheirs() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            draft.resolveTakeTheirs(@@P_ID@@)
            val snap = draft.snapshot()
            val v = snap.@@P_PROP@@.validity
            assertTrue("value becomes theirs", v is TextValidity.Valid && v.value == fixture.primaryTheirs)
            assertTrue(snap.@@P_PROP@@.sync is TextFieldSync.InSync)
            assertFalse("clean", snap.@@P_PROP@@.dirty)
            assertEquals(fixture.primaryTheirs, draft.stash().@@P_PROP@@.base)
        } }
    }

    /** C11 — deleting the canonical under a live draft orphans it; submit is a typed Orphaned; the draft stays live. */
    @Test
    fun c11_deletionOrphans() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            store.deleteCanonical()
            assertEquals(DraftStatusFfi.ORPHANED, draft.snapshot().status)
            assertTrue("the refusal hands the draft back", draft.isLive())
            try {
                draft.submit()
                fail("submitting an orphan is a typed outcome, never a silent failure or a resurrection")
            } catch (e: SubmitErrorFfi.Orphaned) { /* expected */ }
            assertTrue(draft.isLive())
        } }
    }

    /** C12 — a create-flow draft (no base) is never in the fan-out, and commits normally once filled. */
    @Test
    fun c12_createFlowNeverRebases() {
        @@STORE@@.new().use { store -> // empty: no canonical
            store.checkout(@@CO_CAP@@).use { draft ->
                store.applyCanonical(fixture.seed())
                assertEquals("a create-flow draft is not rebased", 0u, store.rebasingDraftCount())
                assertTrue("its primary stays unset", draft.snapshot().@@P_PROP@@.validity is TextValidity.Unset)
                assertFalse(draft.snapshot().anyDirty)
                fillValid(draft)
                draft.submit() // must not throw
            }
        }
    }

    /** C12 — the contrapositive: a draft that keeps an ancestor in ANY field is entity-backed (it rebases, it orphans). */
    @Test
    fun c12_aPartiallyStashedDraftIsStillEntityBacked() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.stash()
        } }
        val partial = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = null)) // forget the secondary's ancestor
        seeded().use { store ->
            store.restore(store.acceptStash(partial)@@RS_NONE@@).use { _ ->
                assertEquals("one surviving ancestor still means entity-backed", 1u, store.rebasingDraftCount())
            }
        }
        @@STORE@@.new().use { empty -> // ...and it orphans into a deleted canonical, not commit as new
            empty.restore(empty.acceptStash(partial)@@RS_NONE@@).use { restored ->
                assertEquals(DraftStatusFfi.ORPHANED, restored.snapshot().status)
            }
        }
    }

    /** C14 — editing a conflicted field to theirs auto-converges (C04 with the events in the other order). */
    @Test
    fun c14_editingAConflictedFieldToTheirsAutoConverges() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            assertTrue(draft.snapshot().@@P_PROP@@.sync is TextFieldSync.Conflicted)
            draft.@@P_SET@@(fixture.primaryTheirs) // type their value
            val snap = draft.snapshot()
            assertTrue("editing to theirs must clear the conflict", snap.@@P_PROP@@.sync is TextFieldSync.InSync)
            assertFalse(snap.@@P_PROP@@.dirty)
            assertTrue(snap.conflicts.isEmpty())
        } }
    }

    /** C15 — base_version tracks the rebase; an orphan's stamp stops moving. */
    @Test
    fun c15_theBaseVersionTracksTheRebase() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            val atCheckout = draft.snapshot().version
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            val afterRebase = draft.snapshot().version
            assertTrue("the stamp must advance on rebase", afterRebase > atCheckout)
            store.deleteCanonical()
            assertEquals("an orphan is based on no canonical; its stamp stops", afterRebase, draft.snapshot().version)
        } }
    }

    /** C17 — a successful submit tombstones the draft; a second is AlreadySubmitted. */
    @Test
    fun c17_aSuccessfulSubmitReleasesTheDraft() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            assertTrue(draft.isLive())
            draft.submit()
            assertFalse("a successful submit tombstones the handle", draft.isLive())
            try {
                draft.submit()
                fail("a second submit is AlreadySubmitted")
            } catch (e: SubmitErrorFfi.AlreadySubmitted) { /* expected */ }
        } }
    }

    /** C17 — a refused submit leaves the draft live and its edit intact, under the same id. */
    @Test
    fun c17_aRefusedSubmitLeavesTheDraftLive() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            try {
                draft.submit()
                fail("a conflict must refuse")
            } catch (e: SubmitErrorFfi.Conflicted) { /* expected */ }
            assertTrue("a refused submit must not consume the draft", draft.isLive())
            val v = draft.snapshot().@@P_PROP@@.validity
            assertTrue(v is TextValidity.Valid && v.value == fixture.primaryMine)
        } }
    }

    /** C18 — close frees the draft, is idempotent, and stops the store rebasing it. */
    @Test
    fun c18_closeFreesIsIdempotentAndStopsRebase() {
        seeded().use { store ->
            val draft = store.checkout(@@CO_NONE@@)
            assertEquals(1u, store.liveDraftCount())
            draft.close()
            assertEquals("close frees the draft", 0u, store.liveDraftCount())
            assertEquals(0u, store.rebasingDraftCount())
            draft.close()
            draft.close() // idempotent, even on an id already gone (guarded by the generated AtomicBoolean)
            assertEquals("close is idempotent", 0u, store.liveDraftCount())
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // a closed draft is not rebased
            assertEquals(0u, store.liveDraftCount())
        }
    }

    /** C19 — a dirty field whose OWN canonical never moved is not conflicted by a rebase of another field. */
    @Test
    fun c19_aDirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@S_PROP@@ = fixture.secondaryTheirs)) // secondary, and only secondary
            val snap = draft.snapshot()
            assertTrue("the primary's canonical never moved", snap.conflicts.isEmpty())
            assertTrue(snap.@@P_PROP@@.sync is TextFieldSync.InSync)
            assertTrue("my edit survives", snap.@@P_PROP@@.dirty)
            val v = snap.@@P_PROP@@.validity
            assertTrue(v is TextValidity.Valid && v.value == fixture.primaryMine)
            val sv = snap.@@S_PROP@@.validity // the clean secondary adopted theirs (C02)
            assertTrue(sv is TextValidity.Valid && sv.value == fixture.secondaryTheirs)
        } }
    }

    /** C20 — a draft stashes each field's raw + ancestor (no sync/verdict, a structural fact) and restores them. */
    @Test
    fun c20_aDraftStashesAndRestores() {
        seeded().use { store ->
            val stash = store.checkout(@@CO_NONE@@).use { draft ->
                draft.@@P_SET@@(fixture.primaryMine)
                try { draft.@@S_SET@@(fixture.secondaryInvalid) } catch (e: @@S_ERR@@) { /* records Invalid{raw} */ }
                draft.stash()
            }
            // TextFieldStashFfi carries only raw + base — "no sync" is a compile-time fact of the type.
            assertEquals(fixture.primaryMine, stash.@@P_PROP@@.raw)
            assertEquals(fixture.primaryBase, stash.@@P_PROP@@.base)
            assertEquals(fixture.secondaryInvalid, stash.@@S_PROP@@.raw)
            store.restore(store.acceptStash(stash)@@RS_NONE@@).use { restored ->
                val snap = restored.snapshot()
                val pv = snap.@@P_PROP@@.validity
                assertTrue(pv is TextValidity.Valid && pv.value == fixture.primaryMine)
                assertTrue(snap.@@P_PROP@@.dirty)
                val sv = snap.@@S_PROP@@.validity
                assertTrue("an Invalid{raw} survives process death", sv is TextValidity.Invalid && sv.raw == fixture.secondaryInvalid)
                assertTrue(snap.@@S_PROP@@.dirty)@@C20_CHECK@@
            }
        }
    }

    /** C21 — restore conflicts exactly the fields whose canonical moved while away; the others stay dirty · InSync. */
    @Test
    fun c21_restoreConflictsOnlyTheFieldsWhoseCanonicalMoved() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        @@STORE@@.new().use { fresh ->
            fresh.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // server moved the primary only
            fresh.restore(fresh.acceptStash(stash)@@RS_NONE@@).use { restored ->
                val snap = restored.snapshot()
                assertEquals(listOf(@@P_ID@@), snap.conflicts)
                val sync = snap.@@P_PROP@@.sync
                assertTrue(sync is TextFieldSync.Conflicted)
                assertEquals("a restored conflict names the CURRENT canonical", fixture.primaryTheirs, (sync as TextFieldSync.Conflicted).theirs)
                val pv = snap.@@P_PROP@@.validity
                assertTrue(pv is TextValidity.Valid && pv.value == fixture.primaryMine)
                assertTrue("the secondary was untouched by the server: dirty, not conflicted", snap.@@S_PROP@@.dirty)
                assertTrue(snap.@@S_PROP@@.sync is TextFieldSync.InSync)
            }
        }
    }

    /** C21 — restoring into a deleted canonical orphans the draft; it does not resurrect the entity. */
    @Test
    fun c21_restoreIntoADeletedCanonicalOrphans() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.stash()
        } }
        @@STORE@@.new().use { empty -> // the server 404s
            empty.restore(empty.acceptStash(stash)@@RS_NONE@@).use { restored ->
                assertEquals(DraftStatusFfi.ORPHANED, restored.snapshot().status)
                try {
                    restored.submit()
                    fail("expected SubmitErrorFfi.Orphaned")
                } catch (e: SubmitErrorFfi.Orphaned) { /* expected */ }
            }
        }
    }

    /** C21 — a resolution taken before the death survives it: its effect lives in the stashed ancestor. */
    @Test
    fun c21_aResolutionSurvivesTheRestore() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            draft.resolveKeepMine(@@P_ID@@) // base := theirs
            draft.stash()
        } }
        @@STORE@@.new().use { fresh ->
            fresh.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // server still says theirs
            fresh.restore(fresh.acceptStash(stash)@@RS_NONE@@).use { restored ->
                val snap = restored.snapshot()
                assertTrue("the user already resolved this; it must not be re-litigated", snap.conflicts.isEmpty())
                val v = snap.@@P_PROP@@.validity
                assertTrue(v is TextValidity.Valid && v.value == fixture.primaryMine)
                assertTrue(snap.@@P_PROP@@.dirty)
            }
        }
    }

    /** C22 — "a draft exists" and "a draft rebases" are different questions; a create-flow draft and an orphan diverge them. */
    @Test
    fun c22_draftCountAndRebasingDraftCountAreDifferentQuestions() {
        @@STORE@@.new().use { empty ->
            empty.checkout(@@CO_NONE@@).use { _ ->
                assertEquals("a create-flow draft exists", 1u, empty.liveDraftCount())
                assertEquals("and is never rebased (C12)", 0u, empty.rebasingDraftCount())
                empty.applyCanonical(fixture.seed())
                empty.checkout(@@CO_NONE@@).use { _ ->
                    assertEquals("an entity-backed checkout is both", 2u, empty.liveDraftCount())
                    assertEquals(1u, empty.rebasingDraftCount())
                    empty.deleteCanonical() // orphan the entity-backed one
                    assertEquals("an orphan still exists (C11)", 2u, empty.liveDraftCount())
                    assertEquals("but is never rebased", 0u, empty.rebasingDraftCount())
                }
            }
        }
    }

    /** C23 — a stashed ancestor a tightened constraint invalidated degrades to dirty-from-unset, and conflicts on rebase. */
    @Test
    fun c23_aDegradedAncestorRestoresDirtyAndConflicts() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        val tightened = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = fixture.secondaryInvalid)) // ancestor no longer parses
        @@STORE@@.new().use { store ->
            store.applyCanonical(fixture.seed()) // canonical secondary == secondaryBase, differs from the rescued value
            store.restore(store.acceptStash(tightened)@@RS_NONE@@).use { restored ->
                val snap = restored.snapshot()
                assertTrue("the rescued value survives, dirty", snap.@@S_PROP@@.dirty)
                val sync = snap.@@S_PROP@@.sync
                assertTrue("a lost ancestor conflicts, it does not overwrite (C03)", sync is TextFieldSync.Conflicted)
                assertEquals(fixture.secondaryBase, (sync as TextFieldSync.Conflicted).theirs)
                assertNull("no ancestor is fabricated", sync.base)
            }
        }
    }

    /** C23 — ...and the convergence guard: a lost ancestor whose rescued value equals canonical lands clean (C04). */
    @Test
    fun c23_aDegradedAncestorConvergesClean() {
        val stash = seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        val tightened = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = fixture.secondaryInvalid))
        @@STORE@@.new().use { store ->
            store.applyCanonical(fixture.seed().copy(@@S_PROP@@ = fixture.secondaryTheirs)) // canonical == the rescued value
            store.restore(store.acceptStash(tightened)@@RS_NONE@@).use { restored ->
                val snap = restored.snapshot()
                assertTrue("a lost ancestor that converges lands clean, not a conflict from nothing", snap.@@S_PROP@@.sync is TextFieldSync.InSync)
                assertFalse(snap.@@S_PROP@@.dirty)
            }
        }
    }

"#;

const CHECKED_TESTS: &str = r#"    /** C13 — a value-moving edit resets the async verdict; a verdict endorses a value, so a changed value un-endorses it. */
    @Test
    fun c13_aValueMovingEditResetsTheVerdict() {
        seeded().use { store -> store.checkout(@@CO_CAP@@).use { draft ->
            draft.@@C_SET@@(fixture.checkedMine)
            assertTrue(draft.@@C_CHECKERRUN@@())
            assertEquals(CheckStateFfi.Passed, draft.snapshot().@@C_CHECK@@)
            draft.@@C_SET@@(fixture.checkedTheirs) // a different value
            assertEquals("a changed value un-endorses", CheckStateFfi.Unchecked, draft.snapshot().@@C_CHECK@@)
        } }
    }

    /** C13 — a value-preserving edit (edit-to-same) leaves the verdict standing. */
    @Test
    fun c13_aValuePreservingEditLeavesTheVerdictStanding() {
        seeded().use { store -> store.checkout(@@CO_CAP@@).use { draft ->
            draft.@@C_SET@@(fixture.checkedMine)
            assertTrue(draft.@@C_CHECKERRUN@@())
            draft.@@C_SET@@(fixture.checkedMine) // edit to the SAME value
            assertEquals("value-based, like dirty", CheckStateFfi.Passed, draft.snapshot().@@C_CHECK@@)
        } }
    }

    /** C13 — a preserved conflict leaves the verdict standing; resolving take-theirs moves the value and resets it. */
    @Test
    fun c13_takeTheirsMovesTheValueAndResetsTheVerdict() {
        seeded().use { store -> store.checkout(@@CO_CAP@@).use { draft ->
            draft.@@C_SET@@(fixture.checkedMine)
            assertTrue(draft.@@C_CHECKERRUN@@())
            store.applyCanonical(fixture.seed().copy(@@C_PROP@@ = fixture.checkedTheirs)) // conflicts; value stays mine
            assertEquals("a conflict that preserves your value leaves the verdict standing", CheckStateFfi.Passed, draft.snapshot().@@C_CHECK@@)
            draft.resolveTakeTheirs(@@C_ID@@) // value moves to theirs
            assertEquals(CheckStateFfi.Unchecked, draft.snapshot().@@C_CHECK@@)
        } }
    }

    /** C16 — an unrun check on a dirty checked field blocks submit, pinned and keyed; a passing check unblocks it. The capability was present from checkout (D34) — C16 binds on UNRUN, not on absent. */
    @Test
    fun c16_anUnrunCheckOnADirtyCheckedFieldBlocksSubmit() {
        seeded().use { store -> store.checkout(@@CO_CAP@@).use { draft ->
            draft.@@C_SET@@(fixture.checkedMine)
            assertEquals(CheckStateFfi.Unchecked, draft.snapshot().@@C_CHECK@@)
            try {
                draft.submit()
                fail("an unchecked dirty checked field must not commit")
            } catch (e: SubmitErrorFfi.Validation) {
                val violation = e.report.ruleErrors.first { it.rule == "@@C_RULE@@" }
                assertEquals("@@C_REQKEY@@", violation.error.key)
                assertEquals(listOf(@@C_ID@@), violation.pins)
            }
            assertTrue(draft.@@C_CHECKERRUN@@())
            draft.submit() // now unblocked
        } }
    }

    /** C16 — the other half: a clean checked field needs no check, or an edit to another field could never submit. */
    @Test
    fun c16_aCleanCheckedFieldNeedsNoCheck() {
        seeded().use { store -> store.checkout(@@CO_NONE@@).use { draft ->
            draft.@@P_SET@@(fixture.primaryMine) // edit a NON-checked field
            assertFalse(draft.snapshot().@@C_PROP@@.dirty)
            draft.submit() // must not throw
        } }
    }

"#;

// ======================================================================================
// The Swift contract suite (D28) — the same map, one language further out.
// ======================================================================================

/// Emit the Swift per-language contract suite for `feature` (deliverable 5). Same C-IDs, same
/// values-only fixture shape as the Kotlin suite; the Swift idioms differ — `try` on every throwing
/// verb, `var v = …; v.field = …` for the value mutation Kotlin does with `copy`, enum cases in lower
/// camel (`.orphaned`, `.name`), and ARC/`deinit` for release (there is no `close()`, so C18/C22
/// observe the store's counts across a scope exit rather than an explicit call).
///
/// `binding_module` is the Swift module BoltFFI generates the bindings into (`GenProfileFfi`).
pub(crate) fn emit_swift_contract_suite(feature: &Feature, binding_module: &str) -> String {
    let entity = feature.entity.name.to_string();
    let store = format!("{entity}StoreFfi");
    let draft = format!("{entity}DraftFfi");
    let values = format!("{entity}Values");
    let stash = format!("{entity}StashFfi");
    let fixture_ty = format!("{entity}ConformanceFixture");
    let suite_ty = format!("{entity}ConformanceSuite");
    let factory = format!("{}ConformanceFixture", lower_camel(&feature.entity.name));

    let fields = FieldProj::all(feature);
    let text_fields: Vec<&FieldProj<'_>> = fields.iter().filter(|p| is_text_field(p)).collect();
    let checked_field = text_fields
        .iter()
        .copied()
        .find(|p| p.field.check.is_some());
    let plain: Vec<&FieldProj<'_>> = text_fields
        .iter()
        .copied()
        .filter(|p| p.field.check.is_none())
        .collect();

    let (Some(primary), Some(secondary)) = (plain.first().copied(), plain.get(1).copied()) else {
        return format!(
            "// @generated by bolted-ffi-gen. DO NOT EDIT.\n// {entity} has fewer than two editable \
             text fields; the contract suite needs two.\n"
        );
    };

    // Swift role names. The field-id is a lower-camel enum case (`.name`), not a screaming constant.
    let p_set = format!("trySet{}", upper_camel(primary.ident()));
    let p_prop = lower_camel(primary.ident());
    let p_idcase = format!(".{}", lower_camel(primary.ident()));
    let p_err = format!(
        "{}ErrorFfi",
        primary
            .field
            .value_ident()
            .map(|i| i.to_string())
            .unwrap_or_default()
    );
    let s_set = format!("trySet{}", upper_camel(secondary.ident()));
    let s_prop = lower_camel(secondary.ident());
    let s_idcase = format!(".{}", lower_camel(secondary.ident()));
    let s_err = format!(
        "{}ErrorFfi",
        secondary
            .field
            .value_ident()
            .map(|i| i.to_string())
            .unwrap_or_default()
    );

    let checked = checked_field.and_then(|p| {
        let c = p.field.check.as_ref()?;
        let camel = upper_camel(p.ident());
        Some((
            format!("trySet{camel}"),                     // c_set
            lower_camel(p.ident()),                       // c_prop
            format!(".{}", lower_camel(p.ident())),       // c_idcase
            format!("{}Check", lower_camel(p.ident())),   // c_check
            format!("{camel}Checker"),                    // c_checker
            format!("{}Checker", lower_camel(p.ident())), // c_label — the argument label D34's parameter gets in Swift
            format!("run{camel}Check"),                   // c_checkerrun
            c.required_key.clone(),                       // c_reqkey
            c.rule.clone(),                               // c_rule
        ))
    });

    // D34: the capability is a checkout/restore argument — the three Swift argument spellings,
    // empty when the feature declares no check (a check-less feature's call sites do not move).
    let (co_none, co_cap, rs_none) = match &checked {
        Some((_, _, _, _, _, c_label, _, _, _)) => (
            format!("{c_label}: nil"),
            format!("{c_label}: passingChecker()"),
            format!(", {c_label}: nil"),
        ),
        None => (String::new(), String::new(), String::new()),
    };

    // --- helpers -----------------------------------------------------------------------------------
    let mut helpers = String::new();
    helpers += &format!(
        "    private func seeded() throws -> {store} {{\n        let store = {store}()\n        \
         try store.applyCanonical(values: fixture.seed())\n        return store\n    }}\n\n"
    );
    helpers += &format!(
        "    private func seedWithPrimary(_ raw: String) -> {values} {{ var v = fixture.seed(); v.{p_prop} = raw; return v }}\n"
    );
    helpers += &format!(
        "    private func seedWithSecondary(_ raw: String) -> {values} {{ var v = fixture.seed(); v.{s_prop} = raw; return v }}\n"
    );
    if let Some((_, c_prop, ..)) = &checked {
        helpers += &format!(
            "    private func seedWithChecked(_ raw: String) -> {values} {{ var v = fixture.seed(); v.{c_prop} = raw; return v }}\n"
        );
    }
    if let Some((.., c_checker, _, _, _, _)) = &checked {
        helpers +=
            &format!("    private func passingChecker() -> {c_checker} {{ PassingChecker() }}\n");
    }
    helpers +=
        "\n    /// Dispatch a raw to the right text setter — mechanical, from the field list.\n";
    helpers += &format!(
        "    private func setText(_ draft: {draft}, _ id: {entity}FieldId, _ raw: String) throws {{\n        switch id {{\n"
    );
    for p in &text_fields {
        helpers += &format!(
            "        case .{}: try draft.trySet{}(raw: raw)\n",
            lower_camel(p.ident()),
            upper_camel(p.ident()),
        );
    }
    helpers += "        default: XCTFail(\"not a text field: \\(id)\")\n        }\n    }\n\n";
    helpers += "    /// Leave a create-flow draft committable: every field valid, any demanded check satisfied.\n";
    helpers += &format!("    private func fillValid(_ draft: {draft}) throws {{\n");
    for p in &fields {
        helpers += &format!(
            "        try draft.trySet{}(raw: fixture.seed().{})\n",
            upper_camel(p.ident()),
            lower_camel(p.ident()),
        );
    }
    if let Some((_, _, _, _, _, _, c_checkerrun, _, _)) = &checked {
        helpers += &format!(
            "        // a create-flow checked field is dirty, so C16 demands its check has run\n        \
             // (the checker itself arrived at checkout — D34)\n        \
             XCTAssertTrue(try draft.{c_checkerrun}())\n"
        );
    }
    helpers += "    }\n";

    // --- assemble ----------------------------------------------------------------------------------
    let mut out = String::new();
    out.push_str(SWIFT_SUITE_BANNER);
    out.push_str("\nimport XCTest\n@testable import @@MODULE@@\n\n");
    out.push_str(SWIFT_FIXTURE_PROTOCOL);
    if checked.is_some() {
        out.push_str(SWIFT_PASSING_CHECKER);
    }
    out.push_str("final class @@SUITE@@: XCTestCase {\n\n");
    out.push_str("    private let fixture: @@FIXTURE@@ = @@FACTORY@@()\n\n");
    out.push_str(&helpers);
    out.push('\n');
    out.push_str(SWIFT_CORE_TESTS);
    if checked.is_some() {
        out.push_str(SWIFT_CHECKED_TESTS);
    }
    out.push_str("}\n");

    // --- markers -----------------------------------------------------------------------------------
    let fixture_checked = match &checked {
        Some(_) => {
            "\n    /// The CHECKED text field's values: the one an async single-flight check guards.\n    var checkedBase: String { get }\n    var checkedMine: String { get }\n    var checkedTheirs: String { get }\n"
        }
        None => "",
    };
    let seed_checked = match &checked {
        Some(_) => "        XCTAssertEqual(fixture.checkedBase, fixture.seed().@@C_PROP@@)\n",
        None => "",
    };
    let c20_check = match &checked {
        Some(_) => {
            "\n            XCTAssertEqual(snap.@@C_CHECK@@, .unchecked) // no verdict survives the stash (C20)"
        }
        None => "",
    };

    let mut s = out
        .replace("@@MODULE@@", binding_module)
        .replace("@@CO_NONE@@", &co_none)
        .replace("@@CO_CAP@@", &co_cap)
        .replace("@@RS_NONE@@", &rs_none)
        .replace("@@FIXTURE_CHECKED@@", fixture_checked)
        .replace("@@SEED_CHECKED@@", seed_checked)
        .replace("@@C20_CHECK@@", c20_check)
        .replace("@@STORE@@", &store)
        .replace("@@DRAFT@@", &draft)
        .replace("@@VALUES@@", &values)
        .replace("@@STASH@@", &stash)
        .replace("@@FIXTURE@@", &fixture_ty)
        .replace("@@SUITE@@", &suite_ty)
        .replace("@@FACTORY@@", &factory)
        .replace("@@ENTITY@@", &entity)
        .replace("@@P_SET@@", &p_set)
        .replace("@@P_PROP@@", &p_prop)
        .replace("@@P_IDCASE@@", &p_idcase)
        .replace("@@P_ERR@@", &p_err)
        .replace("@@S_SET@@", &s_set)
        .replace("@@S_PROP@@", &s_prop)
        .replace("@@S_IDCASE@@", &s_idcase)
        .replace("@@S_ERR@@", &s_err);

    if let Some((
        c_set,
        c_prop,
        c_idcase,
        c_check,
        c_checker,
        _c_label,
        c_checkerrun,
        c_reqkey,
        c_rule,
    )) = &checked
    {
        s = s
            .replace("@@C_SET@@", c_set)
            .replace("@@C_PROP@@", c_prop)
            .replace("@@C_IDCASE@@", c_idcase)
            .replace("@@C_CHECKERRUN@@", c_checkerrun)
            .replace("@@C_CHECKER@@", c_checker)
            .replace("@@C_CHECK@@", c_check)
            .replace("@@C_REQKEY@@", c_reqkey)
            .replace("@@C_RULE@@", c_rule);
    }
    s
}

const SWIFT_SUITE_BANNER: &str = r#"// @generated by bolted-ffi-gen. DO NOT EDIT.
//
// Regenerate with `mise run gen:ffi`. `mise run check` byte-compares this file against the
// declaration it was generated from (D28); a hand-edit fails that drift check, and nothing may
// reformat it — the byte comparison is honest only because no formatter owns a foreign file.
//
// The per-language contract tests (step 13), one language out from the Kotlin suite: every C-ID the
// public generated surface can express (docs/CONFORMANCE.md), generic over the hand-written,
// values-only fixture beside this file. It verifies the BOUNDARY — that the Swift binding and wrapper
// preserve the core's semantics — not the algebra, which the Rust suite proves against four features.
"#;

const SWIFT_FIXTURE_PROTOCOL: &str = r#"/// Everything the emitted suite needs that the declaration cannot know: example values. The suite
/// emits the field-specific verb calls itself; this supplies a valid raw, a distinct second, and a
/// raw that fails tier-1 — never a judgement (kill criterion 3). Hand-written, one impl per feature.
protocol @@FIXTURE@@ {
    /// A fully-valid canonical entity to seed every store from.
    func seed() -> @@VALUES@@

    /// The PRIMARY text field's values — the one the suite edits. `base` is its value in `seed`.
    var primaryBase: String { get }
    var primaryMine: String { get }
    var primaryTheirs: String { get }
    var primaryOther: String { get }
    var primaryInvalid: String { get }

    /// The SECONDARY text field's values — the one the suite moves on the server (C19).
    var secondaryBase: String { get }
    var secondaryTheirs: String { get }
    var secondaryInvalid: String { get }
@@FIXTURE_CHECKED@@
    /// C08's tier-2 rule arrangement, as values — or nil if this feature declares no `#[bolted::rules]`
    /// rule. The declaration never sees a rule body, so unlike a `#[check]` its name and pins cannot be
    /// projected; the fixture supplies them.
    func ruleFlip() -> RuleFlip?
}

/// C08 as data. `dirtyEdits` are applied to a draft checked out from `seed`, leaving the rule
/// satisfied; `flippedCanonical` is a canonical whose rebase moves an *unpinned* field so the rule
/// fires, pinning `pins`. No branching, no judgement — the relationship lives in human-chosen values.
struct RuleFlip {
    let ruleName: String
    let dirtyEdits: [(@@ENTITY@@FieldId, String)]
    let flippedCanonical: @@VALUES@@
    let pins: [@@ENTITY@@FieldId]
}

"#;

const SWIFT_PASSING_CHECKER: &str = r#"/// Approves every value — the checker C13/C16 drive to a pass.
private final class PassingChecker: @@C_CHECKER@@ {
    func check(value: String) -> CheckVerdictFfi { .pass }
}

"#;

const SWIFT_CORE_TESTS: &str = r#"    /// The fixture's constants must describe the seed it returns, or every test below is built on sand.
    func testTheFixtureDescribesItsSeed() {
        XCTAssertEqual(fixture.primaryBase, fixture.seed().@@P_PROP@@)
        XCTAssertEqual(fixture.secondaryBase, fixture.seed().@@S_PROP@@)
@@SEED_CHECKED@@    }

    /// C01 — holding a value loses no validity; the canonical raw re-parses to the same value.
    func testC01RoundtripHoldsValidity() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        XCTAssertEqual(draft.snapshot().@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
        try draft.@@P_SET@@(raw: fixture.primaryMine) // idempotent: the canonical raw re-parses the same
        XCTAssertEqual(draft.snapshot().@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
    }

    /// C02 — a clean field adopts theirs on rebase and stays InSync.
    func testC02ACleanFieldFollowsCanonical() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        let f = draft.snapshot().@@P_PROP@@
        XCTAssertEqual(f.validity, .valid(value: fixture.primaryTheirs))
        guard case .inSync = f.sync else { return XCTFail("a clean field must adopt theirs and stay InSync") }
        XCTAssertFalse(f.dirty)
    }

    /// C03 — a dirty field whose canonical moved is never overwritten: it conflicts, naming theirs.
    func testC03ADirtyFieldIsNeverSilentlyOverwritten() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        let snap = draft.snapshot()
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine), "your value survives")
        guard case .conflicted(let base, let theirs) = snap.@@P_PROP@@.sync else {
            return XCTFail("a dirty field must conflict when its canonical moves")
        }
        XCTAssertEqual(theirs, fixture.primaryTheirs)
        XCTAssertEqual(base, fixture.primaryBase, "the recorded ancestor did not move")
        XCTAssertEqual(snap.conflicts, [@@P_IDCASE@@])
    }

    /// C04 — a dirty field whose value already equals theirs rebases clean.
    func testC04ConvergentRebaseIsClean() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryMine))
        let f = draft.snapshot().@@P_PROP@@
        guard case .inSync = f.sync else { return XCTFail("two edits that agree are not a conflict") }
        XCTAssertFalse(f.dirty)
    }

    /// C05 — setting a field back to its base clears dirty; dirtiness is value-based.
    func testC05RevertForFree() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        XCTAssertTrue(draft.snapshot().@@P_PROP@@.dirty)
        try draft.@@P_SET@@(raw: fixture.primaryBase)
        XCTAssertFalse(draft.snapshot().@@P_PROP@@.dirty, "dirty is a function of the data, not touch history")
    }

    /// C06 — a failed try_set is typed, records Invalid{raw}, blocks submit, and never commits the stale value.
    func testC06NoStaleValueSubmit() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        XCTAssertThrowsError(try draft.@@P_SET@@(raw: fixture.primaryInvalid)) { error in
            XCTAssertNotNil(error as? @@P_ERR@@, "an invalid raw is refused, typed")
        }
        guard case .invalid(let raw, _) = draft.snapshot().@@P_PROP@@.validity else {
            return XCTFail("an invalid raw must be recorded as Invalid")
        }
        XCTAssertEqual(raw, fixture.primaryInvalid)
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .validation(let report)? = error as? SubmitErrorFfi else {
                return XCTFail("an invalid field must block submit")
            }
            XCTAssertTrue(report.fieldErrors.contains { $0.field == @@P_IDCASE@@ })
        }
        XCTAssertEqual(store.canonical()?.@@P_PROP@@.validity, .valid(value: fixture.primaryBase),
            "the previous valid value was NOT silently committed")
    }

    /// C07 — precedence: a deleted canonical outranks a conflict (Orphaned wins).
    func testC07OrphanedOutranksConflicted() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        guard case .conflicted = draft.snapshot().@@P_PROP@@.sync else { return XCTFail("expected a conflict to outrank") }
        store.deleteCanonical() // the conflict survives the orphaning, or this proves nothing
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .orphaned? = error as? SubmitErrorFfi else { return XCTFail("orphaned outranks conflicted") }
        }
    }

    /// C07 — precedence: a conflict outranks a validation error (Conflicted wins).
    func testC07ConflictedOutranksValidation() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs)) // conflict on primary
        XCTAssertThrowsError(try draft.@@S_SET@@(raw: fixture.secondaryInvalid)) // invalid secondary
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .conflicted(let fields)? = error as? SubmitErrorFfi else {
                return XCTFail("conflicted outranks validation")
            }
            XCTAssertEqual(fields, [@@P_IDCASE@@])
        }
    }

    /// C08 — a rebase re-runs tier-2: moving an unpinned field can flip a rule pinned to a field it did not touch.
    func testC08RebaseRerunsTier2() throws {
        guard let flip = fixture.ruleFlip() else { throw XCTSkip("this feature declares no tier-2 rule") }
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        for (id, raw) in flip.dirtyEdits { try setText(draft, id, raw) }
        XCTAssertFalse(draft.validate().ruleErrors.contains { $0.rule == flip.ruleName }, "the arrangement must leave the rule satisfied")
        try store.applyCanonical(values: flip.flippedCanonical)
        let report = draft.validate()
        guard let violation = report.ruleErrors.first(where: { $0.rule == flip.ruleName }) else {
            return XCTFail("the rebase must make the rule fire")
        }
        XCTAssertEqual(violation.pins, flip.pins)
        XCTAssertTrue(draft.snapshot().conflicts.isEmpty, "a pinned field whose own canonical did not move is not conflicted (C19)")
    }

    /// C09 — resolve_keep_mine: value stays yours, base becomes theirs, dirty, InSync.
    func testC09ResolveKeepMine() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        guard case .conflicted = draft.snapshot().@@P_PROP@@.sync else { return XCTFail("expected a conflict to resolve") }
        try draft.resolveKeepMine(field: @@P_IDCASE@@)
        let snap = draft.snapshot()
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine), "value stays mine")
        guard case .inSync = snap.@@P_PROP@@.sync else { return XCTFail("keep-mine returns to InSync") }
        XCTAssertTrue(snap.@@P_PROP@@.dirty, "still dirty")
        XCTAssertEqual(draft.stash().@@P_PROP@@.base, fixture.primaryTheirs, "base became theirs")
    }

    /// C09 — resolve_take_theirs: value and base become theirs, clean, InSync.
    func testC09ResolveTakeTheirs() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        try draft.resolveTakeTheirs(field: @@P_IDCASE@@)
        let snap = draft.snapshot()
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryTheirs), "value becomes theirs")
        guard case .inSync = snap.@@P_PROP@@.sync else { return XCTFail("take-theirs is InSync") }
        XCTAssertFalse(snap.@@P_PROP@@.dirty, "clean")
        XCTAssertEqual(draft.stash().@@P_PROP@@.base, fixture.primaryTheirs)
    }

    /// C11 — deleting the canonical under a live draft orphans it; submit is a typed Orphaned; the draft stays live.
    func testC11DeletionOrphans() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        store.deleteCanonical()
        XCTAssertEqual(draft.snapshot().status, .orphaned)
        XCTAssertTrue(draft.isLive(), "the refusal hands the draft back")
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .orphaned? = error as? SubmitErrorFfi else {
                return XCTFail("submitting an orphan is a typed outcome, never silent")
            }
        }
        XCTAssertTrue(draft.isLive())
    }

    /// C12 — a create-flow draft (no base) is never in the fan-out, and commits normally once filled.
    func testC12CreateFlowNeverRebases() throws {
        let store = @@STORE@@() // empty: no canonical
        let draft = store.checkout(@@CO_CAP@@)
        try store.applyCanonical(values: fixture.seed())
        XCTAssertEqual(store.rebasingDraftCount(), 0, "a create-flow draft is not rebased")
        guard case .unset = draft.snapshot().@@P_PROP@@.validity else { return XCTFail("its primary must stay unset") }
        XCTAssertFalse(draft.snapshot().anyDirty)
        try fillValid(draft)
        try draft.submit() // must not throw
    }

    /// C12 — the contrapositive: a draft that keeps an ancestor in ANY field is entity-backed (it rebases, it orphans).
    func testC12APartiallyStashedDraftIsStillEntityBacked() throws {
        var stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@P_SET@@(raw: fixture.primaryMine)
            stash = draft.stash()
        }
        stash.@@S_PROP@@.base = nil // forget the secondary's ancestor
        let store = try seeded()
        let restored = try store.restore(accepted: store.acceptStash(stash: stash)@@RS_NONE@@)
        XCTAssertEqual(store.rebasingDraftCount(), 1, "one surviving ancestor still means entity-backed")
        _ = restored
        let empty = @@STORE@@() // ...and it orphans into a deleted canonical, not commit as new
        let orphan = try empty.restore(accepted: empty.acceptStash(stash: stash)@@RS_NONE@@)
        XCTAssertEqual(orphan.snapshot().status, .orphaned)
    }

    /// C14 — editing a conflicted field to theirs auto-converges (C04 with the events in the other order).
    func testC14EditingAConflictedFieldToTheirsAutoConverges() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        guard case .conflicted = draft.snapshot().@@P_PROP@@.sync else { return XCTFail("expected a conflict to converge") }
        try draft.@@P_SET@@(raw: fixture.primaryTheirs) // type their value
        let snap = draft.snapshot()
        guard case .inSync = snap.@@P_PROP@@.sync else { return XCTFail("editing to theirs must clear the conflict") }
        XCTAssertFalse(snap.@@P_PROP@@.dirty)
        XCTAssertTrue(snap.conflicts.isEmpty)
    }

    /// C15 — base_version tracks the rebase; an orphan's stamp stops moving.
    func testC15TheBaseVersionTracksTheRebase() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        let atCheckout = draft.snapshot().version
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        let afterRebase = draft.snapshot().version
        XCTAssertGreaterThan(afterRebase, atCheckout, "the stamp must advance on rebase")
        store.deleteCanonical()
        XCTAssertEqual(draft.snapshot().version, afterRebase, "an orphan's stamp stops moving")
    }

    /// C17 — a successful submit tombstones the draft; a second is AlreadySubmitted.
    func testC17ASuccessfulSubmitReleasesTheDraft() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        XCTAssertTrue(draft.isLive())
        try draft.submit()
        XCTAssertFalse(draft.isLive(), "a successful submit tombstones the handle")
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .alreadySubmitted? = error as? SubmitErrorFfi else { return XCTFail("a second submit is AlreadySubmitted") }
        }
    }

    /// C17 — a refused submit leaves the draft live and its edit intact, under the same id.
    func testC17ARefusedSubmitLeavesTheDraftLive() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .conflicted? = error as? SubmitErrorFfi else { return XCTFail("a conflict must refuse") }
        }
        XCTAssertTrue(draft.isLive(), "a refused submit must not consume the draft")
        XCTAssertEqual(draft.snapshot().@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
    }

    /// C18 — release frees the draft and stops the store rebasing it. Swift has no `close()`; ARC
    /// deinit at scope exit is the release path, and the store's counts are how it is observed.
    func testC18ReleaseFreesTheDraftAndStopsRebase() throws {
        let store = try seeded()
        do {
            let draft = store.checkout(@@CO_NONE@@)
            XCTAssertEqual(store.liveDraftCount(), 1)
            _ = draft
        } // ARC deinit → boltffi release → Rust Drop → deregister
        XCTAssertEqual(store.liveDraftCount(), 0, "release frees the draft")
        XCTAssertEqual(store.rebasingDraftCount(), 0)
        try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs)) // a released draft is not rebased
        XCTAssertEqual(store.liveDraftCount(), 0)
    }

    /// C19 — a dirty field whose OWN canonical never moved is not conflicted by a rebase of another field.
    func testC19ADirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine)
        try store.applyCanonical(values: seedWithSecondary(fixture.secondaryTheirs)) // secondary, and only secondary
        let snap = draft.snapshot()
        XCTAssertTrue(snap.conflicts.isEmpty, "the primary's canonical never moved")
        guard case .inSync = snap.@@P_PROP@@.sync else { return XCTFail("an unmoved canonical must not conflict") }
        XCTAssertTrue(snap.@@P_PROP@@.dirty, "my edit survives")
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
        XCTAssertEqual(snap.@@S_PROP@@.validity, .valid(value: fixture.secondaryTheirs), "the clean secondary adopted theirs (C02)")
    }

    /// C20 — a draft stashes each field's raw + ancestor (no sync/verdict, a structural fact) and restores them.
    func testC20ADraftStashesAndRestores() throws {
        let store = try seeded()
        let stash: @@STASH@@
        do {
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@P_SET@@(raw: fixture.primaryMine)
            XCTAssertThrowsError(try draft.@@S_SET@@(raw: fixture.secondaryInvalid)) // records Invalid{raw}
            stash = draft.stash()
        }
        // TextFieldStashFfi carries only raw + base — "no sync" is a compile-time fact of the type.
        XCTAssertEqual(stash.@@P_PROP@@.raw, fixture.primaryMine)
        XCTAssertEqual(stash.@@P_PROP@@.base, fixture.primaryBase)
        XCTAssertEqual(stash.@@S_PROP@@.raw, fixture.secondaryInvalid)
        let restored = try store.restore(accepted: store.acceptStash(stash: stash)@@RS_NONE@@)
        let snap = restored.snapshot()
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
        XCTAssertTrue(snap.@@P_PROP@@.dirty)
        guard case .invalid(let raw, _) = snap.@@S_PROP@@.validity else { return XCTFail("an Invalid{raw} survives process death") }
        XCTAssertEqual(raw, fixture.secondaryInvalid)
        XCTAssertTrue(snap.@@S_PROP@@.dirty)@@C20_CHECK@@
    }

    /// C21 — restore conflicts exactly the fields whose canonical moved while away; the others stay dirty · InSync.
    func testC21RestoreConflictsOnlyTheFieldsWhoseCanonicalMoved() throws {
        let stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@P_SET@@(raw: fixture.primaryMine)
            try draft.@@S_SET@@(raw: fixture.secondaryTheirs)
            stash = draft.stash()
        }
        let fresh = @@STORE@@()
        try fresh.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs)) // server moved the primary only
        let restored = try fresh.restore(accepted: fresh.acceptStash(stash: stash)@@RS_NONE@@)
        let snap = restored.snapshot()
        XCTAssertEqual(snap.conflicts, [@@P_IDCASE@@])
        guard case .conflicted(_, let theirs) = snap.@@P_PROP@@.sync else { return XCTFail("the moved field must conflict") }
        XCTAssertEqual(theirs, fixture.primaryTheirs, "a restored conflict names the CURRENT canonical")
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
        XCTAssertTrue(snap.@@S_PROP@@.dirty, "the secondary was untouched by the server: dirty, not conflicted")
        guard case .inSync = snap.@@S_PROP@@.sync else { return XCTFail("the untouched secondary must stay InSync") }
    }

    /// C21 — restoring into a deleted canonical orphans the draft; it does not resurrect the entity.
    func testC21RestoreIntoADeletedCanonicalOrphans() throws {
        let stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@P_SET@@(raw: fixture.primaryMine)
            stash = draft.stash()
        }
        let empty = @@STORE@@() // the server 404s
        let restored = try empty.restore(accepted: empty.acceptStash(stash: stash)@@RS_NONE@@)
        XCTAssertEqual(restored.snapshot().status, .orphaned)
        XCTAssertThrowsError(try restored.submit()) { error in
            guard case .orphaned? = error as? SubmitErrorFfi else { return XCTFail("expected .orphaned") }
        }
    }

    /// C21 — a resolution taken before the death survives it: its effect lives in the stashed ancestor.
    func testC21AResolutionSurvivesTheRestore() throws {
        let stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@P_SET@@(raw: fixture.primaryMine)
            try store.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs))
            try draft.resolveKeepMine(field: @@P_IDCASE@@) // base := theirs
            stash = draft.stash()
        }
        let fresh = @@STORE@@()
        try fresh.applyCanonical(values: seedWithPrimary(fixture.primaryTheirs)) // server still says theirs
        let restored = try fresh.restore(accepted: fresh.acceptStash(stash: stash)@@RS_NONE@@)
        let snap = restored.snapshot()
        XCTAssertTrue(snap.conflicts.isEmpty, "the user already resolved this; it must not be re-litigated")
        XCTAssertEqual(snap.@@P_PROP@@.validity, .valid(value: fixture.primaryMine))
        XCTAssertTrue(snap.@@P_PROP@@.dirty)
    }

    /// C22 — "a draft exists" and "a draft rebases" are different questions; a create-flow draft and an orphan diverge them.
    func testC22DraftCountAndRebasingDraftCountAreDifferentQuestions() throws {
        let empty = @@STORE@@()
        let createFlow = empty.checkout(@@CO_NONE@@)
        XCTAssertEqual(empty.liveDraftCount(), 1, "a create-flow draft exists")
        XCTAssertEqual(empty.rebasingDraftCount(), 0, "and is never rebased (C12)")
        try empty.applyCanonical(values: fixture.seed())
        let entityBacked = empty.checkout(@@CO_NONE@@)
        XCTAssertEqual(empty.liveDraftCount(), 2, "an entity-backed checkout is both")
        XCTAssertEqual(empty.rebasingDraftCount(), 1)
        empty.deleteCanonical() // orphan the entity-backed one
        XCTAssertEqual(empty.liveDraftCount(), 2, "an orphan still exists (C11)")
        XCTAssertEqual(empty.rebasingDraftCount(), 0, "but is never rebased")
        _ = createFlow
        _ = entityBacked
    }

    /// C23 — a stashed ancestor a tightened constraint invalidated degrades to dirty-from-unset, and conflicts on rebase.
    func testC23ADegradedAncestorRestoresDirtyAndConflicts() throws {
        var stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@S_SET@@(raw: fixture.secondaryTheirs)
            stash = draft.stash()
        }
        stash.@@S_PROP@@.base = fixture.secondaryInvalid // the ancestor no longer parses
        let store = try seeded() // canonical secondary == secondaryBase, differs from the rescued value
        let restored = try store.restore(accepted: store.acceptStash(stash: stash)@@RS_NONE@@)
        let snap = restored.snapshot()
        XCTAssertTrue(snap.@@S_PROP@@.dirty, "the rescued value survives, dirty")
        guard case .conflicted(let base, let theirs) = snap.@@S_PROP@@.sync else {
            return XCTFail("a lost ancestor conflicts, it does not overwrite (C03)")
        }
        XCTAssertEqual(theirs, fixture.secondaryBase)
        XCTAssertNil(base, "no ancestor is fabricated")
    }

    /// C23 — ...and the convergence guard: a lost ancestor whose rescued value equals canonical lands clean (C04).
    func testC23ADegradedAncestorConvergesClean() throws {
        var stash: @@STASH@@
        do {
            let store = try seeded()
            let draft = store.checkout(@@CO_NONE@@)
            try draft.@@S_SET@@(raw: fixture.secondaryTheirs)
            stash = draft.stash()
        }
        stash.@@S_PROP@@.base = fixture.secondaryInvalid
        let store = @@STORE@@()
        try store.applyCanonical(values: seedWithSecondary(fixture.secondaryTheirs)) // canonical == the rescued value
        let restored = try store.restore(accepted: store.acceptStash(stash: stash)@@RS_NONE@@)
        let snap = restored.snapshot()
        guard case .inSync = snap.@@S_PROP@@.sync else { return XCTFail("a lost ancestor that converges lands clean") }
        XCTAssertFalse(snap.@@S_PROP@@.dirty)
    }

"#;

const SWIFT_CHECKED_TESTS: &str = r#"    /// C13 — a value-moving edit resets the async verdict; a verdict endorses a value, so a changed value un-endorses it.
    func testC13AValueMovingEditResetsTheVerdict() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_CAP@@)
        try draft.@@C_SET@@(raw: fixture.checkedMine)
        XCTAssertTrue(try draft.@@C_CHECKERRUN@@())
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .passed)
        try draft.@@C_SET@@(raw: fixture.checkedTheirs) // a different value
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .unchecked, "a changed value un-endorses")
    }

    /// C13 — a value-preserving edit (edit-to-same) leaves the verdict standing.
    func testC13AValuePreservingEditLeavesTheVerdictStanding() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_CAP@@)
        try draft.@@C_SET@@(raw: fixture.checkedMine)
        XCTAssertTrue(try draft.@@C_CHECKERRUN@@())
        try draft.@@C_SET@@(raw: fixture.checkedMine) // edit to the SAME value
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .passed, "value-based, like dirty")
    }

    /// C13 — a preserved conflict leaves the verdict standing; resolving take-theirs moves the value and resets it.
    func testC13TakeTheirsMovesTheValueAndResetsTheVerdict() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_CAP@@)
        try draft.@@C_SET@@(raw: fixture.checkedMine)
        XCTAssertTrue(try draft.@@C_CHECKERRUN@@())
        try store.applyCanonical(values: seedWithChecked(fixture.checkedTheirs)) // conflicts; value stays mine
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .passed, "a conflict that preserves your value leaves the verdict standing")
        try draft.resolveTakeTheirs(field: @@C_IDCASE@@) // value moves to theirs
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .unchecked)
    }

    /// C16 — an unrun check on a dirty checked field blocks submit, pinned and keyed; a passing check
    /// unblocks it. The capability was present from checkout (D34) — C16 binds on UNRUN, not on absent.
    func testC16AnUnrunCheckOnADirtyCheckedFieldBlocksSubmit() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_CAP@@)
        try draft.@@C_SET@@(raw: fixture.checkedMine)
        XCTAssertEqual(draft.snapshot().@@C_CHECK@@, .unchecked)
        XCTAssertThrowsError(try draft.submit()) { error in
            guard case .validation(let report)? = error as? SubmitErrorFfi else {
                return XCTFail("an unchecked dirty checked field must not commit")
            }
            let violation = report.ruleErrors.first { $0.rule == "@@C_RULE@@" }
            XCTAssertEqual(violation?.error.key, "@@C_REQKEY@@")
            XCTAssertEqual(violation?.pins, [@@C_IDCASE@@])
        }
        XCTAssertTrue(try draft.@@C_CHECKERRUN@@())
        try draft.submit() // now unblocked
    }

    /// C16 — the other half: a clean checked field needs no check, or an edit to another field could never submit.
    func testC16ACleanCheckedFieldNeedsNoCheck() throws {
        let store = try seeded()
        let draft = store.checkout(@@CO_NONE@@)
        try draft.@@P_SET@@(raw: fixture.primaryMine) // edit a NON-checked field
        XCTAssertFalse(draft.snapshot().@@C_PROP@@.dirty)
        try draft.submit() // must not throw
    }

"#;

// ======================================================================================
// The C# contract suite (D28) — the same map, backend #3 (step 29).
// ======================================================================================

/// Emit the C# per-language contract suite for `feature` (step 29, deliverable 2). Same C-IDs, same
/// values-only fixture shape as the Kotlin/Swift suites; the C# idioms differ — `readonly record
/// struct` DTOs compared by value equality, `abstract record` DUs matched with `Is.InstanceOf<..>` +
/// a cast, `with` expressions for the value/stash copies Kotlin does with `copy`, PascalCase members,
/// `enum` field-ids, `<Value>ErrorFfiException` wrappers carrying `.Error`, and NUnit `Assert.Throws` /
/// `Assume.That`. Release is deterministic `Dispose()` (the C18 path; the finalizer is a lifecycle
/// concern the hand-written probe owns, not a contract row).
///
/// The bool from `RunUsernameCheck()` means "a check *ran*", never the verdict — a passing checker is
/// used and the verdict is read from the snapshot's check state (C13/C16), exactly as Kotlin/Swift do.
///
/// `binding_ns` is the C# namespace BoltFFI generates the bindings into (`Gen_profile_ffi`); `suite_ns`
/// is where the emitted suite + its hand-written fixture live (`ProfileProbe.Generated`).
pub(crate) fn emit_csharp_contract_suite(
    feature: &Feature,
    binding_ns: &str,
    suite_ns: &str,
) -> String {
    let entity = feature.entity.name.to_string();
    let store = format!("{entity}StoreFfi");
    let draft = format!("{entity}DraftFfi");
    let values = format!("{entity}Values");
    let stash = format!("{entity}StashFfi");
    let field_id = format!("{entity}FieldId");
    let fixture_ty = format!("{entity}ConformanceFixture");
    let suite_ty = format!("{entity}ConformanceSuite");
    let factory = format!("{entity}ConformanceFixtureFactory");

    let fields = FieldProj::all(feature);
    let text_fields: Vec<&FieldProj<'_>> = fields.iter().filter(|p| is_text_field(p)).collect();
    let checked_field = text_fields
        .iter()
        .copied()
        .find(|p| p.field.check.is_some());
    let plain: Vec<&FieldProj<'_>> = text_fields
        .iter()
        .copied()
        .filter(|p| p.field.check.is_none())
        .collect();

    let (Some(primary), Some(secondary)) = (plain.first().copied(), plain.get(1).copied()) else {
        return format!(
            "// @generated by bolted-ffi-gen. DO NOT EDIT.\n// {entity} has fewer than two editable \
             text fields; the contract suite needs two.\n"
        );
    };

    // C# role names. The field-id is a PascalCase enum member (`ProfileFieldId.Name`); snapshot and
    // value properties are PascalCase too, so the id member and the property share `upper_camel`.
    let p_set = format!("TrySet{}", upper_camel(primary.ident()));
    let p_prop = upper_camel(primary.ident()).to_string();
    let p_id = format!("{field_id}.{}", upper_camel(primary.ident()));
    let p_err = format!(
        "{}ErrorFfiException",
        primary
            .field
            .value_ident()
            .map(|i| i.to_string())
            .unwrap_or_default()
    );
    let s_set = format!("TrySet{}", upper_camel(secondary.ident()));
    let s_prop = upper_camel(secondary.ident()).to_string();
    let s_id = format!("{field_id}.{}", upper_camel(secondary.ident()));
    let s_err = format!(
        "{}ErrorFfiException",
        secondary
            .field
            .value_ident()
            .map(|i| i.to_string())
            .unwrap_or_default()
    );

    let checked = checked_field.and_then(|p| {
        let c = p.field.check.as_ref()?;
        let camel = upper_camel(p.ident()).to_string();
        Some((
            format!("TrySet{camel}"),      // 0 c_set
            camel.clone(),                 // 1 c_prop
            format!("{field_id}.{camel}"), // 2 c_id
            format!("{camel}Check"),       // 3 c_check
            format!("{camel}Checker"),     // 4 c_checker
            format!("Run{camel}Check"),    // 5 c_checkerrun
            c.required_key.clone(),        // 6 c_reqkey
            c.rule.clone(),                // 7 c_rule
        ))
    });

    // D34: the capability is a checkout/restore argument — declared absence, the passing checker, and
    // restore's trailing absence — empty when the feature declares no check, so a check-less feature's
    // call sites do not move.
    let (co_none, co_cap, rs_none) = if checked.is_some() {
        (
            "null".to_string(),
            "new PassingChecker()".to_string(),
            ", null".to_string(),
        )
    } else {
        (String::new(), String::new(), String::new())
    };

    // --- helpers (seeded + passing checker + dispatch + fill), built from the field list ------------
    let mut set_text_cases = String::new();
    for p in &text_fields {
        let camel = upper_camel(p.ident());
        set_text_cases +=
            &format!("            case {field_id}.{camel}: draft.TrySet{camel}(raw); break;\n");
    }
    let mut fill_lines = String::new();
    for p in &fields {
        let camel = upper_camel(p.ident());
        fill_lines += &format!("        draft.TrySet{camel}(fixture.Seed().{camel});\n");
    }

    let passing_checker = match &checked {
        Some((.., c_checker, _, _, _)) => format!(
            "    /// Approves every value — the checker C13/C16 drive to a pass.\n    \
             private sealed class PassingChecker : {c_checker}\n    {{\n        \
             public CheckVerdictFfi Check(string value) => CheckVerdictFfi.Pass;\n    }}\n\n"
        ),
        None => String::new(),
    };
    let fill_check = match &checked {
        Some((.., c_checkerrun, _, _)) => format!(
            "        // a create-flow checked field is dirty, so C16 demands its check has run\n        \
             // (the checker itself arrived at checkout — D34)\n        \
             Assert.That(draft.{c_checkerrun}(), Is.True);\n"
        ),
        None => String::new(),
    };

    let mut helpers = String::new();
    helpers += &format!(
        "    private {store} Seeded()\n    {{\n        var store = new {store}();\n        \
         store.ApplyCanonical(fixture.Seed());\n        return store;\n    }}\n\n"
    );
    helpers += &passing_checker;
    helpers +=
        "    /// Dispatch a raw to the right text setter — mechanical, from the field list.\n";
    helpers += &format!(
        "    private static void SetText({draft} draft, {field_id} id, string raw)\n    {{\n        switch (id)\n        {{\n"
    );
    helpers += &set_text_cases;
    helpers += "            default: Assert.Fail($\"not a text field: {id}\"); break;\n        }\n    }\n\n";
    helpers += "    /// Leave a create-flow draft committable: every field valid, any demanded check satisfied.\n";
    helpers += &format!(
        "    private void FillValid({draft} draft)\n    {{\n{fill_lines}{fill_check}    }}\n"
    );

    // --- assemble ----------------------------------------------------------------------------------
    let mut out = String::new();
    out.push_str(CSHARP_SUITE_BANNER);
    out.push_str(
        "\nusing System;\nusing System.Linq;\nusing NUnit.Framework;\nusing @@BINDING_NS@@;\n\nnamespace @@SUITE_NS@@;\n\n",
    );
    out.push_str(CSHARP_FIXTURE_INTERFACE);
    out.push_str("public sealed class @@SUITE@@\n{\n");
    out.push_str("    private readonly @@FIXTURE@@ fixture = @@FACTORY@@.Create();\n\n");
    out.push_str(&helpers);
    out.push('\n');
    out.push_str(CSHARP_SANITY_TEST);
    out.push_str(CSHARP_CORE_TESTS);
    if checked.is_some() {
        out.push_str(CSHARP_CHECKED_TESTS);
    }
    out.push_str("}\n");

    // --- markers -----------------------------------------------------------------------------------
    let fixture_checked = if checked.is_some() {
        "\n    string CheckedBase { get; }\n    string CheckedMine { get; }\n    string CheckedTheirs { get; }\n"
    } else {
        ""
    };
    let seed_checked = if checked.is_some() {
        "        Assert.That(fixture.Seed().@@C_PROP@@, Is.EqualTo(fixture.CheckedBase));\n"
    } else {
        ""
    };
    let c20_check = if checked.is_some() {
        "\n        Assert.That(snap.@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Unchecked())); // no verdict survives the stash (C20)"
    } else {
        ""
    };

    let mut s = out
        .replace("@@BINDING_NS@@", binding_ns)
        .replace("@@SUITE_NS@@", suite_ns)
        .replace("@@CO_NONE@@", &co_none)
        .replace("@@CO_CAP@@", &co_cap)
        .replace("@@RS_NONE@@", &rs_none)
        .replace("@@FIXTURE_CHECKED@@", fixture_checked)
        .replace("@@SEED_CHECKED@@", seed_checked)
        .replace("@@C20_CHECK@@", c20_check)
        .replace("@@STORE@@", &store)
        .replace("@@DRAFT@@", &draft)
        .replace("@@VALUES@@", &values)
        .replace("@@STASH@@", &stash)
        .replace("@@FIELD_ID@@", &field_id)
        .replace("@@FIXTURE@@", &fixture_ty)
        .replace("@@SUITE@@", &suite_ty)
        .replace("@@FACTORY@@", &factory)
        .replace("@@ENTITY@@", &entity)
        .replace("@@P_SET@@", &p_set)
        .replace("@@P_PROP@@", &p_prop)
        .replace("@@P_ID@@", &p_id)
        .replace("@@P_ERR@@", &p_err)
        .replace("@@S_SET@@", &s_set)
        .replace("@@S_PROP@@", &s_prop)
        .replace("@@S_ID@@", &s_id)
        .replace("@@S_ERR@@", &s_err);

    if let Some((c_set, c_prop, c_id, c_check, _c_checker, c_checkerrun, c_reqkey, c_rule)) =
        &checked
    {
        s = s
            .replace("@@C_SET@@", c_set)
            .replace("@@C_CHECKERRUN@@", c_checkerrun)
            .replace("@@C_CHECK@@", c_check)
            .replace("@@C_PROP@@", c_prop)
            .replace("@@C_ID@@", c_id)
            .replace("@@C_REQKEY@@", c_reqkey)
            .replace("@@C_RULE@@", c_rule);
    }
    s
}

const CSHARP_SUITE_BANNER: &str = r##"// @generated by bolted-ffi-gen. DO NOT EDIT.
//
// Regenerate with `mise run gen:ffi`. `mise run check` byte-compares this file against the
// declaration it was generated from (D28); a hand-edit fails that drift check, and nothing may
// reformat it — the byte comparison is honest only because no formatter owns a foreign file.
//
// The per-language contract tests (step 13/29), backend #3 (C#): every C-ID the public generated
// surface can express (docs/CONFORMANCE.md), generic over the hand-written, values-only fixture
// beside this file. It verifies the BOUNDARY — that the C# binding and wrapper preserve the core's
// semantics — not the algebra, which the Rust suite proves against four features.
"##;

const CSHARP_FIXTURE_INTERFACE: &str = r##"/// Everything the emitted suite needs that the declaration cannot know: example values. The suite
/// emits the field-specific verb calls itself; this supplies a valid raw, a distinct second, and a
/// raw that fails tier-1 — never a judgement (kill criterion 3). Hand-written, one impl per feature.
public interface @@FIXTURE@@
{
    /// A fully-valid canonical entity to seed every store from.
    @@VALUES@@ Seed();

    /// The PRIMARY text field's values — the one the suite edits. `Base` is its value in `Seed`.
    string PrimaryBase { get; }
    string PrimaryMine { get; }
    string PrimaryTheirs { get; }
    string PrimaryOther { get; }
    string PrimaryInvalid { get; }

    /// The SECONDARY text field's values — the one the suite moves on the server (C19).
    string SecondaryBase { get; }
    string SecondaryTheirs { get; }
    string SecondaryInvalid { get; }
@@FIXTURE_CHECKED@@
    /// C08's tier-2 rule arrangement, as values — or null if this feature declares no `#[bolted::rules]`
    /// rule. The declaration never sees a rule body, so unlike a `#[check]` its name and pins cannot be
    /// projected; the fixture supplies them.
    RuleFlip? RuleFlip();
}

/// C08 as data. `DirtyEdits` are applied to a draft checked out from `Seed`, leaving the rule
/// satisfied; `FlippedCanonical` is a canonical whose rebase moves an *unpinned* field so the rule
/// fires, pinning `Pins`. No branching, no judgement — the relationship lives in human-chosen values.
public sealed record RuleFlip(
    string RuleName,
    (@@FIELD_ID@@ Id, string Raw)[] DirtyEdits,
    @@VALUES@@ FlippedCanonical,
    @@FIELD_ID@@[] Pins
);

"##;

const CSHARP_SANITY_TEST: &str = r##"    /// The fixture's constants must describe the seed it returns, or every test below is built on sand.
    [Test]
    public void TheFixtureDescribesItsSeed()
    {
        Assert.That(fixture.Seed().@@P_PROP@@, Is.EqualTo(fixture.PrimaryBase));
        Assert.That(fixture.Seed().@@S_PROP@@, Is.EqualTo(fixture.SecondaryBase));
@@SEED_CHECKED@@    }

"##;

const CSHARP_CORE_TESTS: &str = r##"    /// C01 — holding a value loses no validity; the canonical raw re-parses to the same value.
    [Test]
    public void C01_RoundtripHoldsValidity()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        Assert.That(draft.Snapshot().@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
        draft.@@P_SET@@(fixture.PrimaryMine); // idempotent: the canonical raw re-parses the same
        Assert.That(draft.Snapshot().@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
    }

    /// C02 — a clean field adopts theirs on rebase and stays InSync.
    [Test]
    public void C02_ACleanFieldFollowsCanonical()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        var f = draft.Snapshot().@@P_PROP@@;
        Assert.That(f.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryTheirs)));
        Assert.That(f.Sync, Is.EqualTo(new TextFieldSync.InSync()));
        Assert.That(f.Dirty, Is.False);
    }

    /// C03 — a dirty field whose canonical moved is never overwritten: it conflicts, naming theirs.
    [Test]
    public void C03_ADirtyFieldIsNeverSilentlyOverwritten()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        var snap = draft.Snapshot();
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)), "your value survives");
        // Conflicted(Base, Theirs): the recorded ancestor did not move, and theirs is named.
        Assert.That(snap.@@P_PROP@@.Sync, Is.EqualTo(new TextFieldSync.Conflicted(fixture.PrimaryBase, fixture.PrimaryTheirs)));
        Assert.That(snap.Conflicts, Is.EqualTo(new[] { @@P_ID@@ }));
    }

    /// C04 — a dirty field whose value already equals theirs rebases clean.
    [Test]
    public void C04_ConvergentRebaseIsClean()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryMine });
        var f = draft.Snapshot().@@P_PROP@@;
        Assert.That(f.Sync, Is.EqualTo(new TextFieldSync.InSync()));
        Assert.That(f.Dirty, Is.False, "two edits that agree are not a conflict");
    }

    /// C05 — setting a field back to its base clears dirty; dirtiness is value-based.
    [Test]
    public void C05_RevertForFree()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        Assert.That(draft.Snapshot().@@P_PROP@@.Dirty, Is.True);
        draft.@@P_SET@@(fixture.PrimaryBase);
        Assert.That(draft.Snapshot().@@P_PROP@@.Dirty, Is.False, "dirty is a function of the data, not touch history");
    }

    /// C06 — a failed try_set is typed, records Invalid{raw}, blocks submit, and never commits the stale value.
    [Test]
    public void C06_NoStaleValueSubmit()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        Assert.Throws<@@P_ERR@@>(() => draft.@@P_SET@@(fixture.PrimaryInvalid), "an invalid raw is refused, typed");
        var v = draft.Snapshot().@@P_PROP@@.Validity;
        Assert.That(v, Is.InstanceOf<TextValidity.Invalid>(), "recorded as Invalid{raw}");
        Assert.That(((TextValidity.Invalid)v).Raw, Is.EqualTo(fixture.PrimaryInvalid));
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Validation>(), "an invalid field must block submit");
        var report = ((SubmitErrorFfi.Validation)ex.Error).Report;
        Assert.That(report.FieldErrors.Any(fe => fe.Field == @@P_ID@@), Is.True);
        Assert.That(store.Canonical()?.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryBase)),
            "the previous valid value was NOT silently committed");
    }

    /// C07 — precedence: a deleted canonical outranks a conflict (Orphaned wins).
    [Test]
    public void C07_OrphanedOutranksConflicted()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        Assert.That(draft.Snapshot().@@P_PROP@@.Sync, Is.InstanceOf<TextFieldSync.Conflicted>());
        store.DeleteCanonical(); // the conflict survives the orphaning, or this proves nothing
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Orphaned>(), "orphaned outranks conflicted");
    }

    /// C07 — precedence: a conflict outranks a validation error (Conflicted wins).
    [Test]
    public void C07_ConflictedOutranksValidation()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs }); // conflict on primary
        Assert.Throws<@@S_ERR@@>(() => draft.@@S_SET@@(fixture.SecondaryInvalid)); // invalid secondary
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Conflicted>(), "conflicted outranks validation");
        Assert.That(((SubmitErrorFfi.Conflicted)ex.Error).Fields, Is.EqualTo(new[] { @@P_ID@@ }));
    }

    /// C08 — a rebase re-runs tier-2: moving an unpinned field can flip a rule pinned to a field it did not touch.
    [Test]
    public void C08_RebaseRerunsTier2()
    {
        var flip = fixture.RuleFlip();
        Assume.That(flip != null, "this feature declares no tier-2 rule");
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        foreach (var (id, raw) in flip!.DirtyEdits) SetText(draft, id, raw);
        Assert.That(draft.Validate().RuleErrors.Any(v => v.Rule == flip.RuleName), Is.False, "the arrangement must leave the rule satisfied");
        store.ApplyCanonical(flip.FlippedCanonical);
        var report = draft.Validate();
        Assert.That(report.RuleErrors.Any(v => v.Rule == flip.RuleName), Is.True, "the rebase must make the rule fire");
        var violation = report.RuleErrors.First(v => v.Rule == flip.RuleName);
        Assert.That(violation.Pins, Is.EqualTo(flip.Pins));
        Assert.That(draft.Snapshot().Conflicts, Is.Empty, "a pinned field whose own canonical did not move is not conflicted (C19)");
    }

    /// C09 — resolve_keep_mine: value stays yours, base becomes theirs, dirty, InSync.
    [Test]
    public void C09_ResolveKeepMine()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        Assert.That(draft.Snapshot().@@P_PROP@@.Sync, Is.InstanceOf<TextFieldSync.Conflicted>());
        draft.ResolveKeepMine(@@P_ID@@);
        var snap = draft.Snapshot();
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)), "value stays mine");
        Assert.That(snap.@@P_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()), "returns to InSync");
        Assert.That(snap.@@P_PROP@@.Dirty, Is.True, "still dirty");
        Assert.That(draft.Stash().@@P_PROP@@.Base, Is.EqualTo(fixture.PrimaryTheirs), "base became theirs");
    }

    /// C09 — resolve_take_theirs: value and base become theirs, clean, InSync.
    [Test]
    public void C09_ResolveTakeTheirs()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        draft.ResolveTakeTheirs(@@P_ID@@);
        var snap = draft.Snapshot();
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryTheirs)), "value becomes theirs");
        Assert.That(snap.@@P_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()));
        Assert.That(snap.@@P_PROP@@.Dirty, Is.False, "clean");
        Assert.That(draft.Stash().@@P_PROP@@.Base, Is.EqualTo(fixture.PrimaryTheirs));
    }

    /// C11 — deleting the canonical under a live draft orphans it; submit is a typed Orphaned; the draft stays live.
    [Test]
    public void C11_DeletionOrphans()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        store.DeleteCanonical();
        Assert.That(draft.Snapshot().Status, Is.EqualTo(DraftStatusFfi.Orphaned));
        Assert.That(draft.IsLive(), Is.True, "the refusal hands the draft back");
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Orphaned>(), "submitting an orphan is a typed outcome, never silent");
        Assert.That(draft.IsLive(), Is.True);
    }

    /// C12 — a create-flow draft (no base) is never in the fan-out, and commits normally once filled.
    [Test]
    public void C12_CreateFlowNeverRebases()
    {
        using var store = new @@STORE@@(); // empty: no canonical
        using var draft = store.Checkout(@@CO_CAP@@);
        store.ApplyCanonical(fixture.Seed());
        Assert.That(store.RebasingDraftCount(), Is.EqualTo(0u), "a create-flow draft is not rebased");
        Assert.That(draft.Snapshot().@@P_PROP@@.Validity, Is.InstanceOf<TextValidity.Unset>(), "its primary stays unset");
        Assert.That(draft.Snapshot().AnyDirty, Is.False);
        FillValid(draft);
        Assert.DoesNotThrow(() => draft.Submit()); // must not throw
    }

    /// C12 — the contrapositive: a draft that keeps an ancestor in ANY field is entity-backed (it rebases, it orphans).
    [Test]
    public void C12_APartiallyStashedDraftIsStillEntityBacked()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@P_SET@@(fixture.PrimaryMine);
            stash = draft.Stash();
        }
        var partial = stash with { @@S_PROP@@ = stash.@@S_PROP@@ with { Base = null } }; // forget the secondary's ancestor
        using (var store = Seeded())
        using (var restored = store.Restore(store.AcceptStash(partial)@@RS_NONE@@))
        {
            Assert.That(store.RebasingDraftCount(), Is.EqualTo(1u), "one surviving ancestor still means entity-backed");
        }
        using (var empty = new @@STORE@@()) // ...and it orphans into a deleted canonical, not commit as new
        using (var orphan = empty.Restore(empty.AcceptStash(partial)@@RS_NONE@@))
        {
            Assert.That(orphan.Snapshot().Status, Is.EqualTo(DraftStatusFfi.Orphaned));
        }
    }

    /// C14 — editing a conflicted field to theirs auto-converges (C04 with the events in the other order).
    [Test]
    public void C14_EditingAConflictedFieldToTheirsAutoConverges()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        Assert.That(draft.Snapshot().@@P_PROP@@.Sync, Is.InstanceOf<TextFieldSync.Conflicted>());
        draft.@@P_SET@@(fixture.PrimaryTheirs); // type their value
        var snap = draft.Snapshot();
        Assert.That(snap.@@P_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()), "editing to theirs must clear the conflict");
        Assert.That(snap.@@P_PROP@@.Dirty, Is.False);
        Assert.That(snap.Conflicts, Is.Empty);
    }

    /// C15 — base_version tracks the rebase; an orphan's stamp stops moving.
    [Test]
    public void C15_TheBaseVersionTracksTheRebase()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        var atCheckout = draft.Snapshot().Version;
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        var afterRebase = draft.Snapshot().Version;
        Assert.That(afterRebase, Is.GreaterThan(atCheckout), "the stamp must advance on rebase");
        store.DeleteCanonical();
        Assert.That(draft.Snapshot().Version, Is.EqualTo(afterRebase), "an orphan's stamp stops moving");
    }

    /// C17 — a successful submit tombstones the draft; a second is AlreadySubmitted.
    [Test]
    public void C17_ASuccessfulSubmitReleasesTheDraft()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        Assert.That(draft.IsLive(), Is.True);
        draft.Submit();
        Assert.That(draft.IsLive(), Is.False, "a successful submit tombstones the handle");
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.AlreadySubmitted>(), "a second submit is AlreadySubmitted");
    }

    /// C17 — a refused submit leaves the draft live and its edit intact, under the same id.
    [Test]
    public void C17_ARefusedSubmitLeavesTheDraftLive()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Conflicted>(), "a conflict must refuse");
        Assert.That(draft.IsLive(), Is.True, "a refused submit must not consume the draft");
        Assert.That(draft.Snapshot().@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
    }

    /// C18 — Dispose frees the draft (the deterministic release path), is idempotent, and stops the store rebasing it.
    [Test]
    public void C18_DisposeFreesIsIdempotentAndStopsRebase()
    {
        using var store = Seeded();
        var draft = store.Checkout(@@CO_NONE@@);
        Assert.That(store.LiveDraftCount(), Is.EqualTo(1u));
        draft.Dispose();
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u), "Dispose frees the draft");
        Assert.That(store.RebasingDraftCount(), Is.EqualTo(0u));
        draft.Dispose();
        draft.Dispose(); // idempotent, even on an id already gone (guarded by the generated Interlocked)
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u), "Dispose is idempotent");
        store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs }); // a disposed draft is not rebased
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u));
    }

    /// C19 — a dirty field whose OWN canonical never moved is not conflicted by a rebase of another field.
    [Test]
    public void C19_ADirtyFieldIsNotConflictedWhenItsOwnCanonicalDidNotMove()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine);
        store.ApplyCanonical(fixture.Seed() with { @@S_PROP@@ = fixture.SecondaryTheirs }); // secondary, and only secondary
        var snap = draft.Snapshot();
        Assert.That(snap.Conflicts, Is.Empty, "the primary's canonical never moved");
        Assert.That(snap.@@P_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()));
        Assert.That(snap.@@P_PROP@@.Dirty, Is.True, "my edit survives");
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
        Assert.That(snap.@@S_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.SecondaryTheirs)), "the clean secondary adopted theirs (C02)");
    }

    /// C20 — a draft stashes each field's raw + ancestor (no sync/verdict, a structural fact) and restores them.
    [Test]
    public void C20_ADraftStashesAndRestores()
    {
        using var store = Seeded();
        @@STASH@@ stash;
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@P_SET@@(fixture.PrimaryMine);
            Assert.Throws<@@S_ERR@@>(() => draft.@@S_SET@@(fixture.SecondaryInvalid)); // records Invalid{raw}
            stash = draft.Stash();
        }
        // TextFieldStashFfi carries only Raw + Base — "no sync" is a compile-time fact of the type.
        Assert.That(stash.@@P_PROP@@.Raw, Is.EqualTo(fixture.PrimaryMine));
        Assert.That(stash.@@P_PROP@@.Base, Is.EqualTo(fixture.PrimaryBase));
        Assert.That(stash.@@S_PROP@@.Raw, Is.EqualTo(fixture.SecondaryInvalid));
        using var restored = store.Restore(store.AcceptStash(stash)@@RS_NONE@@);
        var snap = restored.Snapshot();
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
        Assert.That(snap.@@P_PROP@@.Dirty, Is.True);
        Assert.That(snap.@@S_PROP@@.Validity, Is.InstanceOf<TextValidity.Invalid>(), "an Invalid{raw} survives process death");
        Assert.That(((TextValidity.Invalid)snap.@@S_PROP@@.Validity).Raw, Is.EqualTo(fixture.SecondaryInvalid));
        Assert.That(snap.@@S_PROP@@.Dirty, Is.True);@@C20_CHECK@@
    }

    /// C21 — restore conflicts exactly the fields whose canonical moved while away; the others stay dirty · InSync.
    [Test]
    public void C21_RestoreConflictsOnlyTheFieldsWhoseCanonicalMoved()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@P_SET@@(fixture.PrimaryMine);
            draft.@@S_SET@@(fixture.SecondaryTheirs);
            stash = draft.Stash();
        }
        using var fresh = new @@STORE@@();
        fresh.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs }); // server moved the primary only
        using var restored = fresh.Restore(fresh.AcceptStash(stash)@@RS_NONE@@);
        var snap = restored.Snapshot();
        Assert.That(snap.Conflicts, Is.EqualTo(new[] { @@P_ID@@ }));
        Assert.That(snap.@@P_PROP@@.Sync, Is.InstanceOf<TextFieldSync.Conflicted>());
        Assert.That(((TextFieldSync.Conflicted)snap.@@P_PROP@@.Sync).Theirs, Is.EqualTo(fixture.PrimaryTheirs), "a restored conflict names the CURRENT canonical");
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
        Assert.That(snap.@@S_PROP@@.Dirty, Is.True, "the secondary was untouched by the server: dirty, not conflicted");
        Assert.That(snap.@@S_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()));
    }

    /// C21 — restoring into a deleted canonical orphans the draft; it does not resurrect the entity.
    [Test]
    public void C21_RestoreIntoADeletedCanonicalOrphans()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@P_SET@@(fixture.PrimaryMine);
            stash = draft.Stash();
        }
        using var empty = new @@STORE@@(); // the server 404s
        using var restored = empty.Restore(empty.AcceptStash(stash)@@RS_NONE@@);
        Assert.That(restored.Snapshot().Status, Is.EqualTo(DraftStatusFfi.Orphaned));
        var ex = Assert.Throws<SubmitErrorFfiException>(() => restored.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Orphaned>());
    }

    /// C21 — a resolution taken before the death survives it: its effect lives in the stashed ancestor.
    [Test]
    public void C21_AResolutionSurvivesTheRestore()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@P_SET@@(fixture.PrimaryMine);
            store.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs });
            draft.ResolveKeepMine(@@P_ID@@); // base := theirs
            stash = draft.Stash();
        }
        using var fresh = new @@STORE@@();
        fresh.ApplyCanonical(fixture.Seed() with { @@P_PROP@@ = fixture.PrimaryTheirs }); // server still says theirs
        using var restored = fresh.Restore(fresh.AcceptStash(stash)@@RS_NONE@@);
        var snap = restored.Snapshot();
        Assert.That(snap.Conflicts, Is.Empty, "the user already resolved this; it must not be re-litigated");
        Assert.That(snap.@@P_PROP@@.Validity, Is.EqualTo(new TextValidity.Valid(fixture.PrimaryMine)));
        Assert.That(snap.@@P_PROP@@.Dirty, Is.True);
    }

    /// C22 — "a draft exists" and "a draft rebases" are different questions; a create-flow draft and an orphan diverge them.
    [Test]
    public void C22_DraftCountAndRebasingDraftCountAreDifferentQuestions()
    {
        using var empty = new @@STORE@@();
        using var createFlow = empty.Checkout(@@CO_NONE@@);
        Assert.That(empty.LiveDraftCount(), Is.EqualTo(1u), "a create-flow draft exists");
        Assert.That(empty.RebasingDraftCount(), Is.EqualTo(0u), "and is never rebased (C12)");
        empty.ApplyCanonical(fixture.Seed());
        using var entityBacked = empty.Checkout(@@CO_NONE@@);
        Assert.That(empty.LiveDraftCount(), Is.EqualTo(2u), "an entity-backed checkout is both");
        Assert.That(empty.RebasingDraftCount(), Is.EqualTo(1u));
        empty.DeleteCanonical(); // orphan the entity-backed one
        Assert.That(empty.LiveDraftCount(), Is.EqualTo(2u), "an orphan still exists (C11)");
        Assert.That(empty.RebasingDraftCount(), Is.EqualTo(0u), "but is never rebased");
    }

    /// C23 — a stashed ancestor a tightened constraint invalidated degrades to dirty-from-unset, and conflicts on rebase.
    [Test]
    public void C23_ADegradedAncestorRestoresDirtyAndConflicts()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@S_SET@@(fixture.SecondaryTheirs);
            stash = draft.Stash();
        }
        var tightened = stash with { @@S_PROP@@ = stash.@@S_PROP@@ with { Base = fixture.SecondaryInvalid } }; // ancestor no longer parses
        using var store2 = Seeded(); // canonical secondary == secondaryBase, differs from the rescued value
        using var restored = store2.Restore(store2.AcceptStash(tightened)@@RS_NONE@@);
        var snap = restored.Snapshot();
        Assert.That(snap.@@S_PROP@@.Dirty, Is.True, "the rescued value survives, dirty");
        Assert.That(snap.@@S_PROP@@.Sync, Is.InstanceOf<TextFieldSync.Conflicted>(), "a lost ancestor conflicts, it does not overwrite (C03)");
        var sync = (TextFieldSync.Conflicted)snap.@@S_PROP@@.Sync;
        Assert.That(sync.Theirs, Is.EqualTo(fixture.SecondaryBase));
        Assert.That(sync.Base, Is.Null, "no ancestor is fabricated");
    }

    /// C23 — ...and the convergence guard: a lost ancestor whose rescued value equals canonical lands clean (C04).
    [Test]
    public void C23_ADegradedAncestorConvergesClean()
    {
        @@STASH@@ stash;
        using (var store = Seeded())
        using (var draft = store.Checkout(@@CO_NONE@@))
        {
            draft.@@S_SET@@(fixture.SecondaryTheirs);
            stash = draft.Stash();
        }
        var tightened = stash with { @@S_PROP@@ = stash.@@S_PROP@@ with { Base = fixture.SecondaryInvalid } };
        using var store2 = new @@STORE@@();
        store2.ApplyCanonical(fixture.Seed() with { @@S_PROP@@ = fixture.SecondaryTheirs }); // canonical == the rescued value
        using var restored = store2.Restore(store2.AcceptStash(tightened)@@RS_NONE@@);
        var snap = restored.Snapshot();
        Assert.That(snap.@@S_PROP@@.Sync, Is.EqualTo(new TextFieldSync.InSync()), "a lost ancestor that converges lands clean");
        Assert.That(snap.@@S_PROP@@.Dirty, Is.False);
    }

"##;

const CSHARP_CHECKED_TESTS: &str = r##"    /// C13 — a value-moving edit resets the async verdict; a verdict endorses a value, so a changed value un-endorses it.
    [Test]
    public void C13_AValueMovingEditResetsTheVerdict()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_CAP@@);
        draft.@@C_SET@@(fixture.CheckedMine);
        Assert.That(draft.@@C_CHECKERRUN@@(), Is.True, "a check ran (the bool is 'ran', not the verdict)");
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Passed()));
        draft.@@C_SET@@(fixture.CheckedTheirs); // a different value
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Unchecked()), "a changed value un-endorses");
    }

    /// C13 — a value-preserving edit (edit-to-same) leaves the verdict standing.
    [Test]
    public void C13_AValuePreservingEditLeavesTheVerdictStanding()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_CAP@@);
        draft.@@C_SET@@(fixture.CheckedMine);
        Assert.That(draft.@@C_CHECKERRUN@@(), Is.True);
        draft.@@C_SET@@(fixture.CheckedMine); // edit to the SAME value
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Passed()), "value-based, like dirty");
    }

    /// C13 — a preserved conflict leaves the verdict standing; resolving take-theirs moves the value and resets it.
    [Test]
    public void C13_TakeTheirsMovesTheValueAndResetsTheVerdict()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_CAP@@);
        draft.@@C_SET@@(fixture.CheckedMine);
        Assert.That(draft.@@C_CHECKERRUN@@(), Is.True);
        store.ApplyCanonical(fixture.Seed() with { @@C_PROP@@ = fixture.CheckedTheirs }); // conflicts; value stays mine
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Passed()), "a conflict that preserves your value leaves the verdict standing");
        draft.ResolveTakeTheirs(@@C_ID@@); // value moves to theirs
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Unchecked()));
    }

    /// C16 — an unrun check on a dirty checked field blocks submit, pinned and keyed; a passing check
    /// unblocks it. The capability was present from checkout (D34) — C16 binds on UNRUN, not on absent.
    [Test]
    public void C16_AnUnrunCheckOnADirtyCheckedFieldBlocksSubmit()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_CAP@@);
        draft.@@C_SET@@(fixture.CheckedMine);
        Assert.That(draft.Snapshot().@@C_CHECK@@, Is.EqualTo(new CheckStateFfi.Unchecked()));
        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Validation>(), "an unchecked dirty checked field must not commit");
        var report = ((SubmitErrorFfi.Validation)ex.Error).Report;
        var violation = report.RuleErrors.First(v => v.Rule == "@@C_RULE@@");
        Assert.That(violation.Error.Key, Is.EqualTo("@@C_REQKEY@@"));
        Assert.That(violation.Pins, Is.EqualTo(new[] { @@C_ID@@ }));
        Assert.That(draft.@@C_CHECKERRUN@@(), Is.True);
        draft.Submit(); // now unblocked
    }

    /// C16 — the other half: a clean checked field needs no check, or an edit to another field could never submit.
    [Test]
    public void C16_ACleanCheckedFieldNeedsNoCheck()
    {
        using var store = Seeded();
        using var draft = store.Checkout(@@CO_NONE@@);
        draft.@@P_SET@@(fixture.PrimaryMine); // edit a NON-checked field
        Assert.That(draft.Snapshot().@@C_PROP@@.Dirty, Is.False);
        Assert.DoesNotThrow(() => draft.Submit()); // must not throw
    }

"##;
