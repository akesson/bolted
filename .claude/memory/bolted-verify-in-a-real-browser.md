---
name: bolted-verify-in-a-real-browser
description: "For bolted's web shell, drive the running app in a headless browser before declaring a behaviour proven — green tests missed real defects the DOM exposed"
metadata:
  node_type: memory
  type: feedback
---

In the bolted repo, `mise run test:web` and the host controller tests both go green without
ever telling you how the app *feels*. Step 04's most valuable findings came from driving the
live app (`mise run serve:web`, then a headless browser via the playwright MCP tools):

- Caret survival under per-keystroke sanitization is only provable with `selectionStart`
  after a **mid-string** insertion — appending to the end passes even when the binding is wrong.
- The F6 UX verdict (a conflicted field edited to equal *theirs* shows a banner where "keep
  mine" and "take theirs" do the same visible thing) is invisible to any assertion.
- Leptos flushes DOM writes one tick after the mutation, so DOM assertions must yield first.
  Core state is already correct synchronously.

**Why:** the step docs ask for *evidence*, and a passing suite is not evidence about a UI.
Kill criteria and §9 questions are decided by observed behaviour.

**How to apply:** after the suite is green, serve the app and exercise the manual protocol in
a browser; use `MutationObserver` (not `setTimeout` polling, which Chrome clamps to ~4 ms) for
latency numbers. Record what you saw in `docs/steps/step-XX-report.md`. See
[[fable-plans-opus-implements]].
