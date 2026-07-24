// Hand-written — NOT generated. Do not delete when regenerating the suite beside it.
//
// The values-only fixture `ProfileConformanceSuite` is generic over (D28, step 13/29). It supplies the
// three things the declaration cannot know — a valid raw, a distinct second, a raw that fails tier-1,
// per role — and the tier-2 rule arrangement, whose name and pins live in a `#[bolted::rules]` impl
// body the generator never sees. It makes no judgement (kill criterion 3): every member is a constant.
//
// The constants mirror the Kotlin/Swift fixtures and `crates/gen-profile/tests/conformance.rs`'s
// `ProfileFixture` exactly — the same roles (primary = name, secondary = email, checked = username) —
// so a failure here and a failure there name the same seam from a fourth angle.

using Gen_profile_ffi;

namespace ProfileProbe.Generated;

/// <summary>The factory the generated suite calls by name; a missing one is a compile error (D25).</summary>
internal static class ProfileConformanceFixtureFactory
{
    public static ProfileConformanceFixture Create() => new ProfileFixtureValues();
}

internal sealed class ProfileFixtureValues : ProfileConformanceFixture
{
    public ProfileValues Seed() => new ProfileValues(
        CheckedBase, PrimaryBase, SecondaryBase,
        new AvailabilityRaw(new PlainDate(2026, 1, 1), new PlainDate(2026, 12, 31)));

    // PRIMARY = name (PersonName). Four distinct valid texts; PrimaryInvalid trims to empty.
    public string PrimaryBase => "Alice";
    public string PrimaryMine => "My Name";
    public string PrimaryTheirs => "Their Name";
    public string PrimaryOther => "Other Name";
    public string PrimaryInvalid => "   ";

    // SECONDARY = email (Email). SecondaryInvalid has no '@'.
    public string SecondaryBase => "alice@corp.example";
    public string SecondaryTheirs => "mine@other.com";
    public string SecondaryInvalid => "not-an-email";

    // CHECKED = username (Username), guarded by the username_unique single-flight check.
    public string CheckedBase => "alice";
    public string CheckedMine => "alice2";
    public string CheckedTheirs => "bravo";

    /// <summary>
    /// C08: the corporate_email rule pins to email and fires when username is corp_-prefixed and the
    /// email is not corporate. Arrange it satisfied — edit email dirty while username is still alice —
    /// then flip it by moving the unpinned username to corp_bob via the canonical.
    /// </summary>
    public RuleFlip? RuleFlip() => new RuleFlip(
        "corporate_email",
        new[] { (ProfileFieldId.Email, "bob@other.com") },
        Seed() with { Username = "corp_bob" },
        new[] { ProfileFieldId.Email });
}
