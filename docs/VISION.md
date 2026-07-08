# Bolted — Vision

**Bolted is everything you bolt around [BoltFFI](https://boltffi.dev): a compile-time-verified
application framework for products built as one Rust core with fully native faces on Windows,
macOS, Linux, Android, iOS, and the web.**

A Bolted product is a **full citizen of every OS it ships on** — not just a window. The same
core also runs as the background daemon or service, sits in the tray or menu bar, integrates
with the file navigator, sends native notifications, starts at login. Generic enough that a
Dropbox clone (sync daemon, Finder/Explorer integration, tray icon) and a Twitter clone (a
plain UI app) are both natural fits.

BoltFFI solves the language boundary and deliberately nothing else. Everything around it —
project shape, platform capabilities, state flow, background services, six targets drifting
apart, an environment-setup wiki that's wrong within a month — is glue every team reinvents,
and glue fails at runtime. Hence the founding rule:

> **Every piece of glue between the Rust core and a platform must be checked by a compiler or
> by the build. Glue that can only fail at runtime is out of scope.**

## The bets

1. **BoltFFI is the boundary** — coupled narrowly (annotations + CLI only), so it remains a
   replaceable part, not a load-bearing wall.
2. **Native everything, shared nothing visible.** All logic and state live in the Rust core;
   every user-visible surface is platform-true — SwiftUI/Compose/WinUI windows, real tray
   icons, real file-manager extensions, real daemons under launchd/systemd/Windows services.
   On the web: Rust web frameworks (Leptos, Dioxus, Silkenweb) consuming the core as a plain
   crate, in the browser only. Never a webview, anywhere.
3. **mise is the single entry point** — toolchains, tasks, and environment pinned and
   verified; the same verbs (`doctor · check · build · test · pack`) on every machine and CI.

## The verification ladder

Every framework feature must live on the highest rung it can reach:

1. **Proven by rustc** (types, traits, exhaustiveness)
2. **Generated, then proven by the platform compiler** (bindings, glue)
3. **Verified at build time** (`mise run check`: drift, coverage, compatibility, doctor)
4. **Runtime-checked** — forbidden for framework mechanics; where the OS forces it
   (permissions), a typed `Result`, never a surprise

A feature that can only exist on rung 4 is out of scope.

## In scope

- Scaffolding, the standard mise verbs, environment doctor
- The core contract: typed state, validation, and errors ([ARCHITECTURE.md](ARCHITECTURE.md));
  capability traits declared in Rust, implemented per platform, coverage-checked per target
- OS integration as first-class surfaces: daemons/services, tray/menu bar, file-manager
  integration, notifications, autostart — all attached to the same verified contract
- The verification harness: binding drift, FFI-surface semver, capability coverage,
  generated contract tests
- Optional batteries under the same rules: persistence, i18n, config, observability

## Out — permanently

- Shared UI toolkits, webviews, widget abstractions
- Runtimes, reflection, dynamic plugin loading
- Hiding the platforms (Xcode and Android Studio stay first-class) or second build entry points
- Binding generation itself — that's BoltFFI's job

## Targets

| Target | Faces | Boundary |
|--------|-------|----------|
| macOS / iOS | Swift: SwiftUI, app extensions, launchd daemons | BoltFFI |
| Android | Kotlin: Compose, services | BoltFFI |
| Windows | C#: WinUI 3, tray, Windows services | BoltFFI |
| Linux | Rust UI (or C#), systemd units | none — plain crate |
| Web | Rust (Leptos / Dioxus / …), browser only | none — plain crate |

## Honest risks

1. **BoltFFI is young** and its benchmarks are micro-benchmarks; the narrow seam is the exit.
2. **Deep OS integration is the roughest terrain** — sandboxed extension processes, IPC,
   daemon lifecycles differ wildly per OS. It must be spiked, not assumed.
3. **Codegen across Swift/Kotlin/C# is the hard 20%.**
4. **"Compile-time verified" must not overclaim**: verified by construction plus build-time
   checks, not one compiler seeing both sides.
5. **mise can't pin Xcode/NDK/Windows SDK** — doctor verifies and warns instead.

## Success looks like

- Clone → `mise install && mise run doctor && mise run build:macos` → running app. No wiki.
- A tray icon or a daemon is a scaffold option, not a custom engineering project.
- Forgetting one platform is impossible to miss; a breaking FFI change cannot merge silently.
- Deleting Bolted leaves working native apps and a working Rust crate. No lock-in.
