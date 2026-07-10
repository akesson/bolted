#!/usr/bin/env python3
"""Step 10's mutation pass over `bolted-ffi-gen` and `bolted-ffi`.

    python3 docs/steps/artifacts/step-10-mutations.py [-k SUBSTRING]

There is a trap here that steps 08 and 09 did not have, and it is worth naming.

`crates/gen-*-ffi/src/generated.rs` is committed, and `tests/drift.rs` compares it against what the
generator produces. So **every** mutation of the generator fails the drift test, immediately, for a
reason that says nothing about whether the code is tested. A mutation pass run that way would report
100% caught and mean nothing — the same vacuity that made step 09's M7 a lie.

So each mutation here does the honest thing:

  1. apply the mutation
  2. regenerate (`mise run gen:ffi`), so the committed file matches the *mutant*
  3. assert the regenerated file actually CHANGED — a mutation that changes no output is vacuous,
     and is reported as such rather than as a pass
  4. run the behavioural suite, with the drift tests excluded

A mutation is "caught" only if a test that asserts on behaviour fails. `bolted-ffi` mutations skip
step 2 (nothing to regenerate) but still must change behaviour.

Exit code 0 iff every mutation is caught and none is vacuous. Restores the tree on the way out, and
on Ctrl-C.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[3]
GEN = ROOT / "crates/bolted-ffi-gen/src"
FFI = ROOT / "crates/bolted-ffi/src/lib.rs"
GENERATED = [
    ROOT / "crates/gen-note-ffi/src/generated.rs",
    ROOT / "crates/gen-profile-ffi/src/generated.rs",
]

# The behavioural suite. `--test drift` is deliberately absent: see the docstring.
SUITE = [
    ["cargo", "test", "-q", "-p", "bolted-ffi-gen", "--lib"],
    ["cargo", "test", "-q", "-p", "gen_profile_ffi", "--test", "wrapper"],
]

# A deadlocked mutant hangs rather than fails. A hang IS the observation (step 05 said the same about
# a SIGSEGV), so it is caught — but only if we stop waiting for it.
TIMEOUT_S = 180


@dataclass
class Mutation:
    id: str
    what: str
    path: pathlib.Path
    old: str
    new: str
    regenerate: bool = True


MUTATIONS: list[Mutation] = [
    Mutation(
        "M1",
        "to_field_id maps every core field to the first FFI variant",
        GEN / "dto.rs",
        "match f { #(#core_field::#variants => #id::#variants,)* }",
        "match f { #(#core_field::#variants => { let _ = #id::#variants; #id::#first },)* }",
    ),
    Mutation(
        "M2",
        "the snapshot's any_dirty is always false",
        GEN / "wrapper.rs",
        "any_dirty: !draft.dirty_fields().is_empty(),",
        "any_dirty: false,",
    ),
    Mutation(
        "M3",
        "the snapshot's conflict list comes out reversed",
        GEN / "wrapper.rs",
        "conflicts: draft.conflicts().into_iter().map(to_field_id).collect(),",
        "conflicts: { let mut c: Vec<_> = draft.conflicts().into_iter().map(to_field_id).collect(); c.reverse(); c },",
    ),
    Mutation(
        "M4",
        "D23: a setter on a released draft goes back to silently returning Ok(())",
        GEN / "wrapper.rs",
        "                    let Some(draft) = g.store.draft_mut(self.id) else {\n                        return Err(#closed);\n                    };",
        "                    let Some(draft) = g.store.draft_mut(self.id) else {\n                        let _ = #closed;\n                        return Ok(());\n                    };",
    ),
    Mutation(
        "M5",
        "the foreign checker is called WITH the store lock held (deadlock)",
        GEN / "wrapper.rs",
        "                let begun = {\n                    let mut g = lock(&self.core);\n                    g.store.draft_mut(self.id).map(|draft| {",
        "                let __g_held = lock(&self.core);\n                let begun = {\n                    let mut g = lock(&self.core);\n                    g.store.draft_mut(self.id).map(|draft| {",
    ),
    Mutation(
        "M6",
        "run_*_check reports Ok(true) when no checker is installed",
        GEN / "wrapper.rs",
        "                let Some(checker) = lock(&self.#slot).take() else {\n                    return Ok(false);\n                };",
        "                let Some(checker) = lock(&self.#slot).take() else {\n                    return Ok(true);\n                };",
    ),
    Mutation(
        "M7",
        "a failed verdict raises the rule name instead of the declared failed_key",
        GEN / "wrapper.rs",
        "CheckVerdictFfi::Fail => Err(CoreErrorData::new(#failed_key)),",
        "CheckVerdictFfi::Fail => Err(CoreErrorData::new(\"check_failed\")),",
    ),
    Mutation(
        "M8",
        "the check driver begins the check but never completes it",
        GEN / "wrapper.rs",
        "let _superseded = draft.complete_check(#check_id::#variant, token, verdict);",
        "let _superseded = { let _ = (token, verdict); false };",
    ),
    Mutation(
        "M9",
        "resolve_take_theirs keeps mine",
        GEN / "wrapper.rs",
        "                    if keep_mine {\n                        draft.resolve_keep_mine(core_field);\n                    } else {\n                        draft.resolve_take_theirs(core_field);\n                    }",
        "                    let _ = keep_mine;\n                    draft.resolve_keep_mine(core_field);",
    ),
    Mutation(
        "M10",
        "C18: Drop stops closing the draft in the store",
        GEN / "wrapper.rs",
        "                let mut g = lock(&self.core);\n                g.store.close(self.id);\n                g.producers.remove(&self.id);",
        "                let mut g = lock(&self.core);\n                g.producers.remove(&self.id);",
    ),
    Mutation(
        "M11",
        "the validation report drops its rule errors",
        GEN / "wrapper.rs",
        "                rule_errors: r\n                    .rule_errors\n                    .iter()",
        "                rule_errors: r\n                    .rule_errors\n                    .iter()\n                    .take(0)",
    ),
    Mutation(
        "M12",
        "bolted-ffi: a text field never reports itself dirty",
        FFI,
        "    TextFieldState {\n        validity,\n        sync,\n        dirty: f.is_dirty(),\n    }",
        "    TextFieldState {\n        validity,\n        sync,\n        dirty: false,\n    }",
        regenerate=False,
    ),
    Mutation(
        "M13",
        "bolted-ffi: the checker is asked about the raw text of an invalid field, never the value",
        FFI,
        "        Validity::Valid(v) => v.clone().into_raw(),\n        Validity::Invalid { raw, .. } => raw.clone(),",
        "        Validity::Valid(_) => String::new(),\n        Validity::Invalid { raw, .. } => raw.clone(),",
        regenerate=False,
    ),
    Mutation(
        "M14",
        "bolted-ffi: a pending check projects as Unchecked, so no spinner ever shows",
        FFI,
        "        CheckState::Pending { .. } => CheckStateFfi::Pending,",
        "        CheckState::Pending { .. } => CheckStateFfi::Unchecked,",
        regenerate=False,
    ),
]

# M1 needs the first variant's ident; wire it in without complicating the dataclass.
_M1_PRELUDE = (
    "    let first: &Ident = feature.entity.fields.first().map(|f| &f.variant).expect(\"non-empty\");\n"
)


def run(cmd: list[str], timeout: int | None = None) -> tuple[int, str]:
    try:
        p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return 124, "TIMEOUT"


def snapshot() -> dict[pathlib.Path, str]:
    files = [FFI, *GENERATED] + sorted(GEN.glob("*.rs"))
    return {f: f.read_text() for f in files}


def restore(saved: dict[pathlib.Path, str]) -> None:
    for f, text in saved.items():
        f.write_text(text)


def apply(m: Mutation) -> bool:
    src = m.path.read_text()
    if m.old not in src:
        print(f"    !! anchor not found in {m.path.name} — the mutation is stale")
        return False
    src = src.replace(m.old, m.new, 1)
    if m.id == "M1":
        marker = "pub fn field_id_enum(feature: &Feature) -> TokenStream2 {\n"
        src = src.replace(marker, marker + _M1_PRELUDE, 1)
    m.path.write_text(src)
    return True


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("-k", help="only run mutations whose id or description contains this")
    args = ap.parse_args()

    todo = [m for m in MUTATIONS if not args.k or args.k.lower() in (m.id + m.what).lower()]
    saved = snapshot()
    baseline = {g: g.read_text() for g in GENERATED}

    survivors: list[Mutation] = []
    vacuous: list[Mutation] = []
    try:
        print(f"{len(todo)} mutations; drift tests excluded (they catch everything, vacuously)\n")
        for m in todo:
            print(f"  {m.id}  {m.what}")
            restore(saved)
            if not apply(m):
                survivors.append(m)
                continue

            if m.regenerate:
                rc, out = run(["cargo", "run", "-q", "-p", "bolted-ffi-gen", "--bin", "gen-ffi",
                               "--", "crates/gen-note/src/lib.rs", "gen_note",
                               "crates/gen-note-ffi/src/generated.rs"])
                rc2, out2 = run(["cargo", "run", "-q", "-p", "bolted-ffi-gen", "--bin", "gen-ffi",
                                 "--", "crates/gen-profile/src/lib.rs", "gen_profile",
                                 "crates/gen-profile-ffi/src/generated.rs"])
                if rc or rc2:
                    print("       caught: the mutant generator does not run\n")
                    continue
                if all(g.read_text() == baseline[g] for g in GENERATED):
                    print("       VACUOUS: regenerating produced identical output. "
                          "This mutation asserts nothing.\n")
                    vacuous.append(m)
                    continue

            failed = None
            for cmd in SUITE:
                rc, out = run(cmd, timeout=TIMEOUT_S)
                if rc != 0:
                    failed = "hung (deadlock)" if out == "TIMEOUT" else first_failure(out)
                    break
            if failed:
                print(f"       caught by {failed}\n")
            else:
                print("       *** SURVIVED ***\n")
                survivors.append(m)
    except KeyboardInterrupt:
        print("\ninterrupted")
    finally:
        restore(saved)

    print("-" * 78)
    print(f"{len(todo) - len(survivors) - len(vacuous)} caught, "
          f"{len(vacuous)} vacuous, {len(survivors)} survived")
    for m in vacuous:
        print(f"  VACUOUS  {m.id}  {m.what}")
    for m in survivors:
        print(f"  SURVIVOR {m.id}  {m.what}")
    if survivors or vacuous:
        print("\nA survivor is two hypotheses: the suite is blind, or the mutant is identical to the\n"
              "original. Rule out the second before you write a test for the first.")
    return 1 if (survivors or vacuous) else 0


def first_failure(out: str) -> str:
    for line in out.splitlines():
        if line.startswith("---- ") and " stdout ----" in line:
            return line[5:].split(" stdout")[0]
        if line.startswith("test ") and line.endswith("FAILED"):
            return line[5:-9].strip()
    for line in out.splitlines():
        if line.startswith("error"):
            return line[:70]
    return "the suite"


if __name__ == "__main__":
    sys.exit(main())
