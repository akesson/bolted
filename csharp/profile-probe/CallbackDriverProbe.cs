using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NUnit.Framework;
using Gen_profile_ffi;

namespace ProfileProbe;

/// <summary>
/// Feature 4 — callback traits (capabilities), backend #3. Step 14 hit <b>kill criterion 1</b> here:
/// the one verb that drives the checker, <c>run_username_check</c>, threw
/// <c>MarshalDirectiveException</c> on every call — the BoltFFI C# codegen stamped a struct-returning
/// P/Invoke with <c>[return: MarshalAs(I1)]</c> (the marshalling for a <i>bool</i> return). That is
/// <b>fixed at released 0.28.0</b>: the IR backend moved the <c>Result&lt;bool, DraftClosed&gt;</c>
/// payload onto an <c>out bool</c> parameter (the <c>I1</c> now sits where the type genuinely is bool),
/// and the envelope return is un-attributed. The step-14 tripwire
/// <c>TheCheckDriverIsBrokenOnThisBackend</c> was watched red at 0.28.0 for exactly that reason
/// (<c>Expected: &lt;MarshalDirectiveException&gt; But was: null</c>) and is now deleted; the parked
/// C13/C16/D10/D23 callback probes come alive below.
///
/// These mirror <c>gen-profile-ffi/tests/wrapper.rs</c> at the FFI surface: the driver's bool means
/// "a check <i>ran</i>" (<c>Ok(true)</c>), not the pass/fail verdict — the verdict lands in the
/// snapshot's <c>UsernameCheck</c> state. A declared-absent capability is the sole <c>Ok(false)</c>.
/// </summary>
public class CallbackDriverProbe
{
    [Test]
    public void TheCheckerInterfaceRegistersWithoutError()
    {
        using var store = Fixture.Seeded();
        // Registration goes through the generated vtable bridge at checkout (D34) and does not throw.
        Gen_profile_ffi.ProfileDraftFfi? draft = null;
        Assert.DoesNotThrow(() => draft = store.Checkout(new PassingChecker()));
        draft?.Dispose();
    }

    /// <summary>
    /// C13/C16 — the driver invokes the checker with the parsed value and a passing verdict lands as
    /// <c>Passed</c>. The bool is <c>true</c> because a check ran; the verdict itself is the check
    /// state, not the bool (a Fail would also return <c>true</c> — see the next test).
    /// </summary>
    [Test]
    public void TheCheckerIsInvokedAndAPassingVerdictLands()
    {
        using var store = Fixture.Seeded();
        var checker = new ScriptedChecker(CheckVerdictFfi.Pass);
        using var draft = store.Checkout(checker);
        draft.TrySetUsername("  Grace  "); // the setter sanitizes to "Grace" before the checker sees it

        Assert.That(draft.RunUsernameCheck(), Is.True, "a check ran (the bool is 'ran', not the verdict)");
        Assert.That(checker.Seen, Is.EqualTo(new[] { "Grace" }), "the checker saw the parsed value, not the raw keystrokes");
        Assert.That(draft.Snapshot().UsernameCheck, Is.EqualTo(new CheckStateFfi.Passed()));
    }

    /// <summary>
    /// C13 — a failing verdict lands as <c>Failed</c> carrying the <b>declared</b> <c>failed_key</c>
    /// (<c>username_taken</c>, from <c>#[check(failed_key = …)]</c> in gen-profile, not invented here),
    /// surfaces as a <c>username_unique</c> rule violation, and blocks the submit (C07). Mirrors
    /// <c>a_failed_check_raises_the_declared_key_and_blocks_the_submit</c>.
    /// </summary>
    [Test]
    public void AFailingVerdictLandsWithTheDeclaredKeyAndBlocksSubmit()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(new ScriptedChecker(CheckVerdictFfi.Fail));
        draft.TrySetUsername("taken");

        Assert.That(draft.RunUsernameCheck(), Is.True);

