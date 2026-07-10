// Hand-written — NOT generated. Do not delete when regenerating the suite beside it.
//
// The values-only fixture `ProfileConformanceSuite` is generic over (D28, step 13). It supplies the
// three things the declaration cannot know — a valid raw, a distinct second, a raw that fails tier-1,
// per role — and the tier-2 rule arrangement, whose name and pins live in a `#[bolted::rules]` impl
// body the generator never sees. It makes no judgement (kill criterion 3): every member is a constant.
//
// The constants mirror `crates/gen-profile/tests/conformance.rs`'s `ProfileFixture` exactly — the same
// roles (primary = name, secondary = email, checked = username) the Rust suite drives — so a failure
// here and a failure there name the same seam from two languages.

package dev.bolted.profileprobe.generated

import com.example.gen_profile_ffi.AvailabilityRaw
import com.example.gen_profile_ffi.PlainDate
import com.example.gen_profile_ffi.ProfileFieldId
import com.example.gen_profile_ffi.ProfileValues

/** The factory the generated suite calls by name; a missing one is a compile error (D25). */
fun profileConformanceFixture(): ProfileConformanceFixture = ProfileFixtureValues

private object ProfileFixtureValues : ProfileConformanceFixture {

    override fun seed(): ProfileValues =
        ProfileValues(
            username = checkedBase,
            name = primaryBase,
            email = secondaryBase,
            availability =
                AvailabilityRaw(
                    start = PlainDate(2026.toUShort(), 1.toUByte(), 1.toUByte()),
                    end = PlainDate(2026.toUShort(), 12.toUByte(), 31.toUByte()),
                ),
        )

    // PRIMARY = name (PersonName). Four distinct valid texts; `primaryInvalid` trims to empty.
    override val primaryBase = "Alice"
    override val primaryMine = "My Name"
    override val primaryTheirs = "Their Name"
    override val primaryOther = "Other Name"
    override val primaryInvalid = "   "

    // SECONDARY = email (Email). `secondaryInvalid` has no '@'.
    override val secondaryBase = "alice@corp.example"
    override val secondaryTheirs = "mine@other.com"
    override val secondaryInvalid = "not-an-email"

    // CHECKED = username (Username), guarded by the `username_unique` single-flight check.
    override val checkedBase = "alice"
    override val checkedMine = "alice2"
    override val checkedTheirs = "bravo"

    /**
     * C08: the `corporate_email` rule pins to `email` and fires when `username` is `corp_`-prefixed and
     * the email is not corporate. Arrange it satisfied — edit `email` dirty while `username` is still
     * `alice` — then flip it by moving the *unpinned* `username` to `corp_bob` via the canonical.
     */
    override fun ruleFlip(): RuleFlip =
        RuleFlip(
            ruleName = "corporate_email",
            dirtyEdits = listOf(ProfileFieldId.EMAIL to "bob@other.com"),
            flippedCanonical = seed().copy(username = "corp_bob"),
            pins = listOf(ProfileFieldId.EMAIL),
        )
}
