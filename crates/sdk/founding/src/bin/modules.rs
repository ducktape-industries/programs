//! `modules PROGRAM`: the view and notifier crates a founding packs into PROGRAM, as the make
//! variables `make wasm-program` narrows `wasm-views` and `wasm-notifiers` to.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program] = args.as_slice() else {
        eprintln!("usage: modules PROGRAM");
        return ExitCode::FAILURE;
    };
    let Some((view, notifier)) = founding::modules(program) else {
        eprintln!("modules: {program} is not a program a founding packs");
        return ExitCode::FAILURE;
    };
    let krate = |module: Option<&str>| module.unwrap_or_default().replace('_', "-");
    println!("VIEWS={} NOTIFIERS={}", krate(view), krate(notifier));
    ExitCode::SUCCESS
}