        var check = draft.Snapshot().UsernameCheck;
        Assert.That(check, Is.InstanceOf<CheckStateFfi.Failed>());
        Assert.That(((CheckStateFfi.Failed)check).Error.Key, Is.EqualTo("username_taken"));

        var report = draft.Validate();
        Assert.That(report.RuleErrors.Any(v => v.Rule == "username_unique"), Is.True,
            "C13: a failed verdict is a rule violation pinned to the checked field");
        Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
    }

    /// <summary>
    /// A live draft whose capability was declared absent at checkout (<c>null</c>, D34) is the one
    /// case that answers <c>Ok(false)</c> — the check does not run, the state stays <c>Unchecked</c>.
    /// This pins the bool's meaning distinct from a refusal (below). Mirrors cell 1 of
    /// <c>running_a_check_without_a_checker_is_not_the_same_as_running_it_on_a_corpse</c>.
    /// </summary>
    [Test]
    public void ADeclaredAbsentCheckerIsTheSoleOkFalse()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(null);
        Assert.That(draft.RunUsernameCheck(), Is.False,
            "a live draft with a declared-absent capability is the only Ok(false)");
        Assert.That(draft.Snapshot().UsernameCheck, Is.EqualTo(new CheckStateFfi.Unchecked()));
    }

    /// <summary>
    /// D23 — <c>run_username_check</c> on a released (submitted) draft refuses <b>unconditionally</b>,
    /// throwing <c>DraftClosedFfiException</c>, capability present or declared-absent. Step 14 recorded
    /// this refusal as masked by the codegen bug (it threw <c>MarshalDirectiveException</c> before it
    /// could return the <c>DraftClosed</c> envelope); it is now observable. Mirrors cells 2 and 3 of
    /// the corpse test — the declared-absent corpse is the control that the liveness gate runs ahead
    /// of the no-capability short-circuit.
    /// </summary>
    [Test]
    public void D23_RunUsernameCheckRefusesAReleasedDraft()
    {
        using var store = Fixture.Seeded();

        var withChecker = store.Checkout(new PassingChecker());
        withChecker.Submit(); // releases the core-side draft; the C# object is NOT disposed
        Assert.That(withChecker.IsLive(), Is.False, "submit tombstones the handle (C17)");
        Assert.Throws<DraftClosedFfiException>(() => withChecker.RunUsernameCheck());
        withChecker.Dispose();

        // Control: a declared-absent capability on a corpse still refuses (the liveness gate wins).
        var noChecker = store.Checkout(null);
        noChecker.Submit();
        Assert.Throws<DraftClosedFfiException>(() => noChecker.RunUsernameCheck());
        noChecker.Dispose();
    }

    /// <summary>
    /// The wrapper's hardest-won invariant (step 02): the driver must call the foreign checker with
    /// <b>no store lock held</b>. This checker reaches reentrantly into the very store whose lock the
    /// driver takes; if phase B held it, the call would deadlock. The call is run on a worker task and
    /// bounded by a timeout so a deadlock fails the test instead of hanging the tier. Mirrors
    /// <c>a_reentrant_checker_does_not_deadlock</c>.
    /// </summary>
    [Test]
    public void AReentrantCheckerDoesNotDeadlock()
    {
        using var store = Fixture.Seeded();
        var checker = new ReentrantChecker(store);
        using var draft = store.Checkout(checker);
        draft.TrySetUsername("grace");

        bool ran = false;
        var t = Task.Run(() => ran = draft.RunUsernameCheck());
        Assert.That(t.Wait(TimeSpan.FromSeconds(5)), Is.True,
            "the check completed — the driver dropped the store lock before the outcall");
        Assert.That(ran, Is.True);
        Assert.That(checker.Calls, Is.EqualTo(1), "the reentrant checker actually fired");
        Assert.That(draft.Snapshot().UsernameCheck, Is.EqualTo(new CheckStateFfi.Passed()));
    }

    /// <summary>
    /// D10 (driver-fact scope, v1.11) — the driver emits a <c>Pending</c> snapshot before calling the
    /// checker, then the verdict, so an accepted value is delivered as the sub-sequence
    /// <c>[Pending, Passed]</c> on the draft's own snapshot stream. Step 14 could not observe this
    /// (the driver threw); it comes alive on the fixed <c>run_username_check</c> and the fixed draft
    /// stream (finding 07).
    /// </summary>
    [Test]
    public async Task D10_TheCheckStreamDeliversPendingThenPassed()
    {
        using var store = Fixture.Seeded();
        using var draft = store.Checkout(new PassingChecker());

        var states = await CollectCheckStatesUntilPassed(draft, () =>
        {
            draft.TrySetUsername("grace");
            draft.RunUsernameCheck();
        });

        int pending = states.FindIndex(s => s is CheckStateFfi.Pending);
        Assert.That(pending, Is.GreaterThanOrEqualTo(0), "the driver emitted a Pending");
        Assert.That(states[pending + 1], Is.InstanceOf<CheckStateFfi.Passed>(),
            "Passed immediately follows Pending on the check stream");
    }

    /// <summary>
    /// The <c>fillValid</c> create-flow, alive. A create-flow draft (no canonical) with every field
    /// filled valid is still <b>not committable</b> while its pinned username check is
    /// <c>Unchecked</c> (C16); the very verb that was dead at step 14 — <c>run_username_check</c> — is
    /// what makes it committable. Mirrors <c>ProfileFixture::fill_valid</c> + <c>c07</c>'s "a
    /// create-flow draft, filled, commits". This is why C12/C22's filled-create-flow submits were
    /// blocked on the C# backend before 0.28.0.
    /// </summary>
    [Test]
    public void FillValidCreateFlowIsCommittableOnlyAfterTheCheckRuns()
    {
        using var store = new ProfileStoreFfi(); // no canonical → checkout is a create-flow draft
        using var draft = store.Checkout(new PassingChecker());

        // Every field valid (mirrors ProfileFixture::fill_valid) — but the check has NOT run yet.
        draft.TrySetUsername("carol");
        draft.TrySetName("Carol");
        draft.TrySetEmail("carol@corp.example");
        draft.TrySetAvailability(new AvailabilityRaw(new PlainDate(2026, 5, 1), new PlainDate(2026, 5, 2)));

        // C16: a dirty checked field with an unrun check blocks the commit, all fields valid or not.
        var blocked = Assert.Throws<SubmitErrorFfiException>(() => draft.Submit());
        Assert.That(blocked!.Error, Is.InstanceOf<SubmitErrorFfi.Validation>());

        // Running the check to a pass is the only change — and now the filled draft commits.
        Assert.That(draft.RunUsernameCheck(), Is.True);
        Assert.That(draft.Snapshot().UsernameCheck, Is.EqualTo(new CheckStateFfi.Passed()));
        Assert.DoesNotThrow(() => draft.Submit(), "fill_valid + a passed check makes the create-flow draft committable");
    }

    /// <summary>
    /// Subscribe to the draft's snapshot stream, run <paramref name="drive"/>, and collect the
    /// distinct consecutive <c>UsernameCheck</c> states delivered until a <c>Passed</c> arrives.
    /// </summary>
    private static async Task<List<CheckStateFfi>> CollectCheckStatesUntilPassed(ProfileDraftFfi draft, Action drive)
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        var states = new List<CheckStateFfi>();
        var subscribed = new SemaphoreSlim(0);
        var consumer = Task.Run(async () =>
        {
            var en = draft.Snapshots(cts.Token).GetAsyncEnumerator(cts.Token);
            subscribed.Release();
            try
            {
                while (await en.MoveNextAsync())
                {
                    var s = en.Current.UsernameCheck;
                    if (states.Count == 0 || !states[^1].Equals(s)) states.Add(s);
                    if (s is CheckStateFfi.Passed) break;
                }
            }
            finally { await en.DisposeAsync(); }
        });
        await subscribed.WaitAsync();
        await Task.Delay(200); // let the poll loop establish before driving
        drive();
        try { await consumer; } catch (OperationCanceledException) { }
        return states;
    }
}
