//! `doctor` — the environment report for what mise cannot pin (step 22, VISION risk 5).
//!
//! The knowledge of what a machine needs already exists, scattered across `mise.toml`'s task
//! guards; doctor is the same knowledge made queryable as one per-tier report. Its scope rule:
//! **cover exactly what `mise install` cannot guarantee** — Xcode, xcodegen, the Android
//! SDK/NDK/system image, Chrome, and the cargo-installed boltffi CLI at its pinned version.
//! Tools mise pins (rust, trunk, wasm-pack, the per-task JDK/Gradle/dotnet) are *exemptions*,
//! not rows: re-checking them would make doctor a second mise.
//!
//! Two drift hazards, two rung-3 pins (both in `tests/doctor_manifest.rs`, inside `mise run
//! check`): every `mise.toml` task must map to a doctor row or an [`EXEMPT`] reason, both
//! directions; and [`BOLTFFI_PINNED`] must equal `setup:boltffi`'s `want="…"` literal.
//!
//! Doctor **warns, never fails** (VISION: "doctor verifies and warns instead") — a machine that
//! deliberately lacks Android is not broken. What it cannot check statically it names in
//! [`MANUAL`] instead of omitting. The evaluation is a pure function of a [`Machine`] the bin
//! assembles, so every judgement here is unit-testable against synthetic environments; the only
//! subprocess (the boltffi version probe) runs in the bin and arrives as data.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The boltffi CLI version doctor demands — the same literal `setup:boltffi` installs
/// (`want="…"` in `mise.toml`). Cross-pinned both ways by `tests/doctor_manifest.rs`; a bump
/// that edits one and not the other fails `mise run check`.
pub const BOLTFFI_PINNED: &str = "0.28.0";

/// What a row probes. Kept closed and data-shaped so the table below stays declarative; the
/// judgement per kind lives once, in [`evaluate`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Probe {
    /// An executable on `$PATH` (the task guards' `command -v`).
    Command(&'static str),
    /// The Android SDK root: `$ANDROID_HOME`, defaulting to `~/Library/Android/sdk` exactly as
    /// every android task does.
    AndroidSdk,
    /// At least one NDK under `$ANDROID_HOME/ndk` (the task picks the newest; doctor only asks
    /// whether the choice is non-empty — re-deriving the *selection* would be duplicated logic,
    /// kill criterion 1).
    AndroidNdk,
    /// The aosp_atd android-34 system image the headless Gradle-managed device boots.
    AndroidSystemImage,
    /// Chrome/Chromium, the engine behind the headless wasm tier (`test:web`'s guard verbatim:
    /// the app bundle on macOS, or `google-chrome`/`chromium` on `$PATH`).
    Chrome,
    /// The cargo-installed boltffi CLI at [`BOLTFFI_PINNED`], looked up on the task guards'
    /// extended path (`${CARGO_HOME:-$HOME/.cargo}/bin` first).
    Boltffi,
}

/// One machine-checked requirement: what it is, which verbs refuse without it, how to fix it.
pub struct Row {
    /// Presentation group, named after the verb family (`apple`, `android`, `web`, `boltffi`).
    pub tier: &'static str,
    /// Human name for the report line.
    pub name: &'static str,
    /// The `mise run` tasks this requirement serves — the coverage manifest's mapping side.
    pub tasks: &'static [&'static str],
    pub probe: Probe,
    /// The remedy line printed under a MISSING row — the same fix the task guard names.
    pub remedy: &'static str,
}

