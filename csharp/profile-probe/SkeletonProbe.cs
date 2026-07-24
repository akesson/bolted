using NUnit.Framework;

namespace ProfileProbe;

/// <summary>
/// The walking skeleton (step 02, milestone 1), backend #3: prove the packed native dylib loads and
/// a call crosses the FFI boundary from <c>dotnet test</c> on this Mac. If this is red, kill
/// criterion 2 is hit and nothing else in the tier is worth running.
/// </summary>
public class SkeletonProbe
{
    [Test]
    public void PingCrossesTheBoundary()
    {
        // Fully qualified: the binding namespace and the top-level class are both `Gen_profile_ffi`.
        string echoed = Gen_profile_ffi.Gen_profile_ffi.Ping("bolted");
        Assert.That(echoed, Does.Contain("bolted"));
    }
}
