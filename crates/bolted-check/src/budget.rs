//! The wasm size budget (step 17) — measure a built `dist/` and enforce a committed budget.
//!
//! # Why this lives in `bolted-check`, behind a feature
//!
//! Step 16's lesson (recorded in memory): ask first whether an analysis is a pure source function
//! or needs runtime facts. The constraint-surface snapshot was neither and became a per-feature
//! `-ffi` example. This one is different again — it reads **built-artifact bytes**, which fits a
//! real `bolted-check` **bin**. But its one extra dependency (`brotli`, for the wire-size number)
//! must not leak into the host `check` graph: `bolted-check` is a dev-dependency of the `-ffi`
//! crates, so a plain dependency would be compiled by every `cargo test --workspace`. So the split
//! this module draws:
//!
//! - The **policy** — parsing the committed budget, comparing a measurement to it, choosing the one
//!   `*_bg.wasm` out of a dist, and formatting the failure — compiles **unconditionally** and is
//!   unit-tested inside plain `cargo test --workspace` (i.e. inside `mise run check`).
//! - The **measurement** — the `dist/` walk and the brotli-q11 compression — sits behind the
//!   `budget` cargo feature, which the `wasm-budget` bin declares via `required-features`. `check`
//!   never enables it, so `brotli` never enters its graph (`cargo tree -p bolted-check` is clean).
//!
//! # The budget policy (also in the committed `wasm-budget.txt` header)
//!
//! Maxima are the measured post-migration baseline × 1.10, rounded up to a whole KiB. The floor is
//! half the baseline wasm, rounded down to a whole KiB — a stub-catcher, so an empty or truncated
//! artifact can never read as "under budget". Re-baselining is a deliberate human edit of the
//! committed file, never automatic: deriving the limits from the build every time is D27's own
//! rejected alternative (a tripwire you silence by reflex guards nothing).

use std::fmt;

/// The three committed budget numbers. Hand-parsed from `key = value` lines (see [`parse_budget`]) —
/// no TOML dependency for three integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    /// Ceiling on the raw (uncompressed) wasm module — the number that grows when the feature or the
    /// framework emits more code.
    pub wasm_raw_max_bytes: u64,
    /// Ceiling on the compressed **wire** total (wasm + JS glue, brotli-q11) — what actually crosses
    /// the network, the truth the budget ultimately guards.
    pub wire_brotli_max_bytes: u64,
    /// Sanity floor on the raw wasm: below this the build produced nothing meaningful (empty/stub),
    /// and a tiny artifact must never pass as "comfortably under the maximum".
    pub wasm_raw_min_bytes: u64,
}

/// Why a `wasm-budget.txt` could not be parsed. Each carries the offending token so the message
/// points a human at the exact line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetParseError {
    /// A non-comment, non-blank line with no `=`.
    MalformedLine(String),
    /// A key that is not one of the three the budget file defines.
    UnknownKey(String),
    /// A value that is not a non-negative integer (after `_` separators are stripped).
    BadValue { key: String, value: String },
    /// The same key set twice.
    Duplicate(String),
    /// A required key never appeared.
    Missing(&'static str),
}

impl fmt::Display for BudgetParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BudgetParseError::MalformedLine(line) => {
                write!(f, "line has no `key = value` form: `{line}`")
            }
            BudgetParseError::UnknownKey(key) => write!(
                f,
                "unknown key `{key}` (expected one of wasm_raw_max_bytes, \
                 wire_brotli_max_bytes, wasm_raw_min_bytes)"
            ),
            BudgetParseError::BadValue { key, value } => {
                write!(f, "`{key}` is not a byte count: `{value}`")
            }
            BudgetParseError::Duplicate(key) => write!(f, "key `{key}` set more than once"),
            BudgetParseError::Missing(key) => write!(f, "required key `{key}` is missing"),
        }
    }
}

impl std::error::Error for BudgetParseError {}

