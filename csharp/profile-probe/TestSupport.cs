using System.Collections.Generic;
using Gen_profile_ffi;

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

/// <summary>
/// A scripted checker that answers a fixed verdict and records the values it was asked about — the C#
/// analogue of <c>Scripted</c> in <c>gen-profile-ffi/tests/wrapper.rs</c>. The recorded values prove
/// the checker saw the <i>parsed</i> value (sanitizer ran first), and <see cref="Seen"/> is the
/// positive control that the callback actually fired.
/// </summary>
internal sealed class ScriptedChecker : UsernameChecker
{
    private readonly CheckVerdictFfi _verdict;
    public readonly List<string> Seen = new();
    public ScriptedChecker(CheckVerdictFfi verdict) { _verdict = verdict; }
    public CheckVerdictFfi Check(string value) { Seen.Add(value); return _verdict; }
}

/// <summary>
/// A checker that reaches reentrantly back into the very store whose lock the driver takes — the C#
/// analogue of <c>Nosy</c> in <c>wrapper.rs</c>. If the driver invoked this callback while holding the
/// store lock, both calls below would deadlock; that they return is the whole point.
/// </summary>
internal sealed class ReentrantChecker : UsernameChecker
{
    private readonly ProfileStoreFfi _store;
    public int Calls;
    public ReentrantChecker(ProfileStoreFfi store) { _store = store; }
    public CheckVerdictFfi Check(string value)
    {
        Calls++;
        _ = _store.LiveDraftCount();
        _ = _store.Canonical();
        return CheckVerdictFfi.Pass;
    }
}
