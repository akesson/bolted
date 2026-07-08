# Bolted — Vision

**Bolted is everything you bolt around [BoltFFI](https://boltffi.dev): a compile-time-verified
application framework for building apps with one shared Rust core, fully native shells on
Windows, macOS, Linux, Android, and iOS — and a first-class web target, where a Rust web app
(Dioxus, Leptos, Silkenweb, …) consumes the same core as a plain crate compiled to WASM. The
web target means the browser: Bolted never puts the WASM build inside a native webview.**

BoltFFI solves the hardest technical problem of the core-in-Rust architecture — the language
boundary — and deliberately solves nothing else. Bolted's job is everything around that boundary:
project structure, platform capabilities, state flow, build orchestration, and above all
*verification*. The founding rule is simple:

> **Every piece of glue between the Rust core and a platform shell must be either checked by a
> compiler or checked by the build. Glue that can only fail at runtime is out of scope.**

---

## 1. The problem

The core-in-Rust architecture is proven: write business logic, state, and persistence once in
Rust; keep the UI true to each platform (SwiftUI, Compose, WinUI — and on the web, a Rust web
framework such as Leptos or Dioxus). Teams like 1Password have shipped on this model for years. BoltFFI now makes the boundary itself fast and idiomatic —
`#[data]` and `#[export]` in, XCFrameworks / AARs / NuGet packages out.

But the boundary is perhaps 20% of what a team actually needs. The remaining 80% is reinvented,
badly, by every project:

- **No canonical shape.** Where does the core crate live? Where do six platform shells live? How
  do generated bindings flow into each? Every team invents a layout and a pile of shell scripts.
- **The reverse direction is lawless.** The core inevitably needs things from the platform —
  secure storage, file paths, notifications, connectivity, background execution. This gets wired
  as ad-hoc callbacks with stringly-typed keys, and a missing implementation is discovered when a
  user taps the button.
- **Drift is silent.** Someone changes a `#[data]` struct, forgets to regenerate or forgets one
  of six shells, and the failure surfaces at runtime on the one platform nobody tested.
- **Six targets means six toolchains.** Rust + Xcode + NDK + JDK + .NET + wasm32/trunk is an
  environment-setup wiki page that is wrong within a month.
- **No state discipline.** "The core owns the state, the UI observes it" is the right pattern,
  but nothing enforces it, so logic leaks into ViewModels and diverges per platform.

None of these are FFI problems, so BoltFFI rightly won't solve them. They are *framework*
problems.

## 2. The three bets

1. **BoltFFI is the boundary.** Fast enough to stop designing around FFI cost (chatty APIs become
   acceptable), idiomatic enough that generated bindings feel native (async/await in Swift,
   coroutines in Kotlin, Tasks in C#). Bolted couples to it *narrowly* — only through its
   annotations and CLI — so that the boundary layer remains a replaceable part, not a load-bearing
   wall in every module.
2. **Platform-true shells, never shared UI.** Bolted will never ship a cross-platform UI
   toolkit, a webview, or a widget abstraction. Each native target gets the UI technology native
   *to it*: SwiftUI, Compose, WinUI. On the web the shell is a **Rust** web app — Dioxus,
   Leptos, Silkenweb, or any wasm-targeting Rust framework — consuming the core as a plain
   crate: no FFI, no TypeScript bindings, one compiler seeing UI and core together. The WASM
   build exists for the browser and only the browser: Bolted never embeds it in a native webview
   to imitate a desktop or mobile app, and frameworks that offer webview modes (e.g. Dioxus
   desktop) are used in their browser mode only. Bolted abstracts the plumbing beneath the
   shell, not the platform identity above it.
3. **mise is the single entry point.** One tool pins toolchains, defines tasks, and sets
   environment — identically on every laptop and in CI. `mise run <verb>` is the only interface a
   developer or pipeline needs to know.

## 3. The Prime Directive: the verification ladder

Every Bolted feature must justify itself on the highest rung it can reach. From strongest to
weakest:

| Rung | Guarantee | Examples |
|------|-----------|----------|
| **1. Proven by rustc** | Wrong code does not compile | Capability traits, typed state, exhaustive error enums, typestate APIs |
| **2. Generated, then proven by the platform compiler** | Wrong glue does not compile in Swift/Kotlin/C# | BoltFFI bindings, generated ViewModel observers, generated capability stubs |
| **3. Verified at build time** | Wrong setup fails `mise run check` with an actionable warning or error | Binding drift, capability coverage, FFI semver, environment doctor |
| **4. Checked at runtime** | Failure is a typed `Result`, never a surprise | OS permission denials, and nothing else |

Rung 4 is **forbidden for framework mechanics**. If a proposed feature can only work via runtime
discovery, reflection, or stringly-typed lookup, it does not enter the perimeter — it either gets
redesigned to reach rung 3 or it stays out. Where the operating system itself forces runtime
failure (permissions, hardware availability), the failure must surface as an ordinary typed error
in the core's API, never as a panic or a platform exception.

Build-time checks are tiered: everything is a hard error in CI; locally, checks distinguish
**deny** (drift, missing capability implementation) from **warn** (perf lints, dead exports), and
the tiers are configurable per project.

## 4. Architecture

```
┌─ SwiftUI ─┐ ┌─ Compose ─┐ ┌─ WinUI 3 ─┐ ┌─ Linux ───┐ ┌─ Browser ────────────┐
│  Swift    │ │  Kotlin   │ │    C#     │ │ Rust UI   │ │ Rust: Leptos/Dioxus… │
└─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └──────────┬───────────┘
      │             │             │             │                  │
  BoltFFI bindings + generated ViewModels       plain Rust crate dependency
      │       (rung 2)            │             │  zero FFI (rung 1)
┌─────┴─────────────┴─────────────┴─────────────┴──────────────────┴───────────┐
│                             Rust core (one crate)                            │
│            app logic · typed state · persistence · error taxonomy            │
│                                                                              │
│            capability traits: SecureStore, Paths, Notifier, Net…             │
└──────────────────────────────────────┬───────────────────────────────────────┘
                                       │  implemented per platform (Rust on the web),
                                       │  coverage verified by `mise run check`  ← rung 3
```

Two directions cross the boundary, and Bolted owns the pattern for both:

- **Outbound (core → shell):** the core exposes its API and emits **typed state streams**
  (BoltFFI async streams). Bolted defines the unidirectional data flow: shells send intents in,
  observe state out, and hold no business logic. For the FFI shells (Swift, Kotlin, C#), thin
  observer/ViewModel glue is generated so it is compiler-checked, not hand-maintained; Rust
  shells (web, Linux-native) subscribe to the same streams directly, no codegen involved.
- **Inbound (shell → core): the capability system.** The core declares what it needs from the
  platform as Rust traits, exported through BoltFFI's callback-trait mechanism. Each platform
  shell provides implementations in its own language — Swift, Kotlin, C#, or plain Rust against
  browser APIs on the web. The build verifies the matrix: **every
  declared capability × every enabled target = an implementation, or the build fails.** A missing
  keychain implementation on Windows is a build error on Monday, not a crash report on Friday.
  The matrix also supports explicit opt-outs: the browser sandbox cannot implement every
  capability, so a target may declare a capability *unsupported* — a compile-time-visible fact
  the core must handle in its types, never a silent gap.

## 5. The perimeter

Scope is organized as rings. Inner rings are the framework; outer rings are optional batteries
that obey the same verification ladder. Anything not listed is outside until it proves it can
live on rungs 1–3.

### Ring 0 — Foundation
- **Scaffolding:** `bolted new` produces the canonical workspace — core crate, per-platform shell
  projects, generated-bindings directories, CI config — that all tooling can rely on.
- **mise standard tasks:** the fixed verb set every Bolted project shares:
  `mise run doctor · check · build:<target> · test · pack · release`. Platform build systems
  (xcodebuild, Gradle, dotnet) are invoked *through* these tasks, never beside them.
- **Toolchain pinning + doctor:** Rust, JDK, Node, .NET, Python pinned via mise; things mise
  cannot manage (Xcode, NDK, Windows SDK) are *verified* by `doctor` with exact, actionable
  warnings ("Xcode 16.2 required, 15.4 found — run …").

### Ring 1 — The contract
- **Capability system:** trait declaration, generated native stubs, per-target implementation
  registry, build-time coverage matrix.
- **State pattern:** core-owned state, typed intents in, typed state streams out; generated
  observer glue for the FFI shells, direct Rust subscription for the Rust shells. One pattern,
  all six targets.
- **Error taxonomy:** a single error discipline from core to shell — Rust error enums that arrive
  as native exceptions/Results with their structure intact, plus a lint that forbids
  `panic!`/`unwrap` on exported paths.

### Ring 2 — The verification harness
The heart of the framework: `mise run check` runs a growing catalog of analyses, each emitting
build-time warnings or errors:
- **Binding drift:** regenerate bindings, diff against committed/consumed ones; any mismatch
  fails.
- **FFI surface compatibility:** snapshot the exported API; breaking changes require an explicit
  version bump — a breaking change can never land silently.
- **Capability coverage:** the matrix check described above.
- **Dead-export detection:** exported symbols no shell consumes are flagged.
- **Performance lints:** flag type shapes that defeat BoltFFI's zero-copy path (e.g. nested
  collections or string-heavy structs on hot APIs) so the cost is visible when the type is
  written, not when it's profiled.
- **WASM size budget:** the web bundle's compiled size is checked against a declared budget —
  the dependency that doubles the download is a warning at the commit that adds it.
- **Contract tests:** generated round-trip tests per FFI language — every exported type crosses
  the boundary and back in CI on every FFI target. (Rust shells need none: rustc already sees
  both sides.)
- **Doctor:** the environment checks from Ring 0, runnable anywhere.

### Ring 3 — Batteries (optional, same rules)
- **Persistence:** SQLite in the core with compile-time-checked queries and migrations verified
  at build time.
- **i18n:** message catalogs compiled to typed keys — a missing translation or wrong placeholder
  is a build warning, not a runtime blank.
- **Config:** typed, environment-aware configuration resolved at build time.
- **Observability:** `tracing` in the core, bridged by generated glue to os_log / logcat / ETW /
  journald / structured browser-console output.

## 6. Outside the perimeter — permanently

- **No shared UI framework.** No widgets, no webview, no declarative-UI DSL. Ever. The web
  target is not a loophole: the WASM build runs in real browsers only, never inside a native
  window.
- **No runtime.** No interpreter, no reflection, no dynamic plugin loading, no service locator.
- **No hiding the platforms.** Xcode and Android Studio remain first-class; a Bolted shell is a
  normal native project a platform developer recognizes instantly.
- **No second build entry point.** No top-level Makefiles/justfiles beside mise.
- **Not a bindings generator.** Boundary mechanics belong to BoltFFI; Bolted contributes
  upstream rather than wrapping or forking codegen.
- **No networking/HTTP opinion** in Rings 0–2. The core is free to use any crate; a battery may
  come later if it can reach rung 3.

## 7. Platform matrix

| Target | Shell | UI | Delivery | Path maturity |
|--------|-------|-----|----------|---------------|
| iOS / macOS | Swift | SwiftUI | XCFramework + SPM | Strongest — BoltFFI's best-trodden path |
| Android | Kotlin | Compose | jniLibs / AAR | Strong |
| Windows | C# | WinUI 3 | NuGet | Good, less community precedent |
| Linux | Rust (direct) or C# | Slint/egui/GTK or Avalonia | Flatpak/deb | Weakest FFI story — but Linux can consume the core as a plain crate with **zero FFI**, which Bolted treats as a feature, not a gap |
| Web | Rust (direct) | Dioxus / Leptos / Silkenweb | Static WASM bundle (trunk / framework CLI) | **Zero FFI** — the core is a plain crate dependency; the browser sandbox limits which capabilities exist |

The web shell is **Rust**: Bolted deliberately does not use BoltFFI's WASM/TypeScript bindings.
With a Rust web framework the core is consumed as an ordinary crate, so one compiler verifies
UI and core together — rung 1, the strongest guarantee in the whole matrix. BoltFFI is used
only where a real language boundary exists: Swift, Kotlin, C#.

And the web target means the **browser**. Bolted never ships the WASM build inside a native
webview to imitate a desktop or mobile app — on every other target the shell is genuinely
native, full stop. The web row exists so the same core reaches users with a URL, not so native
apps can be faked.

## 8. Honest risks

A vision doc that hides its risks is marketing. These are real:

1. **BoltFFI is young.** It is a recent project without years of production hardening, and its
   headline numbers (1,000× vs UniFFI) are boundary micro-benchmarks — real apps won't feel that
   multiplier. Bolted's value must not depend on it: the mitigation is the deliberately narrow
   coupling (annotations + CLI only), keeping a migration to another generator survivable rather
   than fatal.
2. **Codegen is the hard 20%.** Generated ViewModels and capability stubs across the three FFI
   shell languages (Swift, Kotlin, C#) is a serious code-generation project and the most likely
   place for scope creep. The
   verification ladder is the defense: a generator that can't keep rung-2 guarantees gets cut,
   not shipped soft.
3. **"Compile-time verified" must not overclaim.** No single compiler sees both sides of an FFI
   boundary. The honest formulation — and the one this document commits to — is *verified by
   construction plus build-time checks*. Rung 3 is checks, not proofs, and the docs must say so.
4. **Windows and Linux are the road less traveled.** The Apple/Android paths have abundant prior
   art; the desktop paths will surface problems first and need the most design attention early.
5. **mise cannot pin everything.** Xcode, NDK, and Windows SDK versions sit outside mise's
   control; `doctor` verifies and warns instead of pinning. That is a rung-3 answer to a problem
   that has no rung-1 answer, and it's the best available.
6. **The web target constrains the core.** The browser is a sandbox: the core must keep
   compiling for `wasm32-unknown-unknown`, with no blocking I/O, limited threading, capability
   gaps, and a real download-size budget. Supporting the web well forces async-first,
   lean-dependency discipline on the entire core — a genuine cost, accepted deliberately,
   because the same discipline makes every other target better too.
7. **Rust web frameworks are still young.** Leptos, Dioxus, and Silkenweb move fast and none has
   React-scale maturity. Bolted stays framework-agnostic on the web — the core is just a crate —
   so a shell can switch frameworks without touching the core.

## 9. What success looks like

- A new developer clones a Bolted app, runs `mise install && mise run doctor && mise run
  build:macos`, and has a running app — without reading a setup wiki.
- Adding a core method and forgetting one platform is **impossible to miss**: the next build says
  exactly what is missing and where.
- A breaking change to the FFI surface **cannot merge silently**.
- `bolted new` to a running app on all six targets: under 30 minutes.
- Deleting Bolted from a project leaves six working apps — five native, one web — and a working
  Rust crate. The framework adds verification and structure, never lock-in.

## 10. Positioning

| Alternative | Why not |
|-------------|---------|
| **Flutter / React Native / KMP-with-Compose** | Shared UI. Bolted's premise is that UI should be native and logic shared — the opposite trade. |
| **Tauri** | Webview UI on the desktop. Bolted draws the line the other way: WASM belongs in the browser, and desktop apps get truly native UI — never a webview. |
| **Crux** | Closest relative — shared Rust core, native shells. Bolted differs in its bets: BoltFFI-grade boundary performance and idioms, mise-standardized builds, and the build-time verification harness as the product's center of gravity. |
| **Raw BoltFFI** | Exactly right, and exactly where every team starts. Bolted is what they build around it next — done once, verified, and shared. |

---

*This document sets the perimeter. Features enter by demonstrating a rung on the verification
ladder; they leave when they can't hold it. The boundary belongs to BoltFFI; everything bolted
around it belongs here.*
