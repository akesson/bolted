//! `mise run gen:ffi` — write the committed FFI source for one feature.
//!
//!     gen-ffi <feature-src.rs> <feature_crate_ident> <out.rs>
//!
//! The same function the drift test in `mise run check` calls, so a green check means the committed
//! file is exactly what this would write.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [source, feature_crate, out] = args.as_slice() else {
        eprintln!("usage: gen-ffi <feature-src.rs> <feature_crate_ident> <out.rs>");
        return ExitCode::FAILURE;
    };

    match run(Path::new(source), feature_crate, Path::new(out)) {
        Ok(bytes) => {
            println!("wrote {out} ({bytes} bytes)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-ffi: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(source: &Path, feature_crate: &str, out: &Path) -> Result<usize, String> {
    let src = std::fs::read_to_string(source)
        .map_err(|e| format!("reading {}: {e}", source.display()))?;
    let generated = bolted_ffi_gen::generate(&src, feature_crate)
        .map_err(|e| format!("{}: {e}", source.display()))?;
    std::fs::write(out, &generated).map_err(|e| format!("writing {}: {e}", out.display()))?;
    Ok(generated.len())
}
