using System;
using NUnit.Framework;
using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Feature 1 — classes with methods, backend #3. Store and draft are <c>IDisposable</c> handle
/// classes; every verb sits behind <c>ThrowIfDisposed()</c>. This probe confirms the surface runs,
/// that <c>Dispose</c> is idempotent and <c>using</c>-friendly, and — the step-05 H2 question — that
/// use-after-dispose is a <b>typed</b> refusal (<c>ObjectDisposedException</c>), not silent UB.
/// </summary>
public class ClassHandleProbe
{
    [Test]
    public void StoreAndDraftMethodsRun()
    {
        using var store = Fixture.Seeded();
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u));
        using var draft = store.Checkout();
        Assert.That(store.LiveDraftCount(), Is.EqualTo(1u));
        Assert.That(draft.IsLive(), Is.True);
        draft.TrySetName(Fixture.NameMine);
        var snap = draft.Snapshot();
        Assert.That(snap.Name.Validity, Is.EqualTo(new TextValidity.Valid(Fixture.NameMine)));
        Assert.That(snap.Name.Dirty, Is.True);
    }

    [Test]
    public void UsingReleasesTheDraft()
    {
        using var store = Fixture.Seeded();
        using (var draft = store.Checkout())
        {
            Assert.That(store.LiveDraftCount(), Is.EqualTo(1u));
            _ = draft;
        } // Dispose() at scope exit → the Rust draft is closed (C18)
        Assert.That(store.LiveDraftCount(), Is.EqualTo(0u), "using disposes → store-side close");
    }

    [Test]
    public void DisposeIsIdempotent()
    {
        var store = Fixture.Seeded();
        var draft = store.Checkout();
        draft.Dispose();
        Assert.DoesNotThrow(() => { draft.Dispose(); draft.Dispose(); },
            "Interlocked.Exchange guard makes Dispose idempotent, even on an id already gone (C18)");
        store.Dispose();
    }

    /// <summary>
    /// H2, re-asked on backend #3. On Android the raw hazard was a dangling-pointer dereference
    /// (silent UB). On C# a disposed handle zeroes its pointer and every verb throws
    /// <c>ObjectDisposedException</c> before any native call — the hazard is a typed refusal here, so
    /// the step-05 H2 "silent aliasing" hazard does not exist on this backend via the Dispose path.
    /// </summary>
    [Test]
    public void UseAfterDisposeIsTyped_NotUB()
    {
        var store = Fixture.Seeded();
        var draft = store.Checkout();
        draft.Dispose();
        Assert.Throws<ObjectDisposedException>(() => draft.TrySetName("x"));
        Assert.Throws<ObjectDisposedException>(() => draft.Snapshot());
        Assert.Throws<ObjectDisposedException>(() => draft.Submit());

        store.Dispose();
        Assert.Throws<ObjectDisposedException>(() => store.LiveDraftCount());
        Assert.Throws<ObjectDisposedException>(() => store.Checkout());
    }
}
