#!/usr/bin/env python3
"""Mutation pass for step 09. Each mutation must be caught by at least one named test."""
import subprocess, sys, pathlib, re

ROOT = pathlib.Path("/Users/hakesson/Developer/akesson/worktrees/bolted/clean-pelican/bolted")

# (name, file, old, new, why)
MUTATIONS = [
    ("M1 is_based consults a single field",
     "crates/bolted-macros/src/entity.rs",
     "            fn is_based(&self) -> bool {\n                #(#bases)||*\n            }",
     "            fn is_based(&self) -> bool {\n                #(#bases);*\n            }",
     "C12: a partially-ancestored draft is misjudged create-flow and overwrites the server"),

    ("M2 resolve_take_theirs skips the guard",
     "crates/bolted-macros/src/entity.rs",
     "            fn resolve_take_theirs(&mut self, field: #field_id) {\n                self.bolted_guard(|__d| match field { #(#take_theirs,)* })\n            }",
     "            fn resolve_take_theirs(&mut self, field: #field_id) {\n                let __d = self; match field { #(#take_theirs,)* }\n            }",
     "C13: taking theirs moves the value, so a stale Done(Ok) survives it"),

    ("M3 dirty_fields emits in reverse declaration order",
     "crates/bolted-macros/src/entity.rs",
     "    let dirty = fields.iter().map(|f| {",
     "    let dirty = fields.iter().rev().map(|f| {",
     "declaration order is observable"),

    ("M4 len_chars max is exclusive (off-by-one)",
     "crates/bolted-macros/src/value.rs",
     "            if __len > #max { return Err(#error::TooLong { max: #max, actual: __len }); }\n        },\n        // `&__raw` is `&String`",
     "            if __len > #max + 1 { return Err(#error::TooLong { max: #max, actual: __len }); }\n        },\n        // `&__raw` is `&String`",
     "a 21-character username is accepted"),

    ("M5 rebase skips the guard",
     "crates/bolted-macros/src/entity.rs",
     "                self.bolted_guard(|__d| { #(#rebases)* });",
     "                { let __d = &mut *self; #(#rebases)* };",
     "C13: a rebase that adopts a new canonical moves the value, so the verdict must reset"),

    ("M6 the tier-2 rules are never collected",
     "crates/bolted-macros/src/entity.rs",
     "                report.rule_errors.extend(#rule_set::rules(self));",
     "                let _ = #rule_set::rules(self);",
     "C08: the corporate_email rule never fires"),

    # NB: the first version of this mutation pointed check_pins at fields.first(), which for
    # `Profile` IS `username` — the checked field. It was vacuous, and "survived" for that reason.
    ("M7 check_pins names the wrong field",
     "crates/bolted-macros/src/entity.rs",
     "    let pins = checked.iter().map(|(f, c)| {\n        let (variant, field) = (&c.variant, &f.variant);\n        quote!(#check_id::#variant => #field_id::#field)\n    });",
     "    let last = fields.last().map(|f| f.variant.clone());\n    let pins = checked.iter().map(|(_f, c)| {\n        let (variant, field) = (&c.variant, &last);\n        quote!(#check_id::#variant => #field_id::#field)\n    });",
     "C13/C16: the check endorses a field it does not guard"),

    ("M8 the `lowercase` sanitizer is dropped",
     "crates/bolted-macros/src/value.rs",
     "        Sanitizer::Lowercase => quote!(let __raw = __raw.to_lowercase();),",
     "        Sanitizer::Lowercase => quote!(),",
     "C01: Email's raw roundtrip no longer canonicalizes case"),

    ("M9 commit_gates checks conflicts before orphaned",
     "crates/bolted-core/src/draft.rs",
     "    if matches!(draft.status(), DraftStatus::Orphaned) {\n        return Some(CommitError::Orphaned);\n    }\n    let conflicts = draft.conflicts();\n    if !conflicts.is_empty() {\n        return Some(CommitError::Conflicted { fields: conflicts });\n    }",
     "    let conflicts = draft.conflicts();\n    if !conflicts.is_empty() {\n        return Some(CommitError::Conflicted { fields: conflicts });\n    }\n    if matches!(draft.status(), DraftStatus::Orphaned) {\n        return Some(CommitError::Orphaned);\n    }",
     "C07/C11: an orphaned draft with a conflict reports the wrong typed refusal"),

    ("M10 an unrun check never blocks a dirty field",
     "crates/bolted-core/src/single_flight.rs",
     "            CheckState::Idle if pinned_field_is_dirty => crate::ErrorData::new(required_key),",
     "            CheckState::Idle if false => crate::ErrorData::new(required_key),",
     "C16: a client-side `unique` that was never computed is submitted"),

    ("M11 commit_gates checks validation before conflicts",
     "crates/bolted-core/src/draft.rs",
     "    let conflicts = draft.conflicts();\n    if !conflicts.is_empty() {\n        return Some(CommitError::Conflicted { fields: conflicts });\n    }\n    let report = draft.validate();\n    if !report.is_ok() {\n        return Some(CommitError::Validation(report));\n    }",
     "    let report = draft.validate();\n    if !report.is_ok() {\n        return Some(CommitError::Validation(report));\n    }\n    let conflicts = draft.conflicts();\n    if !conflicts.is_empty() {\n        return Some(CommitError::Conflicted { fields: conflicts });\n    }",
     "C07: a conflicted draft reports validation errors about a value the user has not chosen"),
    ("M12 the checked field's setter loses its guard",
     "crates/bolted-macros/src/entity.rs",
     "        let body = if f.check.is_some() {",
     "        let body = if false {",
     "C13: an edit to the checked field no longer resets its verdict"),
]


def run_tests():
    p = subprocess.run(["cargo", "test", "--workspace", "--no-fail-fast"],
                       cwd=ROOT, capture_output=True, text=True)
    failed = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED", p.stdout, re.M)))
    build_err = "error[" in p.stderr or "error:" in p.stderr
    return failed, build_err, p.stderr


def main():
    results = []
    for name, relpath, old, new, why in MUTATIONS:
        f = ROOT / relpath
        src = f.read_text()
        if old not in src:
            print(f"!! {name}: ANCHOR NOT FOUND in {relpath}", file=sys.stderr)
            results.append((name, None, why, "ANCHOR NOT FOUND"))
            continue
        f.write_text(src.replace(old, new, 1))
        failed, build_err, stderr = run_tests()
        f.write_text(src)  # revert immediately

        if build_err and not failed:
            results.append((name, ["<compile error>"], why, "compile"))
        else:
            results.append((name, failed, why, "test"))
        print(f"== {name}\n   caught by {len(failed)} test(s): {failed[:6]}{' ...' if len(failed)>6 else ''}\n   build_err={build_err}\n")

    print("\n\n===== SUMMARY =====")
    survivors = []
    for name, failed, why, kind in results:
        if failed is None:
            print(f"SKIP     {name}")
        elif not failed and kind != "compile":
            print(f"SURVIVED {name}  <-- {why}")
            survivors.append(name)
        else:
            n = len(failed)
            print(f"caught({n:2}) {name}")
    print(f"\nsurvivors: {len(survivors)}")
    for s in survivors:
        print("  ", s)


if __name__ == "__main__":
    main()
