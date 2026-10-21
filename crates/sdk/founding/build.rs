//! Writes founding's program lists (`lists.rs`, which lib.rs includes) and its schema table
//! (`interfaces.rs`, which schema.rs includes) from the crates' own folders: src/derive.rs.

#[path = "src/derive.rs"]
mod derive;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(why) = run() {
        panic!("{why}");
    }
}

fn run() -> Result<(), String> {
    let here =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("cargo set no CARGO_MANIFEST_DIR")?);
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("cargo set no OUT_DIR")?);
    let crates = here.join("../..");
    let found = derive::read(&crates)?;
    let founded = derive::founded(&text(&here.join("founding.toml"))?)?;
    let unlinked = derive::unlinked(&found, &text(&here.join("Cargo.toml"))?)?;
    if !unlinked.is_empty() {
        return Err(format!(
            "founding reads every program's schema from its crate, so crates/sdk/founding/Cargo.toml links each; it lacks {}",
            unlinked.join(", ")
        ));
    }
    let lists = derive::lists(&found, &founded)?;
    let programs: Vec<String> = found.programs.iter().cloned().collect();
    fs::write(out.join("lists.rs"), derive::rust(&lists))
        .map_err(|why| format!("lists.rs: {why}"))?;
    fs::write(out.join("interfaces.rs"), derive::interfaces(&programs))
        .map_err(|why| format!("interfaces.rs: {why}"))?;
    for group in derive::GROUPS {
        println!("cargo::rerun-if-changed={}", crates.join(group).display());
    }
    for manifest in &found.manifests {
        println!("cargo::rerun-if-changed={}", manifest.display());
    }
    for file in ["founding.toml", "Cargo.toml", "src/derive.rs"] {
        println!("cargo::rerun-if-changed={file}");
    }
    Ok(())
}

fn text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()))
}
