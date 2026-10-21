//! `schema-diff OLD NEW`: how NEW's interface differs from OLD's, each a program's code (by its
//! `ducktape.schema` section) or a `.schema` file, and what each code says it was built from.
//! Exits 0 when NEW reads each old byte it still knows as OLD did, 1 when one breaks, 2 on an error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use founding::{Change, Interface};

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [old, new] = args.as_slice() else {
        eprintln!("usage: schema-diff OLD NEW (each a program's .wasm or a .schema file)");
        return ExitCode::from(2);
    };
    match compare(old, new) {
        Ok(breaks) => ExitCode::from(u8::from(breaks)),
        Err(error) => {
            eprintln!("schema-diff: {error}");
            ExitCode::from(2)
        }
    }
}

fn compare(old: &Path, new: &Path) -> founding::Result<bool> {
    let read = |path: &Path| -> founding::Result<Interface> {
        let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        founding::interface(&bytes).map_err(|error| format!("{}: {error}", path.display()).into())
    };
    let (was, is) = (read(old)?, read(new)?);
    println!("old: {} ({})", old.display(), built(&was));
    println!("new: {} ({})", new.display(), built(&is));
    let found = founding::differences(&was.schema, &is.schema)?;
    for difference in &found {
        println!("{difference}");
    }
    let breaking = found
        .iter()
        .filter(|difference| difference.change == Change::Breaking)
        .count();
    let summary = match (found.len(), breaking) {
        (0, _) => "the interfaces are the same".to_owned(),
        (all, 0) => format!(
            "{all} differences, each appended or retired: old bytes of every variant still there read the same"
        ),
        (all, breaking) => {
            format!("{all} differences, {breaking} of them (!) read old bytes otherwise")
        }
    };
    println!("{summary}");
    Ok(breaking > 0)
}

fn built(interface: &Interface) -> String {
    match (&interface.source, interface.code) {
        (Some(source), _) => format!("built from {}@{}", source.repo, source.commit),
        (None, true) => "code that names no commit it was built from".to_owned(),
        (None, false) => "a schema file".to_owned(),
    }
}
