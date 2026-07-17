using System.Linq;
using NUnit.Framework;
using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Feature 2 — typed errors (<c>error_style = throwing</c>). A failed setter throws a
/// <c>&lt;Value&gt;ErrorFfiException</c> whose <c>.Error</c> is the decoded discriminated-union DTO;
/// <c>submit</c> throws <c>SubmitErrorFfiException</c> the same way. The error carries key+params
/// data, never a string (the core's <c>ErrorData</c>), so a shell localises it.
/// </summary>
public class TypedErrorProbe
{
    [Test]
    public void AFailedSetterThrowsATypedException()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(null);
        var ex = Assert.Throws<PersonNameErrorFfiException>(() => draft.TrySetName(Fixture.NameInvalid));
        // The refusal is a typed DU value, not a string.
        Assert.That(ex!.Error, Is.Not.Null);
        // And the field records Invalid{raw} — the previous valid value is not silently kept.
        var v = draft.Snapshot().Name.Validity;
        Assert.That(v, Is.InstanceOf<TextValidity.Invalid>());
        Assert.That(((TextValidity.Invalid)v).Raw, Is.EqualTo(Fixture.NameInvalid));
    }

    [Test]
    public void AnInvalidFieldBlocksSubmitWithTypedKeyAndParams()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(null);
        Assert.Throws<PersonNameErrorFfiException>(() => draft.TrySetName(Fixture.NameInvalid));

        var ex = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(ex!.Error, Is.InstanceOf<SubmitErrorFfi.Validation>());
        var report = ((SubmitErrorFfi.Validation)ex.Error).Report;
        var fieldError = report.FieldErrors.Single(fe => fe.Field == ProfileFieldId.Name);
        // key+params data, never a string.
        Assert.That(fieldError.Error.Key, Is.Not.Empty);
        Assert.That(fieldError.Error.Params, Is.Not.Null);
    }
}
