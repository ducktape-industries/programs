//! `pack --code WASM --describe WASM --schema FILE --tools FILE [--view WASM] [--notify WASM]
//! [--commit ID --repo NAME] OUT`: one program's code with every section a founding embeds, for a
//! program built outside this repository (`tools/pack.sh`), and its code id on stdout.

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

use founding::{Parts, Source};

const USAGE: &str = "usage: pack --code WASM --describe WASM --schema FILE --tools FILE [--view WASM] [--notify WASM] [--commit ID --repo NAME] OUT";

fn main() -> ExitCode {
    let args: Result<Vec<String>, _> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect();
    let Ok(args) = args else {
        eprintln!("pack: every argument is UTF-8");
        return ExitCode::FAILURE;
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pack: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> founding::Result<()> {
    let (out, mut flags) = flags(args)?;
    let code = read(&required(&mut flags, "code")?)?;
    let describe = read(&required(&mut flags, "describe")?)?;
    let schema = read(&required(&mut flags, "schema")?)?;
    let tools = read(&required(&mut flags, "tools")?)?;
    let view = flags.remove("view").map(|path| read(&path)).transpose()?;
    let notify = flags.remove("notify").map(|path| read(&path)).transpose()?;
    let source = match (flags.remove("commit"), flags.remove("repo")) {
        (Some(commit), Some(repo)) => Some(Source { repo, commit }),
        (None, None) => None,
        _ => return Err("--commit and --repo go together".into()),
    };
    if let Some(unknown) = flags.keys().next() {
        return Err(format!("--{unknown} is not a flag of pack; {USAGE}").into());
    }
    let packed = founding::pack_parts(&Parts {
        code: &code,
        view: view.as_deref(),
        describe: &describe,
        schema: &schema,
        tools: &tools,
        notify: notify.as_deref(),
        source: source.as_ref(),
    })?;
    let id = founding::code_id(&packed)?;
    fs::write(out, &packed).map_err(|error| format!("{out}: {error}"))?;
    println!("code id: {} {out}", abi::hex(id.digest()));
    Ok(())
}

fn flags(args: &[String]) -> founding::Result<(&String, BTreeMap<String, String>)> {
    let Some((out, pairs)) = args.split_last() else {
        return Err(USAGE.into());
    };
    let mut flags = BTreeMap::new();
    for pair in pairs.chunks(2) {
        let [flag, value] = pair else {
            return Err(USAGE.into());
        };
        let Some(name) = flag.strip_prefix("--") else {
            return Err(USAGE.into());
        };
        if flags.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(format!("--{name} is given twice").into());
        }
    }
    Ok((out, flags))
}

fn required(flags: &mut BTreeMap<String, String>, name: &str) -> founding::Result<String> {
    match flags.remove(name) {
        Some(value) => Ok(value),
        None => Err(format!("--{name} is missing; {USAGE}").into()),
    }
}

fn read(path: &str) -> founding::Result<Vec<u8>> {
    Ok(fs::read(path).map_err(|error| format!("{path}: {error}"))?)
}
