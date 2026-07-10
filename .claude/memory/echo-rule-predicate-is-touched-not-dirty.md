---
name: echo-rule-predicate-is-touched-not-dirty
description: "Bolted's echo rule protects a focused control that was TYPED INTO, not one that is `dirty` — sanitization can make a field clean while the user is still typing in it"
metadata:
  node_type: memory
  type: project
---

A shell must never repaint a focused text control the user has typed into. The tempting predicate —
`focused && field.is_dirty()` — is **wrong**, and step 06 froze it that way before a test caught it.

Counterexample: base value `"alice"`, user focuses the field and types `"  alice  "`. The core trims,
so the value never moved: the field is **clean** while the control holds live keystrokes. Any external
refresh (a rebase on an *unrelated* field triggers a full buffer sync) then repaints it, eats the
spaces and jumps the caret — exactly the defect the echo rule exists to prevent.

The shipped predicate is **`focused && typed-into since the core last wrote this buffer`** — one
shell-local `bool`, cleared on focus, on blur, and whenever the shell writes the buffer from the core.
`dirty` and `touched` agree in every other reachable state.

**Why:** `dirty` is value-based by design (ARCHITECTURE §8, revert-for-free), and value-based dirtiness
deliberately cannot see an edit that sanitizes back to the base value. The echo rule is about the
*control's text*, not the *field's value*.

**How to apply:** `touched` here is shell-local presentation state about a text box, which is fine —
it is *not* the core-side `touched`/`visible_errors` flag ARCHITECTURE §8 rejects. See
`ProfileController::focused_touched` (Rust) and `ProfileViewModel.focusedTouched` (Swift), each with a
regression test named `..._sanitizes_back_to_base_still_keeps_its_text`. Same reflex as
[[art-gc-probes-need-a-control]]: the decision was plausible, agreed, and wrong until a test ran.
