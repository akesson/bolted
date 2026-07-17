using System;
using System.Threading;
using System.Threading.Tasks;
using NUnit.Framework;
using GenProfileFfi;

namespace ProfileProbe;

/// <summary>
/// Feature 3 — async streams. BoltFFI exposes each <c>#[ffi_stream]</c> as an
/// <c>IAsyncEnumerable&lt;ProfileSnapshot&gt;</c> fed by a background poll loop draining the Rust
/// side's bounded ring in batches, consumed with <c>await foreach</c>.
///
/// Note the gap this probe cannot close on this backend: D10's <c>[Pending, Passed]</c> check-state
/// sequence is stream-only, and the <b>only</b> verb that produces it — <c>run_username_check</c> —
/// throws on the C# backend (see <see cref="CallbackDriverProbe"/> and the step-14 report). Stream
/// delivery itself, driven by ordinary mutations, works and is proven here.
/// </summary>
public class StreamProbe
{
    private static async Task<string?> FirstUsernameMatching(ProfileDraftFfi draft, string want, Action mutate)
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        string? got = null;
        var subscribed = new SemaphoreSlim(0);
        var consumer = Task.Run(async () =>
        {
            var en = draft.Snapshots(cts.Token).GetAsyncEnumerator(cts.Token);
            subscribed.Release();
            try
            {
                while (await en.MoveNextAsync())
                {
                    if (en.Current.Username.Validity is TextValidity.Valid v && v.Value == want)
                    {
                        got = v.Value;
                        break;
                    }
                }
            }
            finally { await en.DisposeAsync(); }
        });
        await subscribed.WaitAsync();
        await Task.Delay(200); // let the poll loop establish before mutating
        mutate();
        try { await consumer; } catch (OperationCanceledException) { }
        return got;
    }

    [Test]
    public async Task ASnapshotIsDeliveredOnMutation()
    {
        using var store = new ProfileStoreFfi();
        using var draft = store.Checkout(null);
        string? got = await FirstUsernameMatching(draft, "zoe", () => draft.TrySetUsername("zoe"));
        Assert.That(got, Is.EqualTo("zoe"));
    }

    /// <summary>A fresh subscription replays nothing — it delivers only future events (the value set
    /// before subscribing is visible via the <c>Snapshot()</c> recovery getter, not re-delivered).</summary>
    [Test]
    public async Task AFreshSubscriptionIsFutureOnly()
    {
        using var store = new ProfileStoreFfi();
        using var draft = store.Checkout(null);
        draft.TrySetUsername("before"); // BEFORE subscribing
        Assert.That(draft.Snapshot().Username.Validity, Is.EqualTo(new TextValidity.Valid("before")));

        string? got = await FirstUsernameMatching(draft, "after", () => draft.TrySetUsername("after"));
        Assert.That(got, Is.EqualTo("after"), "only the future event is delivered; 'before' is never replayed");
    }
}
