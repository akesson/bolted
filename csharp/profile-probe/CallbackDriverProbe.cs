using System.Runtime.InteropServices;
using NUnit.Framework;
using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Feature 4 — callback traits (capabilities), backend #3. This is where step 14 hit
/// <b>kill criterion 1</b>: one of the four load-bearing features is broken on the C# backend at
/// runtime. The <c>UsernameChecker</c> interface + vtable bridge are generated idiomatically and
/// registration (since D34, an explicit <c>Checkout</c> argument) succeeds without error — but the
/// <b>only</b> verb that drives the checker, <c>run_username_check</c>, throws on every call.
///
/// Root cause (a BoltFFI 0.27.3 C# codegen bug, in generated <c>dist/</c> we do not edit):
/// <c>run_*_check</c> is the surface's one <c>Result&lt;bool, DraftClosed&gt;</c>-returning verb. Its
/// wire return is the <c>FfiBuf</c> envelope (read here via <c>WireReader</c>), but the backend
/// stamped the P/Invoke with <c>[return: MarshalAs(UnmanagedType.I1)]</c> — the marshalling for a
/// <i>bool</i> return, which it confused with the Rust return's bool payload. <c>MarshalAs(I1)</c> on
/// a struct return is invalid C# on every .NET runtime, so the call throws
/// <c>MarshalDirectiveException</c> before any checker is invoked. Details in the step-14 report.
///
/// These tests assert the CURRENT (broken) behaviour so the probe is an honest, green record of it —
/// exactly the Android @HazardProbe stance, where the throw IS the observation. When BoltFFI fixes
/// the attribute, <see cref="TheCheckDriverIsBrokenOnThisBackend"/> goes red: that red is the signal
/// to delete this file and emit the real C13/C16 callback tests (revisit condition).
/// </summary>
public class CallbackDriverProbe
{
    [Test]
    public void TheCheckerInterfaceRegistersWithoutError()
    {
        using var store = Fixture.Seeded();
        // Registration goes through the generated vtable bridge at checkout (D34) and does not throw.
        GenProfileFfi.ProfileDraftFfi? draft = null;
        Assert.DoesNotThrow(() => draft = store.Checkout(new PassingChecker()));
        draft?.Dispose();
    }

    [Test]
    public void TheCheckDriverIsBrokenOnThisBackend()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(new PassingChecker());
        draft.TrySetUsername(Fixture.UsernameOther);

        // The finding: run_*_check cannot be called. It throws at the P/Invoke return marshalling,
        // independent of the checker (proven separately with no checker set). This is kill
        // criterion 1 — feature 4 (callbacks) cannot be exercised end to end on the C# backend.
        Assert.Throws<MarshalDirectiveException>(() => draft.RunUsernameCheck());
    }
}
