//! `founding [--commit ID] RELEASE OUT [PROGRAM...]`: writes the founding bundle `make wasm-publish`
//! releases, or, given programs, only each one's packed code (`make wasm-program`). With a commit,
//! each program's code is stamped as built from it (`ducktape.source`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use founding::Source;

const USAGE: &str = "usage: founding [--commit ID] RELEASE_DIR OUT_DIR [PROGRAM...]";

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let (source, args) = match args.as_slice() {
        [flag, commit, rest @ ..] if flag.as_os_str() == "--commit" => {
            let Some(commit) = commit.to_str() else {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            };
            let source = Source {
                repo: founding::REPO.into(),
                commit: commit.into(),
            };
            (Some(source), rest)
        }
        rest => (None, rest),
    };
    let [release, out, programs @ ..] = args else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let written = match programs {
        [] => founding::bundle(release, out, source.as_ref()),
        named => each(release, out, named, source.as_ref()),
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("founding: {error}");
            ExitCode::FAILURE
        }
    }
}

fn each(
    release: &Path,
    out: &Path,
    programs: &[PathBuf],
    source: Option<&Source>,
) -> founding::Result<()> {
    std::fs::create_dir_all(out)?;
    for program in programs {
        let program = program.to_str().ok_or("a program's name is UTF-8")?;
        let packed = founding::pack(release, program, source)?;
        std::fs::write(out.join(format!("{program}.wasm")), packed)?;
    }
    Ok(())
}