/// The requirement table. Rows are the union of `mise.toml`'s un-pinnable guards; the manifest
/// test holds this table and `mise.toml` together from both sides.
pub const ROWS: &[Row] = &[
    Row {
        tier: "boltffi",
        name: "boltffi CLI",
        tasks: &[
            "pack:apple",
            "pack:apple:http",
            "pack:android",
            "pack:csharp",
        ],
        probe: Probe::Boltffi,
        remedy: "mise run setup:boltffi",
    },
    Row {
        tier: "apple",
        name: "xcodebuild",
        tasks: &["pack:apple", "pack:apple:http"],
        probe: Probe::Command("xcodebuild"),
        remedy: "install Xcode (the full app, not the CLT) — VISION risk 5",
    },
    Row {
        tier: "apple",
        name: "swift",
        tasks: &[
            "test:apple",
            "test:apple:http",
            "test:apple:gen",
            "run:apple",
        ],
        probe: Probe::Command("swift"),
        remedy: "install Xcode — VISION risk 5",
    },
    Row {
        tier: "apple:ui",
        name: "xcodegen",
        tasks: &["test:apple:ui"],
        probe: Probe::Command("xcodegen"),
        remedy: "brew install xcodegen",
    },
    Row {
        tier: "android",
        name: "Android SDK",
        tasks: &[
            "pack:android",
            "pack:android:http",
            "test:android",
            "test:android:http",
            "test:android:app",
            "test:android:gen",
            "test:android:hazard",
            "run:android",
            "bench:android:device",
        ],
        probe: Probe::AndroidSdk,
        remedy: "install the Android SDK (or set ANDROID_HOME) — VISION risk 5",
    },
    Row {
        tier: "android",
        name: "Android NDK",
        tasks: &["pack:android", "pack:android:http"],
        probe: Probe::AndroidNdk,
        remedy: "sdkmanager 'ndk;27.0.12077973'",
    },
    Row {
        tier: "android",
        name: "aosp_atd android-34 system image",
        tasks: &[
            "test:android",
            "test:android:http",
            "test:android:app",
            "test:android:gen",
            "test:android:hazard",
        ],
        probe: Probe::AndroidSystemImage,
        remedy: "sdkmanager 'system-images;android-34;aosp_atd;arm64-v8a'",
    },
    Row {
        tier: "web",
        name: "Chrome",
        tasks: &["test:web"],
        probe: Probe::Chrome,
        remedy: "install Google Chrome (wasm-pack fetches a matching chromedriver)",
    },
];

/// Tasks with no doctor row, each with the reason — the coverage manifest's other side. A task
/// listed here with a stale name, or missing from both this list and every row's `tasks`, fails
/// the manifest test.
pub const EXEMPT: &[(&str, &str)] = &[
    ("check", "pure cargo; rust is mise-pinned"),
    ("test", "pure cargo; rust is mise-pinned"),
    ("gen:ffi", "pure cargo; the generators are in-repo"),
    (
        "setup:boltffi",
        "is itself the remedy doctor names for the boltffi row",
    ),
    (
        "build:web",
        "mise-pinned tools only (trunk); the wasm target self-heals in-task",
    ),
    (
        "serve:web",
        "mise-pinned tools only (trunk); the wasm target self-heals in-task",
    ),
    (
        "check:web",
        "mise-pinned tools only (trunk); the wasm target self-heals in-task",
    ),
    (
        "test:csharp",
        "dotnet is mise-pinned per-task; the seam needs nothing else",
    ),
    (
        "test:os:sandbox",
        "spike — disposal-eligible (topology design pass)",
    ),
    (
        "test:os:launchd",
        "spike — disposal-eligible (topology design pass)",
    ),
    (
        "test:os:app",
        "spike — disposal-eligible (topology design pass)",
    ),
    (
        "run:os:app",
        "spike — disposal-eligible (topology design pass)",
    ),
    (
        "test:os:linux",
        "spike — disposal-eligible (topology design pass)",
    ),
    (
        "test:os:systemd",
        "spike — disposal-eligible (topology design pass)",
    ),
    ("doctor", "is this report"),
];

/// What doctor cannot check statically, named instead of omitted.
pub const MANUAL: &[&str] = &[
    "test:apple:ui also needs a logged-in GUI session + Accessibility permission for the \
     controlling app (System Settings → Privacy & Security → Accessibility)",
    "run:android / bench:android:device need an attached device — and the bench refuses \
     emulators on purpose (step-07 kill criterion 4)",
];

