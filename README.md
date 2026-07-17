# Bolted

Everything you bolt around [BoltFFI](https://boltffi.dev): a compile-time-verified application
framework for products built as one Rust core with fully native faces — windows, daemons, tray
icons, file-manager integration — on Windows / macOS / Linux / Android / iOS, plus a Rust-web
target (browser only, never a webview).

**Status: design validation spike.** Nothing to build yet beyond the spike steps.

| Doc | What it is |
|-----|------------|
| [docs/VISION.md](docs/VISION.md) | Scope, principles, the verification ladder, non-goals |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | The design: facets over a store-owned core, observe/command/draft contract, typed fields, live rebase |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | The ubiquitous language — deliberately small, owner-curated |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Phased plan and step status |
| [docs/steps/](docs/steps/) | Detailed per-step plans and completion reports |
| [CLAUDE.md](CLAUDE.md) | Project memory: read order, working agreement, conventions |

Build entry point (once step 01 lands): [mise](https://mise.jdx.dev) — `mise run check`.
