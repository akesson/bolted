//! `mise run gen:ffi` — write the committed C# per-language contract suite for one feature (D28).
//!
//!     gen-csharp-suite <feature-src.rs> <binding_ns> <suite_ns> <out.cs>
//!
//! The same function the drift test in `mise run check` calls, so a green check means the committed
//! `.cs` is exactly what this would write. Text in, text out — no dotnet SDK, no boltffi CLI. The
//! values-only fixture that makes the suite runnable is hand-written beside the output, un-generated.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, binding_ns, suite_ns, out] = args.as_slice() else {
        eprintln!("usage: gen-csharp-suite <feature-src.rs> <binding_ns> <suite_ns> <out.cs>");
        return ExitCode::FAILURE;
    };

    match run(Path::new(source), binding_ns, suite_ns, Path::new(out)) {
        Ok(bytes) => {
            println!("wrote {out} ({bytes} bytes)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-csharp-suite: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(source: &Path, binding_ns: &str, suite_ns: &str, out: &Path) -> Result<usize, String> {
    let src = std::fs::read_to_string(source)
        .map_err(|e| format!("reading {}: {e}", source.display()))?;
    let generated = bolted_ffi_gen::csharp_contract_suite(&src, binding_ns, suite_ns)
        .map_err(|e| format!("{}: {e}", source.display()))?;
    std::fs::write(out, &generated).map_err(|e| format!("writing {}: {e}", out.display()))?;
    Ok(generated.len())
}