/// Parse a committed budget file: `key = value` lines, `#` comments (whole-line or trailing), blank
/// lines ignored, `_` digit separators allowed in values. All three keys are required, unknown keys
/// and duplicates are hard errors — a budget file with a typo'd key must fail loudly, not silently
/// leave a limit unset.
pub fn parse_budget(text: &str) -> Result<Budget, BudgetParseError> {
    let mut wasm_raw_max: Option<u64> = None;
    let mut wire_brotli_max: Option<u64> = None;
    let mut wasm_raw_min: Option<u64> = None;

    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| BudgetParseError::MalformedLine(raw_line.to_owned()))?;
        let key = key.trim();
        let value = value.trim();
        let slot = match key {
            "wasm_raw_max_bytes" => &mut wasm_raw_max,
            "wire_brotli_max_bytes" => &mut wire_brotli_max,
            "wasm_raw_min_bytes" => &mut wasm_raw_min,
            other => return Err(BudgetParseError::UnknownKey(other.to_owned())),
        };
        if slot.is_some() {
            return Err(BudgetParseError::Duplicate(key.to_owned()));
        }
        *slot = Some(parse_u64(value).ok_or_else(|| BudgetParseError::BadValue {
            key: key.to_owned(),
            value: value.to_owned(),
        })?);
    }

    Ok(Budget {
        wasm_raw_max_bytes: wasm_raw_max.ok_or(BudgetParseError::Missing("wasm_raw_max_bytes"))?,
        wire_brotli_max_bytes: wire_brotli_max
            .ok_or(BudgetParseError::Missing("wire_brotli_max_bytes"))?,
        wasm_raw_min_bytes: wasm_raw_min.ok_or(BudgetParseError::Missing("wasm_raw_min_bytes"))?,
    })
}

fn strip_comment(line: &str) -> &str {
    match line.split_once('#') {
        Some((before, _)) => before,
        None => line,
    }
}

fn parse_u64(value: &str) -> Option<u64> {
    let cleaned: String = value.chars().filter(|c| *c != '_').collect();
    if cleaned.is_empty() || !cleaned.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    cleaned.parse().ok()
}

/// The measured sizes of a built `dist/`: the wasm module and the JS glue, each raw and brotli-q11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    pub wasm_raw: u64,
    pub wasm_brotli: u64,
    pub js_raw: u64,
    pub js_brotli: u64,
}

impl Measurement {
    /// Raw bytes that ship: wasm + glue.
    pub fn wire_raw(&self) -> u64 {
        self.wasm_raw + self.js_raw
    }

    /// Compressed bytes that cross the wire: brotli(wasm) + brotli(glue). The budget's headline
    /// number (step-04 measured each stream separately and summed for the wire).
    pub fn wire_brotli(&self) -> u64 {
        self.wasm_brotli + self.js_brotli
    }
}

/// A single way a measurement broke its budget. Each carries measured **and** limit so the printed
/// message is self-explanatory (M4 asserts the numbers appear).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    WasmRawOverBudget { measured: u64, max: u64 },
    WireBrotliOverBudget { measured: u64, max: u64 },
    WasmRawUnderFloor { measured: u64, min: u64 },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::WasmRawOverBudget { measured, max } => write!(
                f,
                "wasm (raw) {} exceeds budget {} by {} B",
                fmt_bytes(*measured),
                fmt_bytes(*max),
                measured.saturating_sub(*max)
            ),
            Violation::WireBrotliOverBudget { measured, max } => write!(
                f,
                "wire (brotli) {} exceeds budget {} by {} B",
                fmt_bytes(*measured),
                fmt_bytes(*max),
                measured.saturating_sub(*max)
            ),
            Violation::WasmRawUnderFloor { measured, min } => write!(
                f,
                "wasm (raw) {} is below the sanity floor {} — the build produced no meaningful \
                 artifact (empty/stub wasm). Fix the build; do not lower the floor to pass",
                fmt_bytes(*measured),
                fmt_bytes(*min),
            ),
        }
    }
}

/// Compare a measurement to a budget. Returns every violation (there can be more than one), empty
/// when the build is within budget. Pure — the fs and compression already happened.
pub fn check_budget(m: &Measurement, b: &Budget) -> Vec<Violation> {
    let mut v = Vec::new();
    if m.wasm_raw > b.wasm_raw_max_bytes {
        v.push(Violation::WasmRawOverBudget {
            measured: m.wasm_raw,
            max: b.wasm_raw_max_bytes,
        });
    }
    if m.wire_brotli() > b.wire_brotli_max_bytes {
        v.push(Violation::WireBrotliOverBudget {
            measured: m.wire_brotli(),
            max: b.wire_brotli_max_bytes,
        });
    }
    if m.wasm_raw < b.wasm_raw_min_bytes {
        v.push(Violation::WasmRawUnderFloor {
            measured: m.wasm_raw,
            min: b.wasm_raw_min_bytes,
        });
    }
    v
}

