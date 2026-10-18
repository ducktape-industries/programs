//! `founding RELEASE OUT [PROGRAM...]`: writes the founding bundle `make wasm-publish` releases,
//! or, given programs, only each one's packed code (`make wasm-program`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [release, out, programs @ ..] = args.as_slice() else {
        eprintln!("usage: founding RELEASE_DIR OUT_DIR [PROGRAM...]");
        return ExitCode::FAILURE;
    };
    let written = match programs {
        [] => founding::bundle(release, out),
        named => each(release, out, named),
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("founding: {error}");
            ExitCode::FAILURE
        }
    }
}

fn each(release: &Path, out: &Path, programs: &[PathBuf]) -> founding::Result<()> {
    std::fs::create_dir_all(out)?;
    for program in programs {
        let program = program.to_str().ok_or("a program's name is UTF-8")?;
        let packed = founding::pack(release, program)?;
        std::fs::write(out.join(format!("{program}.wasm")), packed)?;
    }
    Ok(())
}
