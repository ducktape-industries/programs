use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use wasm_encoder::{CustomSection, Module};

const COMMIT: &str = "4cd69803b2faa3996ce1ddadbbc9b1d6e7641133";

fn module(tag: &str) -> Vec<u8> {
    let mut module = Module::new();
    module.section(&CustomSection {
        name: "tag".into(),
        data: tag.as_bytes().into(),
    });
    module.finish()
}

fn built(dir: &Path) {
    for name in ["chat", "chat_view", "chat.describe", "chat_view.notify"] {
        fs::write(dir.join(format!("{name}.wasm")), module(name)).unwrap();
    }
    founding::write_schemas(dir, &["chat"]).unwrap();
}

fn parts(dir: &Path) -> Vec<OsString> {
    let named = [
        ("--code", "chat.wasm"),
        ("--describe", "chat.describe.wasm"),
        ("--schema", "chat.schema"),
        ("--tools", "chat.tools"),
    ];
    named
        .into_iter()
        .flat_map(|(flag, file)| [flag.into(), dir.join(file).into_os_string()])
        .collect()
}

fn pack(args: &[OsString]) -> (bool, String) {
    let (packed, _, stderr) = pack_printing(args);
    (packed, stderr)
}

fn pack_printing(args: &[OsString]) -> (bool, String, String) {
    let ran = Command::new(env!("CARGO_BIN_EXE_pack"))
        .args(args)
        .output()
        .unwrap();
    (
        ran.status.success(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn pack_writes_the_bytes_a_founding_packs_from_the_same_parts() {
    let dir = tempfile::tempdir().unwrap();
    built(dir.path());
    let out = dir.path().join("packed.wasm");
    let mut args = parts(dir.path());
    for (flag, value) in [
        ("--view", dir.path().join("chat_view.wasm").into_os_string()),
        (
            "--notify",
            dir.path().join("chat_view.notify.wasm").into_os_string(),
        ),
        ("--commit", COMMIT.into()),
        ("--repo", founding::REPO.into()),
    ] {
        args.push(flag.into());
        args.push(value);
    }
    args.push(out.clone().into_os_string());
    let (packed, stdout, stderr) = pack_printing(&args);
    assert!(packed, "{stderr}");
    let source = founding::Source {
        repo: founding::REPO.into(),
        commit: COMMIT.into(),
    };
    let founded = founding::pack(dir.path(), "chat", Some(&source)).unwrap();
    let written = fs::read(&out).unwrap();
    assert_eq!(written, founded);
    let put = blob::frame("program", &written).unwrap();
    let recorded = blob::contract::Entry::of(&put).unwrap();
    let id = abi::hex(recorded.id.digest());
    assert_eq!(stdout, format!("code id: {id} {}\n", out.display()));
}

#[test]
fn a_program_crate_writes_the_schema_and_tools_a_founding_writes_for_it() {
    let dir = tempfile::tempdir().unwrap();
    founding::write_schemas(dir.path(), &["chat"]).unwrap();
    let (schema, tools) = borsh_json::tools::sections::<chat::Chat>().unwrap();
    assert_eq!(schema, fs::read(dir.path().join("chat.schema")).unwrap());
    assert_eq!(tools, fs::read(dir.path().join("chat.tools")).unwrap());
}

#[test]
fn pack_refuses_a_missing_part_an_unknown_flag_and_a_commit_with_no_repo() {
    let dir = tempfile::tempdir().unwrap();
    built(dir.path());
    let out: OsString = dir.path().join("packed.wasm").into_os_string();
    let mut no_tools = parts(dir.path());
    no_tools.truncate(6);
    no_tools.push(out.clone());
    let mut unknown = parts(dir.path());
    unknown.extend(["--name".into(), "chat".into(), out.clone()]);
    let mut no_repo = parts(dir.path());
    no_repo.extend(["--commit".into(), COMMIT.into(), out]);
    for (args, said) in [
        (no_tools, "--tools is missing"),
        (unknown, "--name is not a flag of pack"),
        (no_repo, "--commit and --repo go together"),
    ] {
        let (packed, stderr) = pack(&args);
        assert!(!packed, "{said}");
        assert!(stderr.contains(said), "{said}: {stderr}");
    }
    assert!(!dir.path().join("packed.wasm").exists());
}
