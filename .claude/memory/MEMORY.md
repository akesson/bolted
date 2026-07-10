# Memory index

- [Fable plans, Opus implements](fable-plans-opus-implements.md) — handoff via committed repo docs (CLAUDE.md, ROADMAP, step docs/reports)
- [askama symlinked-CARGO_HOME bug](askama-symlink-cargo-home-bug.md) — upstream state, verified mechanism, one-line fix PR opportunity at askama config.rs:403
- [Verify the web shell in a real browser](bolted-verify-in-a-real-browser.md) — a green suite is not evidence about a UI; drive the running app
- [ART GC probes need a control](art-gc-probes-need-a-control.md) — WeakReference.get() in a poll loop keeps the object alive; use a ReferenceQueue
- [Echo rule: touched, not dirty](echo-rule-predicate-is-touched-not-dirty.md) — sanitization makes a field clean while the user is still typing in it
- [Compose cannot see ViewModel method reads](compose-cannot-see-viewmodel-method-reads.md) — strong skipping + a StateFlow read = a UI that never updates
- [A missing prop_assume asserts the bug](a-missing-prop-assume-asserts-the-bug.md) — the generator never samples the counterexample your precondition forgot to exclude
