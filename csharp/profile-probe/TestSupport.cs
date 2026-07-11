using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Hand-written test support for the C# freeze-contract probe (step 14). The seed constants mirror
/// the Swift/Kotlin fixtures and `crates/gen-profile/tests/conformance.rs` exactly — primary = name,
/// secondary = email, checked = username — so a failure here names the same seam from a third angle.
/// </summary>
internal static class Fixture
{
    // CHECKED = username; PRIMARY = name; SECONDARY = email.
    public const string UsernameBase = "alice";
    public const string UsernameOther = "alice2";
    public const string NameBase = "Alice";
    public const string NameMine = "My Name";
    public const string NameInvalid = "   ";          // trims to empty → tier-1 refusal
    public const string EmailBase = "alice@corp.example";
    public const string EmailInvalid = "not-an-email"; // no '@'

    public static ProfileValues Seed() => new ProfileValues(
        UsernameBase, NameBase, EmailBase,
        new AvailabilityRaw(new PlainDate(2026, 1, 1), new PlainDate(2026, 12, 31)));

    public static ProfileStoreFfi Seeded()
    {
        var store = new ProfileStoreFfi();
        store.ApplyCanonical(Seed());
        return store;
    }
}

/// <summary>Approves every value — the checker C13/C16 would drive to a pass.</summary>
internal sealed class PassingChecker : UsernameChecker
{
    public int Calls;
    public CheckVerdictFfi Check(string value) { Calls++; return CheckVerdictFfi.Pass; }
}
