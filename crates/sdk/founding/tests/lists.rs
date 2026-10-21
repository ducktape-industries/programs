//! The Makefile's PROGRAMS, VIEWS and NOTIFIERS are founding's lists: both read them from the
//! crates' folders, so what the one finds the other finds too.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn make(variable: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut make = Command::new("make")
        .args([
            "-s",
            "--no-print-directory",
            "-f",
            "Makefile",
            "-f",
            "-",
            "lists",
        ])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let rule = format!("lists:\n\t@echo $({variable})\n");
    make.stdin
        .take()
        .unwrap()
        .write_all(rule.as_bytes())
        .unwrap();
    let output = make.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut names: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

fn crates<'a>(modules: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut names: Vec<String> = modules.map(|module| module.replace('_', "-")).collect();
    names.sort();
    names
}

#[test]
fn make_finds_the_programs_founding_finds() {
    let viewed = founding::PROGRAMS.iter().map(|(program, _)| *program);
    let every = viewed
        .chain(founding::UNVIEWED.iter().copied())
        .chain(founding::UNFOUNDED.iter().copied());
    assert_eq!(make("PROGRAMS"), crates(every));
}

#[test]
fn make_finds_the_views_and_notifiers_founding_packs() {
    let views = founding::RIDES
        .iter()
        .map(|(_, view)| *view)
        .chain(founding::VIEWS);
    assert_eq!(make("VIEWS"), crates(views));
    let notifiers = founding::NOTIFIERS.iter().map(|(_, view)| *view);
    assert_eq!(make("NOTIFIERS"), crates(notifiers));
}
