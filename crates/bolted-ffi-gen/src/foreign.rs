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
    checker_set: String,
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
            checker_set: format!("set{camel}Checker"),
            checker_run: format!("run{camel}Check"),
            required_key: c.required_key.clone(),
            rule_name: c.rule.clone(),
        })
    });

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
                 draft.{}(passingChecker())\n        check(draft.{}())\n",
                c.checker_set, c.checker_run,
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
            .replace("@@C_CHECKERSET@@", &c.checker_set)
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            assertTrue(draft.snapshot().@@P_PROP@@.dirty)
            draft.@@P_SET@@(fixture.primaryBase)
            assertFalse("dirty is a function of the data, not of touch history", draft.snapshot().@@P_PROP@@.dirty)
        } }
    }

    /** C06 — a failed try_set is typed, records Invalid{raw}, blocks submit, and never commits the stale value. */
    @Test
    fun c06_noStaleValueSubmit() {
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
            store.checkout().use { draft ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.stash()
        } }
        val partial = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = null)) // forget the secondary's ancestor
        seeded().use { store ->
            store.restore(store.acceptStash(partial)).use { _ ->
                assertEquals("one surviving ancestor still means entity-backed", 1u, store.rebasingDraftCount())
            }
        }
        @@STORE@@.new().use { empty -> // ...and it orphans into a deleted canonical, not commit as new
            empty.restore(empty.acceptStash(partial)).use { restored ->
                assertEquals(DraftStatusFfi.ORPHANED, restored.snapshot().status)
            }
        }
    }

    /** C14 — editing a conflicted field to theirs auto-converges (C04 with the events in the other order). */
    @Test
    fun c14_editingAConflictedFieldToTheirsAutoConverges() {
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
        seeded().use { store -> store.checkout().use { draft ->
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
            val draft = store.checkout()
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
        seeded().use { store -> store.checkout().use { draft ->
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
            val stash = store.checkout().use { draft ->
                draft.@@P_SET@@(fixture.primaryMine)
                try { draft.@@S_SET@@(fixture.secondaryInvalid) } catch (e: @@S_ERR@@) { /* records Invalid{raw} */ }
                draft.stash()
            }
            // TextFieldStashFfi carries only raw + base — "no sync" is a compile-time fact of the type.
            assertEquals(fixture.primaryMine, stash.@@P_PROP@@.raw)
            assertEquals(fixture.primaryBase, stash.@@P_PROP@@.base)
            assertEquals(fixture.secondaryInvalid, stash.@@S_PROP@@.raw)
            store.restore(store.acceptStash(stash)).use { restored ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        @@STORE@@.new().use { fresh ->
            fresh.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // server moved the primary only
            fresh.restore(fresh.acceptStash(stash)).use { restored ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            draft.stash()
        } }
        @@STORE@@.new().use { empty -> // the server 404s
            empty.restore(empty.acceptStash(stash)).use { restored ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine)
            store.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs))
            draft.resolveKeepMine(@@P_ID@@) // base := theirs
            draft.stash()
        } }
        @@STORE@@.new().use { fresh ->
            fresh.applyCanonical(fixture.seed().copy(@@P_PROP@@ = fixture.primaryTheirs)) // server still says theirs
            fresh.restore(fresh.acceptStash(stash)).use { restored ->
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
            empty.checkout().use { _ ->
                assertEquals("a create-flow draft exists", 1u, empty.liveDraftCount())
                assertEquals("and is never rebased (C12)", 0u, empty.rebasingDraftCount())
                empty.applyCanonical(fixture.seed())
                empty.checkout().use { _ ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        val tightened = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = fixture.secondaryInvalid)) // ancestor no longer parses
        @@STORE@@.new().use { store ->
            store.applyCanonical(fixture.seed()) // canonical secondary == secondaryBase, differs from the rescued value
            store.restore(store.acceptStash(tightened)).use { restored ->
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
        val stash = seeded().use { store -> store.checkout().use { draft ->
            draft.@@S_SET@@(fixture.secondaryTheirs)
            draft.stash()
        } }
        val tightened = stash.copy(@@S_PROP@@ = stash.@@S_PROP@@.copy(base = fixture.secondaryInvalid))
        @@STORE@@.new().use { store ->
            store.applyCanonical(fixture.seed().copy(@@S_PROP@@ = fixture.secondaryTheirs)) // canonical == the rescued value
            store.restore(store.acceptStash(tightened)).use { restored ->
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
        seeded().use { store -> store.checkout().use { draft ->
            draft.@@C_CHECKERSET@@(passingChecker())
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
        seeded().use { store -> store.checkout().use { draft ->
            draft.@@C_CHECKERSET@@(passingChecker())
            draft.@@C_SET@@(fixture.checkedMine)
            assertTrue(draft.@@C_CHECKERRUN@@())
            draft.@@C_SET@@(fixture.checkedMine) // edit to the SAME value
            assertEquals("value-based, like dirty", CheckStateFfi.Passed, draft.snapshot().@@C_CHECK@@)
        } }
    }

    /** C13 — a preserved conflict leaves the verdict standing; resolving take-theirs moves the value and resets it. */
    @Test
    fun c13_takeTheirsMovesTheValueAndResetsTheVerdict() {
        seeded().use { store -> store.checkout().use { draft ->
            draft.@@C_CHECKERSET@@(passingChecker())
            draft.@@C_SET@@(fixture.checkedMine)
            assertTrue(draft.@@C_CHECKERRUN@@())
            store.applyCanonical(fixture.seed().copy(@@C_PROP@@ = fixture.checkedTheirs)) // conflicts; value stays mine
            assertEquals("a conflict that preserves your value leaves the verdict standing", CheckStateFfi.Passed, draft.snapshot().@@C_CHECK@@)
            draft.resolveTakeTheirs(@@C_ID@@) // value moves to theirs
            assertEquals(CheckStateFfi.Unchecked, draft.snapshot().@@C_CHECK@@)
        } }
    }

    /** C16 — an unrun check on a dirty checked field blocks submit, pinned and keyed; a passing check unblocks it. */
    @Test
    fun c16_anUnrunCheckOnADirtyCheckedFieldBlocksSubmit() {
        seeded().use { store -> store.checkout().use { draft ->
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
            draft.@@C_CHECKERSET@@(passingChecker())
            assertTrue(draft.@@C_CHECKERRUN@@())
            draft.submit() // now unblocked
        } }
    }

    /** C16 — the other half: a clean checked field needs no check, or an edit to another field could never submit. */
    @Test
    fun c16_aCleanCheckedFieldNeedsNoCheck() {
        seeded().use { store -> store.checkout().use { draft ->
            draft.@@P_SET@@(fixture.primaryMine) // edit a NON-checked field
            assertFalse(draft.snapshot().@@C_PROP@@.dirty)
            draft.submit() // must not throw
        } }
    }

"#;
