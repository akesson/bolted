// Hand-written — NOT generated. Do not delete when regenerating the suite beside it.
//
// The values-only fixture `ProfileConformanceSuite` is generic over (D28, step 13). It supplies the
// three things the declaration cannot know — a valid raw, a distinct second, a raw that fails tier-1,
// per role — and the tier-2 rule arrangement, whose name and pins live in a `#[bolted::rules]` impl
// body the generator never sees. It makes no judgement (kill criterion 3): every member is a constant.
//
// The constants mirror the Kotlin fixture and `crates/gen-profile/tests/conformance.rs`'s
// `ProfileFixture` exactly — the same roles (primary = name, secondary = email, checked = username) —
// so a failure here, in Kotlin, and in Rust name the same seam from three angles.

import GenProfileFfi

/// The factory the generated suite calls by name; a missing one is a compile error (D25).
func profileConformanceFixture() -> ProfileConformanceFixture { ProfileFixtureValues() }

private struct ProfileFixtureValues: ProfileConformanceFixture {

    func seed() -> ProfileValues {
        ProfileValues(
            username: checkedBase,
            name: primaryBase,
            email: secondaryBase,
            availability: AvailabilityRaw(
                start: PlainDate(year: 2026, month: 1, day: 1),
                end: PlainDate(year: 2026, month: 12, day: 31)
            )
        )
    }

    // PRIMARY = name (PersonName). Four distinct valid texts; `primaryInvalid` trims to empty.
    let primaryBase = "Alice"
    let primaryMine = "My Name"
    let primaryTheirs = "Their Name"
    let primaryOther = "Other Name"
    let primaryInvalid = "   "

    // SECONDARY = email (Email). `secondaryInvalid` has no '@'.
    let secondaryBase = "alice@corp.example"
    let secondaryTheirs = "mine@other.com"
    let secondaryInvalid = "not-an-email"

    // CHECKED = username (Username), guarded by the `username_unique` single-flight check.
    let checkedBase = "alice"
    let checkedMine = "alice2"
    let checkedTheirs = "bravo"

    /// C08: `corporate_email` pins to `email` and fires when `username` is `corp_`-prefixed and the
    /// email is not corporate. Arrange it satisfied — edit `email` dirty while `username` is still
    /// `alice` — then flip it by moving the *unpinned* `username` to `corp_bob` via the canonical.
    func ruleFlip() -> RuleFlip? {
        RuleFlip(
            ruleName: "corporate_email",
            dirtyEdits: [(.email, "bob@other.com")],
            flippedCanonical: seedWith(username: "corp_bob"),
            pins: [.email]
        )
    }

    private func seedWith(username raw: String) -> ProfileValues {
        var v = seed()
        v.username = raw
        return v
    }
}