/// The environment, as data. The bin assembles it from the real process environment (plus the
/// one subprocess probe); tests assemble synthetic ones. Evaluation never reads `std::env`.
pub struct Machine {
    /// `$PATH`, verbatim.
    pub path: String,
    /// `$HOME` — the base for the Android SDK default and `~/.cargo`.
    pub home: PathBuf,
    /// `$ANDROID_HOME`, if set (overrides the default, exactly as the tasks do).
    pub android_home: Option<PathBuf>,
    /// `$CARGO_HOME`, if set.
    pub cargo_home: Option<PathBuf>,
    /// Trimmed stdout of `boltffi --version`, if the CLI was found and ran. `None` = not found.
    pub boltffi_version: Option<String>,
}

impl Machine {
    /// The Android SDK root this machine resolves to — env override or the same default every
    /// android task uses.
    pub fn android_sdk(&self) -> PathBuf {
        self.android_home
            .clone()
            .unwrap_or_else(|| self.home.join("Library/Android/sdk"))
    }

    /// Where the boltffi CLI lives: `${CARGO_HOME:-$HOME/.cargo}/bin`, the task guards' PATH
    /// extension.
    pub fn cargo_bin(&self) -> PathBuf {
        self.cargo_home
            .clone()
            .unwrap_or_else(|| self.home.join(".cargo"))
            .join("bin")
    }
}

/// One evaluated row: `Ok` carries the found detail (a path, a version), `Missing` carries what
/// was looked for and not found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Status {
    Ok(String),
    Missing(String),
}

pub struct RowResult {
    pub tier: &'static str,
    pub name: &'static str,
    pub remedy: &'static str,
    pub status: Status,
}

/// Search a `$PATH` string for an executable, the way `command -v` does. Pure given its inputs,
/// which is what makes the doctor judgements testable against a synthetic PATH.
pub fn find_executable(name: &str, path: &str) -> Option<PathBuf> {
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Evaluate every row against one machine. The per-probe judgements mirror the task guards they
/// aggregate — same paths, same defaults — restating *literals* (pinned by the manifest test),
/// never re-deriving *logic* (kill criterion 1: the NDK row asks "any?", not "which?").
pub fn evaluate(m: &Machine) -> Vec<RowResult> {
    ROWS.iter()
        .map(|row| RowResult {
            tier: row.tier,
            name: row.name,
            remedy: row.remedy,
            status: probe(row.probe, m),
        })
        .collect()
}

fn probe(probe: Probe, m: &Machine) -> Status {
    match probe {
        Probe::Command(name) => match find_executable(name, &m.path) {
            Some(p) => Status::Ok(p.display().to_string()),
            None => Status::Missing(format!("`{name}` not on PATH")),
        },
        Probe::AndroidSdk => {
            let sdk = m.android_sdk();
            if sdk.is_dir() {
                Status::Ok(sdk.display().to_string())
            } else {
                Status::Missing(format!("no SDK at {}", sdk.display()))
            }
        }
        Probe::AndroidNdk => {
            let ndk = m.android_sdk().join("ndk");
            let any = std::fs::read_dir(&ndk)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.path().is_dir());
            if any {
                Status::Ok(format!("{}/*", ndk.display()))
            } else {
                Status::Missing(format!("no NDK under {}", ndk.display()))
            }
        }
        Probe::AndroidSystemImage => {
            let img = m.android_sdk().join("system-images/android-34/aosp_atd");
            if img.is_dir() {
                Status::Ok(img.display().to_string())
            } else {
                Status::Missing(format!("no dir at {}", img.display()))
            }
        }
        Probe::Chrome => {
            let bundle = Path::new("/Applications/Google Chrome.app");
            if bundle.is_dir() {
                Status::Ok(bundle.display().to_string())
            } else if let Some(p) = find_executable("google-chrome", &m.path)
                .or_else(|| find_executable("chromium", &m.path))
            {
                Status::Ok(p.display().to_string())
            } else {
                Status::Missing("no Chrome app bundle, google-chrome or chromium".to_owned())
            }
        }
        Probe::Boltffi => match &m.boltffi_version {
            // Substring match, exactly like `setup:boltffi`'s `grep -q "$want"` — doctor judges
            // with the guard's own predicate rather than inventing a stricter parse.
            Some(v) if v.contains(BOLTFFI_PINNED) => Status::Ok(v.clone()),
            Some(v) => Status::Missing(format!("found `{v}`, want {BOLTFFI_PINNED}")),
            None => Status::Missing(format!(
                "no boltffi in {} or on PATH",
                m.cargo_bin().display()
            )),
        },
    }
}

