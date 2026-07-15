//! `wasm-budget` — measure a built web `dist/` and (optionally) enforce a committed size budget.
//!
//! Behind the `budget` cargo feature (`[[bin]] required-features`), so the brotli compression
//! dependency never enters a build that did not ask for it — in particular `mise run check`, which
//! never enables the feature. All the policy (parsing, comparison, dist selection, formatting) lives
//! in [`bolted_check::budget`] and is unit-tested inside plain `cargo test --workspace`; this bin is
//! the thin CLI that adds the filesystem walk and the exit code.
//!
//! ```text
//! wasm-budget --print <dist>            # measure; print raw + brotli-q11 for wasm/glue/wire
//! wasm-budget check  <dist> <budget>    # measure; assert against a committed budget file
//! ```
//!
//! Exit codes: 0 within budget, 1 over budget or a dist/budget-file error, 2 on a usage error.
#![forbid(unsafe_code)]

use bolted_check::budget;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, dist] if flag == "--print" => run_print(Path::new(dist)),
        [cmd, dist, budget_file] if cmd == "check" => {
            run_check(Path::new(dist), Path::new(budget_file))
        }
        _ => {
            eprintln!(
                "usage:\n  wasm-budget --print <dist>\n  wasm-budget check <dist> <budget-file>"
            );
            ExitCode::from(2)
        }
    }
}

fn run_print(dist: &Path) -> ExitCode {
    match budget::measure(dist) {
        Ok(m) => {
            print!("{}", budget::render_measurement(&m));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("wasm-budget: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(dist: &Path, budget_file: &Path) -> ExitCode {
    let text = match std::fs::read_to_string(budget_file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "wasm-budget: cannot read budget file {}: {e}",
                budget_file.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let budget = match budget::parse_budget(&text) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "wasm-budget: malformed budget file {}: {e}",
                budget_file.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let measurement = match budget::measure(dist) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("wasm-budget: {e}");
            return ExitCode::FAILURE;
        }
    };

    print!("{}", budget::render_measurement(&measurement));
    let violations = budget::check_budget(&measurement, &budget);
    if violations.is_empty() {
        println!("\nwasm size budget OK — within the committed limits.");
        ExitCode::SUCCESS
    } else {
        eprint!("{}", budget::format_report(&violations));
        ExitCode::FAILURE
    }
}
