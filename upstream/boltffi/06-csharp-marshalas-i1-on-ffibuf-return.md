# C# backend stamps `[return: MarshalAs(I1)]` on an `FfiBuf`-returning P/Invoke → every call throws

**Reported against:** boltffi 0.27.3, **still present at 0.27.5** · **Severity:** high (a whole feature
— callbacks — is unusable on the C# backend) · **Disposition at 0.27.5: ALIVE (reproduces).**

> **Upstream status (2026-07-15):** filed as fix PR
> [boltffi/boltffi#662](https://github.com/boltffi/boltffi/pull/662), **closed without merge —
> resolved**. The C# backend is being rewritten on the IR pipeline (#654), which derives
> `return_marshal_i1` alongside the native return type and does not have the bug (verified by
> generating our demo through #654 and calling it from .NET). Maintainer (mhedgpeth) confirmed it
> works on #654 and pulled our demo tests in there; the fix ships when #654 merges. Our tripwire
> (`mise run test:csharp`) tells us when a release actually clears it.

## Summary

For an `#[export]` method returning `Result<bool, E>` (error_style = throwing), the C# backend emits a
P/Invoke that returns the `FfiBuf` wire envelope but tags it with `[return: MarshalAs(UnmanagedType.I1)]`
— the marshalling directive for a **`bool`** return. `MarshalAs(I1)` on a struct return is invalid on
every .NET runtime, so the call throws `System.Runtime.InteropServices.MarshalDirectiveException`
before the method body runs. The backend appears to key the attribute off the Rust return's **bool
payload** rather than the **`FfiBuf` wire type**.

In this repo the one such verb is `run_username_check` (the async single-flight check driver), so the
entire callback capability (feature 4) cannot be exercised end to end on C#. This is what stopped the
step-14 C# port on its kill criterion.

## Repro (this repo: `crates/gen-profile-ffi`, `csharp/profile-probe`)

```
mise run test:csharp
```

`CallbackDriverProbe.TheCheckDriverIsBrokenOnThisBackend` asserts the break
(`Assert.Throws<MarshalDirectiveException>(() => draft.RunUsernameCheck())`) and **passes** — i.e. the
throw still happens.

## The smoking gun — fresh 0.27.5-generated source

`crates/gen-profile-ffi/dist/csharp/src/GenProfileFfi.cs`:

```csharp
[DllImport(LibName, EntryPoint = "boltffi_profile_draft_ffi_run_username_check")]
[return: MarshalAs(UnmanagedType.I1)]                         // ← bool-return marshalling …
internal static extern FfiBuf ProfileDraftFfiRunUsernameCheck(IntPtr self);   // … on an FfiBuf struct return
```

Contrast, three lines down — the same `FfiBuf` return, correctly with **no** `MarshalAs`:

```csharp
[DllImport(LibName, EntryPoint = "boltffi_profile_draft_ffi_validate")]
internal static extern FfiBuf ProfileDraftFfiValidate(IntPtr self);
```

`is_live` (a genuinely `bool`-returning export) *correctly* gets `[return: MarshalAs(I1)]`. Only the
one `Result<bool, _>` verb wrongly inherits it. **Byte-identical between 0.27.3 and 0.27.5** (the C#
generated surface did not change across the bump).

## Expected

Emit `[return: MarshalAs(UnmanagedType.I1)]` **only** when the P/Invoke's return type is `bool`. For a
verb whose wire return is `FfiBuf` (any `Result<_, _>` / value-returning export), emit no return
marshalling directive — exactly as the C# backend already does for `Validate`, `Submit`, `TrySet*`, etc.

## Impact / blast radius

Every C13/C16 callback path, `[Pending, Passed]` verdict streaming, the reentrant-checker path, and the
create-flow `fillValid` check are unreachable on C#. Not fixable from the consumer side (the defect is
in generated `dist/` we do not edit).

## Acceptance test

On the C# backend, calling a `Result<bool, E>`-returning `#[export]` method returns its value (or
throws the typed error) — it does not throw `MarshalDirectiveException` at the P/Invoke boundary.