/// Render the report. Pure text out; tiers in table order; a MISSING row is followed by its
/// remedy line; the manual notes close the report so what doctor cannot see is named.
pub fn render(results: &[RowResult]) -> String {
    let mut out = String::new();
    out.push_str("bolted doctor — the environment mise cannot pin (VISION risk 5)\n");

    let mut tier_seen: Vec<&str> = Vec::new();
    for r in results {
        if !tier_seen.contains(&r.tier) {
            tier_seen.push(r.tier);
            let _ = writeln!(out, "\n[{}]", r.tier);
        }
        match &r.status {
            Status::Ok(detail) => {
                let _ = writeln!(out, "  ok       {} — {detail}", r.name);
            }
            Status::Missing(why) => {
                let _ = writeln!(out, "  MISSING  {} — {why}", r.name);
                let _ = writeln!(out, "           remedy: {}", r.remedy);
            }
        }
    }

    out.push_str("\nmanual (doctor cannot check these — they are named, not omitted):\n");
    for note in MANUAL {
        let _ = writeln!(out, "  - {note}");
    }

    let missing = results
        .iter()
        .filter(|r| matches!(r.status, Status::Missing(_)))
        .count();
    let _ = writeln!(
        out,
        "\n{}/{} ok — doctor warns, never fails; every verb still fail-fasts with the same remedy",
        results.len() - missing,
        results.len()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch dir unique to one test, under the target-adjacent temp root. Created fresh;
    /// best-effort removed by the OS's temp policy (contents are empty dirs/files).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("bolted-doctor-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir creates");
        dir
    }

    fn machine(home: &Path) -> Machine {
        Machine {
            path: String::new(),
            home: home.to_path_buf(),
            android_home: None,
            cargo_home: None,
            boltffi_version: None,
        }
    }

    #[cfg(unix)]
    fn plant_executable(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").expect("writes");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmods");
        p
    }

    #[cfg(unix)]
    #[test]
    fn find_executable_walks_the_given_path_only() {
        let a = scratch("path-a");
        let b = scratch("path-b");
        plant_executable(&b, "toolx");
        let path = format!("{}:{}", a.display(), b.display());
        assert_eq!(find_executable("toolx", &path), Some(b.join("toolx")));
        assert_eq!(find_executable("toolx", &a.display().to_string()), None);
        // A plain file without the execute bit is not an executable.
        std::fs::write(a.join("tooly"), "").expect("writes");
        assert_eq!(find_executable("tooly", &path), None);
    }

    #[test]
    fn the_android_sdk_default_matches_the_task_guards() {
        let m = machine(Path::new("/Users/nobody"));
        assert_eq!(
            m.android_sdk(),
            PathBuf::from("/Users/nobody/Library/Android/sdk")
        );
        let with_env = Machine {
            android_home: Some(PathBuf::from("/opt/sdk")),
            ..machine(Path::new("/Users/nobody"))
        };
        assert_eq!(with_env.android_sdk(), PathBuf::from("/opt/sdk"));
    }

    #[test]
    fn a_bare_home_reports_the_android_rows_missing_with_their_remedies() {
        let home = scratch("bare-home");
        let results = evaluate(&machine(&home));
        let by_name = |n: &str| {
            results
                .iter()
                .find(|r| r.name == n)
                .unwrap_or_else(|| panic!("row `{n}` exists"))
        };
        assert!(matches!(by_name("Android SDK").status, Status::Missing(_)));
        assert!(matches!(by_name("Android NDK").status, Status::Missing(_)));
        assert_eq!(
            by_name("Android NDK").remedy,
            "sdkmanager 'ndk;27.0.12077973'"
        );
    }

    #[test]
    fn a_planted_sdk_flips_the_rows_it_serves_and_only_those() {
        let home = scratch("planted-sdk");
        let sdk = home.join("Library/Android/sdk");
        std::fs::create_dir_all(sdk.join("ndk/27.0.12077973")).expect("ndk dir");
        std::fs::create_dir_all(sdk.join("system-images/android-34/aosp_atd")).expect("img dir");
        let results = evaluate(&machine(&home));
        for name in [
            "Android SDK",
            "Android NDK",
            "aosp_atd android-34 system image",
        ] {
            let row = results.iter().find(|r| r.name == name);
            assert!(
                matches!(row.map(|r| &r.status), Some(Status::Ok(_))),
                "`{name}` should be ok"
            );
        }
        // An empty ndk/ dir (no version subdir) is still missing: the guard wants a choice.
        let home2 = scratch("empty-ndk");
        std::fs::create_dir_all(home2.join("Library/Android/sdk/ndk")).expect("dir");
        let results2 = evaluate(&machine(&home2));
        let ndk = results2.iter().find(|r| r.name == "Android NDK");
        assert!(matches!(ndk.map(|r| &r.status), Some(Status::Missing(_))));
    }

    #[test]
    fn the_boltffi_row_judges_with_the_guards_substring_predicate() {
        let home = scratch("boltffi");
        let m = |v: Option<&str>| Machine {
            boltffi_version: v.map(str::to_owned),
            ..machine(&home)
        };
        let status = |m: &Machine| {
            evaluate(m)
                .into_iter()
                .find(|r| r.name == "boltffi CLI")
                .map(|r| r.status)
        };
        assert!(matches!(
            status(&m(Some("boltffi 0.28.0"))),
            Some(Status::Ok(_))
        ));
        assert!(matches!(
            status(&m(Some("boltffi 0.27.3"))),
            Some(Status::Missing(_))
        ));
        assert!(matches!(status(&m(None)), Some(Status::Missing(_))));
    }

    #[test]
    fn a_missing_row_renders_its_remedy_and_the_manual_notes_always_render() {
        let results = vec![
            RowResult {
                tier: "apple",
                name: "xcodebuild",
                remedy: "install Xcode",
                status: Status::Ok("/usr/bin/xcodebuild".to_owned()),
            },
            RowResult {
                tier: "android",
                name: "Android NDK",
                remedy: "sdkmanager 'ndk;27.0.12077973'",
                status: Status::Missing("no NDK".to_owned()),
            },
        ];
        let text = render(&results);
        assert!(text.contains("ok       xcodebuild — /usr/bin/xcodebuild"));
        assert!(text.contains("MISSING  Android NDK — no NDK"));
        assert!(text.contains("remedy: sdkmanager 'ndk;27.0.12077973'"));
        assert!(text.contains("manual (doctor cannot check these"));
        assert!(text.contains("Accessibility"));
        assert!(text.contains("1/2 ok"));
    }

    #[test]
    fn every_row_serves_at_least_one_task_and_no_task_is_both_mapped_and_exempt() {
        // The manifest test (tests/doctor_manifest.rs) holds this table against mise.toml; this
        // half holds it against itself.
        for row in ROWS {
            assert!(!row.tasks.is_empty(), "row `{}` serves no task", row.name);
        }
        let exempt: Vec<&str> = EXEMPT.iter().map(|(t, _)| *t).collect();
        for row in ROWS {
            for task in row.tasks {
                assert!(
                    !exempt.contains(task),
                    "task `{task}` is both mapped (row `{}`) and exempt",
                    row.name
                );
            }
        }
    }
}