/// The human report for a failed budget check: the violations, then the duty. Named after the fact
/// that a tripwire people silence by reflex guards nothing — so the message says *how* to raise the
/// budget deliberately, and distinguishes a floor breach (a broken build, not a size to bless).
pub fn format_report(violations: &[Violation]) -> String {
    let mut s = String::from("\nwasm size budget FAILED:\n");
    for v in violations {
        s.push_str(&format!("  - {v}\n"));
    }
    s.push_str(
        "\nReview what changed. If a size increase is intended, raise the relevant maximum in\n\
         crates/profile-web/wasm-budget.txt in this same change — deliberately; the budget is never\n\
         auto-bumped (the D27 precedent). A floor breach is different: it means the build is broken —\n\
         fix the build, do not lower the floor.\n",
    );
    s
}

/// The `--print` report: the measured table, plus a copy-pasteable suggested budget at the baseline
/// policy (max × 1.10 ↑KiB, floor × 0.5 ↓KiB). M3 sets `wasm-budget.txt` from this block.
pub fn render_measurement(m: &Measurement) -> String {
    let mut s = String::new();
    s.push_str("wasm size measurement (brotli: `brotli` crate, quality 11, window 22)\n");
    s.push_str(&format!(
        "  wasm   raw = {:<22} brotli = {}\n",
        fmt_bytes(m.wasm_raw),
        fmt_bytes(m.wasm_brotli)
    ));
    s.push_str(&format!(
        "  glue   raw = {:<22} brotli = {}\n",
        fmt_bytes(m.js_raw),
        fmt_bytes(m.js_brotli)
    ));
    s.push_str(&format!(
        "  wire   raw = {:<22} brotli = {}\n",
        fmt_bytes(m.wire_raw()),
        fmt_bytes(m.wire_brotli())
    ));
    s.push_str(
        "\nsuggested wasm-budget.txt (baseline policy: max × 1.10 ↑KiB, floor × 0.5 ↓KiB):\n",
    );
    s.push_str(&format!(
        "  wasm_raw_max_bytes    = {}\n",
        ceil_headroom_kib(m.wasm_raw)
    ));
    s.push_str(&format!(
        "  wire_brotli_max_bytes = {}\n",
        ceil_headroom_kib(m.wire_brotli())
    ));
    s.push_str(&format!(
        "  wasm_raw_min_bytes    = {}\n",
        floor_half_kib(m.wasm_raw)
    ));
    s
}

/// `measured × 1.10`, rounded **up** to a whole KiB — the maximum policy.
pub fn ceil_headroom_kib(n: u64) -> u64 {
    (n * 110 / 100).div_ceil(1024) * 1024
}

/// `measured × 0.5`, rounded **down** to a whole KiB — the floor policy (a stub-catcher).
pub fn floor_half_kib(n: u64) -> u64 {
    (n / 2 / 1024) * 1024
}

fn fmt_bytes(n: u64) -> String {
    format!("{n} B ({:.1} KiB)", n as f64 / 1024.0)
}

/// Choose the single `*_bg.wasm` in a dist listing. Trunk hashes the filename
/// (`profile-web-<hash>_bg.wasm`) and a stale `dist/` can hold leftovers from an earlier build — so
/// ≠ 1 match is a hard error, never a silent pick. Pure, so the policy is unit-tested without a
/// filesystem.
pub fn pick_bg_wasm(names: &[String]) -> Result<String, DistError> {
    let mut hits: Vec<&String> = names.iter().filter(|n| n.ends_with("_bg.wasm")).collect();
    hits.sort();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => Err(DistError::NoWasm),
        _ => Err(DistError::AmbiguousWasm(
            hits.into_iter().cloned().collect(),
        )),
    }
}

/// Why a `dist/` could not be measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistError {
    /// No `*_bg.wasm` at all — an unbuilt or wrong directory.
    NoWasm,
    /// More than one `*_bg.wasm` — a stale dist; refuse rather than measure an arbitrary one.
    AmbiguousWasm(Vec<String>),
    /// The wasm's paired glue (`<stem>.js`) is absent.
    MissingGlue(String),
    /// A filesystem or compression error, with context.
    Io(String),
}

impl fmt::Display for DistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistError::NoWasm => write!(
                f,
                "no `*_bg.wasm` in the dist directory — run `trunk build --release` first"
            ),
            DistError::AmbiguousWasm(names) => write!(
                f,
                "more than one `*_bg.wasm` in dist ({}) — a stale build; clean dist and rebuild",
                names.join(", ")
            ),
            DistError::MissingGlue(name) => {
                write!(f, "the wasm's paired glue `{name}` is missing from dist")
            }
            DistError::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for DistError {}

