//! `schemas RELEASE PROGRAM...`: writes `<program>.schema` and `<program>.tools` for each program
//! named, which the bundle packs.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(release) = args.next().map(PathBuf::from) else {
        eprintln!("usage: schemas RELEASE_DIR PROGRAM...");
        return ExitCode::FAILURE;
    };
    let programs: Vec<String> = args.collect();
    match founding::write_schemas(&release, &programs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("schemas: {error}");
            ExitCode::FAILURE
        }
    }
}
