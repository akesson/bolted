using System;
using System.Runtime.CompilerServices;
using NUnit.Framework;
using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Handle lifetime on backend #3 — the findings that feed the ARCHITECTURE §6 / D26 design pass.
/// The probe records runtime truth; it does <b>not</b> amend the frozen documents (that is a design
/// session's job, per the step doc).
/// </summary>
public class LifecycleProbe
{
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference CheckoutAndAbandon(ProfileStoreFfi store)
    {
        var d = store.Checkout();     // entity-backed, deliberately never disposed
        return new WeakReference(d);   // no strong reference escapes this frame
    }

    /// <summary>
    /// THE finding. ARCHITECTURE §6 says "Kotlin / C#: <c>close()</c> only, the GC never frees the
    /// Rust draft." On this backend that is <b>wrong for C#</b>: <c>ProfileDraftFfi</c> has a
    /// finalizer (<c>~ProfileDraftFfi() =&gt; Dispose()</c>), so a forgotten, undisposed draft is
    /// reclaimed by the GC, and its finalizer reaches the store-side close — the live-draft count
    /// falls without any explicit <c>Dispose</c>. This is D26's recorded revisit condition met
    /// ("a Cleaner inside bindgen, where the CAS makes it safe"). A still-referenced control draft
    /// proves the GC was selective (it collected the abandoned one, not the control), per the
    /// ART GC-probe lesson that a probe without a control measures nothing.
    /// </summary>
    [Test]
    public void AForgottenDraftIsReclaimedByTheFinalizer_Section6IsWrongForCSharp()
    {
        using var store = Fixture.Seeded();
        using var control = store.Checkout();
        var controlWeak = new WeakReference(control);
        Assert.That(store.LiveDraftCount(), Is.EqualTo(1u), "the control is checked out");

        var abandonedWeak = CheckoutAndAbandon(store);
        Assert.That(store.LiveDraftCount(), Is.EqualTo(2u), "the abandoned draft is live too");

        for (int i = 0; i < 5 && abandonedWeak.IsAlive; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }

        Assert.That(abandonedWeak.IsAlive, Is.False, "the abandoned, undisposed draft was collected");
        Assert.That(controlWeak.IsAlive, Is.True, "the referenced control was NOT collected — the GC was selective");
        Assert.That(store.LiveDraftCount(), Is.EqualTo(1u),
            "the finalizer reached the store-side close: the GC freed the Rust draft (§6's C# row is wrong)");
        GC.KeepAlive(control);
    }

    /// <summary>
    /// D26 leak-freedom, C#-shaped, and deliberately meaningful <b>despite</b> the finalizer above:
    /// deterministic teardown via <c>Dispose</c> returns C22's counts to baseline, and the assertion
    /// runs <b>before any collection can occur</b> — so a finalizer that would eventually absolve a
    /// forgotten <c>Dispose</c> is not what makes this green. Determinism, not the GC, is the contract.
    /// </summary>
    [Test]
    public void DeterministicDisposeReturnsCountsToBaseline_NoGCInvolved()
    {
        using var store = Fixture.Seeded();
        var a = store.Checkout();
        var b = store.Checkout();
        Assert.That(store.LiveDraftCount(), Is.EqualTo(2u));
        Assert.That(store.RebasingDraftCount(), Is.EqualTo(2u));

        a.Dispose();
        b.Dispose();
        // No GC.Collect here, on purpose: the count must be back to baseline the instant Dispose runs.
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u), "Dispose is deterministic release");
        Assert.That(store.RebasingDraftCount(), Is.EqualTo(0u));
    }

    /// <summary>
    /// D23 — a mutating verb on a released (submitted) draft is a typed refusal, and observers stay
    /// total. <c>Assert.Throws</c> is itself the positive control the step-11 trap demands: a silent
    /// no-op (swallowed refusal) would make it fail. The setter refuses via its value-error DU's
    /// <c>DraftClosed</c> case; resolve refuses via <c>DraftClosedFfiException</c>; a second submit is
    /// <c>AlreadySubmitted</c>. (<c>run_username_check</c>'s D23 refusal is masked by the same
    /// codegen bug that breaks feature 4 — it throws <c>MarshalDirectiveException</c>, not
    /// <c>DraftClosed</c>; recorded in the report, not asserted as a contract here.)
    /// </summary>
    [Test]
    public void D23_MutatorsOnAReleasedDraftAreTypedRefusals_ObserversTotal()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout();
        draft.Submit(); // releases the core-side draft; the C# object is NOT disposed
        Assert.That(draft.IsLive(), Is.False, "submit tombstones the handle (C17)");

        var setterEx = Assert.Throws<PersonNameErrorFfiException>(() => draft.TrySetName("x"));
        Assert.That(setterEx!.Error, Is.InstanceOf<PersonNameErrorFfi.DraftClosed>());

        Assert.Throws<DraftClosedFfiException>(() => draft.ResolveTakeTheirs(ProfileFieldId.Name));

        var submitEx = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(submitEx!.Error, Is.InstanceOf<SubmitErrorFfi.AlreadySubmitted>());

        // Observers are total on a tombstone — they must never throw.
        Assert.DoesNotThrow(() => draft.Snapshot());
        Assert.DoesNotThrow(() => draft.Validate());
    }
}