/// Measure a built `dist/`: pick the one wasm, pair it with its glue, and record raw + brotli-q11
/// for each. Behind the `budget` feature because it is the only path that pulls `brotli` (and does
/// filesystem work) — the policy above stays feature-free so it runs inside `mise run check`.
#[cfg(feature = "budget")]
pub fn measure(dist: &std::path::Path) -> Result<Measurement, DistError> {
    use std::fs;

    let mut names = Vec::new();
    let entries =
        fs::read_dir(dist).map_err(|e| DistError::Io(format!("read {}: {e}", dist.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| DistError::Io(format!("entry in {}: {e}", dist.display())))?;
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file && let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }

    let wasm_name = pick_bg_wasm(&names)?;
    // Trunk pairs `<stem>_bg.wasm` with the glue `<stem>.js`. Derive the glue from the chosen wasm
    // rather than globbing `*.js`: the pairing is exact, and a stale dist could hold spare `.js`.
    let stem = wasm_name
        .strip_suffix("_bg.wasm")
        .ok_or_else(|| DistError::Io("the chosen wasm lost its `_bg.wasm` suffix".to_owned()))?;
    let js_name = format!("{stem}.js");
    if !names.iter().any(|n| n == &js_name) {
        return Err(DistError::MissingGlue(js_name));
    }

    let wasm_bytes = fs::read(dist.join(&wasm_name))
        .map_err(|e| DistError::Io(format!("read {wasm_name}: {e}")))?;
    let js_bytes =
        fs::read(dist.join(&js_name)).map_err(|e| DistError::Io(format!("read {js_name}: {e}")))?;

    Ok(Measurement {
        wasm_raw: wasm_bytes.len() as u64,
        wasm_brotli: brotli_len(&wasm_bytes)?,
        js_raw: js_bytes.len() as u64,
        js_brotli: brotli_len(&js_bytes)?,
    })
}

/// Compressed length at brotli quality 11, window 22 — brotli's `BROTLI_DEFAULT_WINDOW`, i.e. the
/// `brotli -q11` CLI default step-04 measured with. `into_inner` finalizes the stream before its
/// length is read.
#[cfg(feature = "budget")]
fn brotli_len(bytes: &[u8]) -> Result<u64, DistError> {
    use std::io::Write;
    let mut w = brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22);
    w.write_all(bytes)
        .map_err(|e| DistError::Io(format!("brotli compress: {e}")))?;
    Ok(w.into_inner().len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
# a comment
wasm_raw_max_bytes    = 335872   # trailing comment
wire_brotli_max_bytes = 102_400

wasm_raw_min_bytes    = 155648
";

    #[test]
    fn parses_a_well_formed_file_with_comments_and_separators() {
        let b = parse_budget(GOOD).expect("parses");
        assert_eq!(
            b,
            Budget {
                wasm_raw_max_bytes: 335872,
                wire_brotli_max_bytes: 102400, // `_` separator stripped
                wasm_raw_min_bytes: 155648,
            }
        );
    }

    #[test]
    fn a_missing_key_is_an_error() {
        let text = "wasm_raw_max_bytes = 1\nwire_brotli_max_bytes = 2\n";
        assert_eq!(
            parse_budget(text),
            Err(BudgetParseError::Missing("wasm_raw_min_bytes"))
        );
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let text = "wasm_raw_max_bytes = 1\ngzip_max_bytes = 2\n";
        assert_eq!(
            parse_budget(text),
            Err(BudgetParseError::UnknownKey("gzip_max_bytes".to_owned()))
        );
    }

    #[test]
    fn a_duplicate_key_is_an_error() {
        let text = "wasm_raw_max_bytes = 1\nwasm_raw_max_bytes = 2\n";
        assert_eq!(
            parse_budget(text),
            Err(BudgetParseError::Duplicate("wasm_raw_max_bytes".to_owned()))
        );
    }

    #[test]
    fn a_non_integer_value_is_an_error() {
        let text = "wasm_raw_max_bytes = big\n";
        assert_eq!(
            parse_budget(text),
            Err(BudgetParseError::BadValue {
                key: "wasm_raw_max_bytes".to_owned(),
                value: "big".to_owned()
            })
        );
    }

    #[test]
    fn a_line_without_equals_is_an_error() {
        assert_eq!(
            parse_budget("wasm_raw_max_bytes 1\n"),
            Err(BudgetParseError::MalformedLine(
                "wasm_raw_max_bytes 1".to_owned()
            ))
        );
    }

    fn budget() -> Budget {
        Budget {
            wasm_raw_max_bytes: 300,
            wire_brotli_max_bytes: 100,
            wasm_raw_min_bytes: 50,
        }
    }

    #[test]
    fn a_measurement_within_budget_has_no_violations() {
        let m = Measurement {
            wasm_raw: 250,
            wasm_brotli: 60,
            js_raw: 20,
            js_brotli: 30,
        };
        assert!(check_budget(&m, &budget()).is_empty());
    }

    #[test]
    fn wasm_over_max_is_a_violation_naming_both_numbers() {
        let m = Measurement {
            wasm_raw: 320,
            wasm_brotli: 60,
            js_raw: 20,
            js_brotli: 30,
        };
        let v = check_budget(&m, &budget());
        assert_eq!(
            v,
            vec![Violation::WasmRawOverBudget {
                measured: 320,
                max: 300
            }]
        );
        let msg = v[0].to_string();
        assert!(msg.contains("320 B") && msg.contains("300 B"), "{msg}");
    }

    #[test]
    fn wire_brotli_over_max_is_a_violation() {
        let m = Measurement {
            wasm_raw: 250,
            wasm_brotli: 80,
            js_raw: 20,
            js_brotli: 30, // wire brotli = 110 > 100
        };
        assert_eq!(
            check_budget(&m, &budget()),
            vec![Violation::WireBrotliOverBudget {
                measured: 110,
                max: 100
            }]
        );
    }

    #[test]
    fn under_floor_is_a_violation_even_when_under_the_max() {
        let m = Measurement {
            wasm_raw: 40, // < 50 floor, and well under the 300 max
            wasm_brotli: 10,
            js_raw: 5,
            js_brotli: 5,
        };
        let v = check_budget(&m, &budget());
        assert_eq!(
            v,
            vec![Violation::WasmRawUnderFloor {
                measured: 40,
                min: 50
            }]
        );
        assert!(v[0].to_string().contains("floor"), "{}", v[0]);
    }

    #[test]
    fn multiple_violations_are_all_reported() {
        let m = Measurement {
            wasm_raw: 320, // over max
            wasm_brotli: 90,
            js_raw: 20,
            js_brotli: 30, // wire brotli 120 > 100
        };
        assert_eq!(check_budget(&m, &budget()).len(), 2);
    }

    #[test]
    fn pick_bg_wasm_accepts_exactly_one() {
        let names = vec![
            "index.html".to_owned(),
            "profile-web-abc123.js".to_owned(),
            "profile-web-abc123_bg.wasm".to_owned(),
        ];
        assert_eq!(pick_bg_wasm(&names).unwrap(), "profile-web-abc123_bg.wasm");
    }

    #[test]
    fn pick_bg_wasm_refuses_zero() {
        let names = vec!["index.html".to_owned(), "app.js".to_owned()];
        assert_eq!(pick_bg_wasm(&names), Err(DistError::NoWasm));
    }

    #[test]
    fn pick_bg_wasm_refuses_more_than_one() {
        let names = vec![
            "profile-web-abc_bg.wasm".to_owned(),
            "profile-web-def_bg.wasm".to_owned(),
        ];
        match pick_bg_wasm(&names) {
            Err(DistError::AmbiguousWasm(hits)) => assert_eq!(hits.len(), 2),
            other => panic!("expected AmbiguousWasm, got {other:?}"),
        }
    }

    #[test]
    fn headroom_rounds_up_to_a_whole_kib() {
        // 100 * 1.10 = 110, rounds up to one whole KiB.
        assert_eq!(ceil_headroom_kib(100), 1024);
        // 300000 * 1.10 = 330000 → next KiB boundary.
        assert_eq!(ceil_headroom_kib(300000), 330000u64.div_ceil(1024) * 1024);
    }

    #[test]
    fn floor_rounds_down_to_a_whole_kib() {
        // 311610 / 2 = 155805 → floor to KiB = 155648.
        assert_eq!(floor_half_kib(311610), 155648);
    }

    #[test]
    fn the_report_names_the_deliberate_raise_duty() {
        let v = vec![Violation::WasmRawOverBudget {
            measured: 320,
            max: 300,
        }];
        let report = format_report(&v);
        assert!(report.contains("wasm-budget.txt"));
        assert!(report.contains("deliberately"));
    }
}
