//! Founding's program lists, from the crates' own folders: a crate under crates/system or crates/app
//! with a `program` feature is a program, and a view that ships says so in its Cargo.toml
//! (`[package.metadata.ducktape]`: `ships = true`, and `rides = "<program>"` it packs into).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

pub const GROUPS: [&str; 2] = ["system", "app"];

#[derive(Debug, Default)]
pub struct Crates {
    pub programs: BTreeSet<String>,
    pub rides: BTreeMap<String, String>,
    pub alone: BTreeSet<String>,
    pub notifiers: BTreeSet<String>,
    pub manifests: Vec<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Lists {
    pub viewed: Vec<(String, String)>,
    pub unviewed: Vec<String>,
    pub unfounded: Vec<String>,
    pub rides: Vec<(String, String)>,
    pub alone: Vec<String>,
    pub notifiers: Vec<(String, String)>,
}

pub fn read(crates: &Path) -> Result<Crates, String> {
    let mut found = Crates::default();
    for group in GROUPS {
        let dir = crates.join(group);
        let entries = fs::read_dir(&dir).map_err(|why| format!("{}: {why}", dir.display()))?;
        let mut folders: Vec<PathBuf> = entries
            .filter_map(|entry| Some(entry.ok()?.path()))
            .collect();
        folders.sort();
        for folder in folders {
            let manifest = folder.join("Cargo.toml");
            if manifest.is_file() {
                found.add(&folder, &manifest)?;
            }
        }
    }
    Ok(found)
}

impl Crates {
    fn add(&mut self, folder: &Path, manifest: &Path) -> Result<(), String> {
        let at = |why: String| format!("{}: {why}", manifest.display());
        let text = fs::read_to_string(manifest).map_err(|why| at(why.to_string()))?;
        let table: Table = toml::from_str(&text).map_err(|why| at(why.to_string()))?;
        let package = table.get("package").and_then(Value::as_table);
        let name = package
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| at("no package name".to_owned()))?
            .to_owned();
        if folder.file_name().and_then(|folder| folder.to_str()) != Some(name.as_str()) {
            return Err(at(format!(
                "{name} sits in another folder; make names a crate by its folder"
            )));
        }
        let features = table.get("features").and_then(Value::as_table);
        let feature =
            |feature: &str| features.is_some_and(|features| features.contains_key(feature));
        let ducktape = package
            .and_then(|package| package.get("metadata"))
            .and_then(|metadata| metadata.get("ducktape"));
        let ships = ducktape
            .and_then(|ducktape| ducktape.get("ships"))
            .and_then(Value::as_bool);
        let rides = ducktape
            .and_then(|ducktape| ducktape.get("rides"))
            .and_then(Value::as_str);
        self.manifests.push(manifest.to_path_buf());
        if feature("program") {
            self.programs.insert(name.clone());
        }
        if ships != Some(true) {
            return Ok(());
        }
        match rides {
            Some(program) => {
                self.rides.insert(name.clone(), program.to_owned());
            }
            None => {
                self.alone.insert(name.clone());
            }
        }
        if feature("notifier") {
            self.notifiers.insert(name);
        }
        Ok(())
    }
}

pub fn founded(founding: &str) -> Result<Vec<String>, String> {
    let table: Table = toml::from_str(founding).map_err(|why| format!("founding.toml: {why}"))?;
    let programs = table
        .get("programs")
        .and_then(Value::as_array)
        .ok_or("founding.toml founds no [[programs]]")?;
    programs
        .iter()
        .map(|program| {
            let id = program.get("id").and_then(Value::as_str);
            id.map(str::to_owned)
                .ok_or_else(|| "founding.toml: a [[programs]] entry has no id".to_owned())
        })
        .collect()
}

pub fn lists(crates: &Crates, founded: &[String]) -> Result<Lists, String> {
    let mut view_of: BTreeMap<&str, &str> = BTreeMap::new();
    for (view, program) in &crates.rides {
        if !crates.programs.contains(program) {
            return Err(format!(
                "{view} rides {program}, which is no program: no crate under crates/system or crates/app of that name has a `program` feature"
            ));
        }
        if let Some(other) = view_of.insert(program, view) {
            return Err(format!(
                "{other} and {view} both ride {program}: a program packs one view"
            ));
        }
    }
    if let Some(id) = founded.iter().find(|id| !crates.programs.contains(*id)) {
        return Err(format!(
            "founding.toml founds {id}, which is no program crate"
        ));
    }
    let mut lists = Lists::default();
    for id in founded {
        match view_of.get(id.as_str()) {
            Some(view) => lists.viewed.push((id.clone(), module(view))),
            None => lists.unviewed.push(id.clone()),
        }
    }
    let unfounded = crates
        .programs
        .iter()
        .filter(|program| !founded.contains(*program));
    lists.unfounded = unfounded.cloned().collect();
    lists.rides = view_of
        .iter()
        .map(|(program, view)| ((*program).to_owned(), module(view)))
        .collect();
    lists.alone = crates.alone.iter().map(|view| module(view)).collect();
    for view in &crates.notifiers {
        let program = crates.rides.get(view).ok_or_else(|| {
            format!("{view} builds a notifier but rides no program: a founding packs a notifier into the program its view rides")
        })?;
        lists.notifiers.push((program.clone(), module(view)));
    }
    Ok(lists)
}

pub fn unlinked(crates: &Crates, manifest: &str) -> Result<Vec<String>, String> {
    let table: Table =
        toml::from_str(manifest).map_err(|why| format!("founding's Cargo.toml: {why}"))?;
    let links = table.get("dependencies").and_then(Value::as_table);
    let linked =
        |program: &&String| links.is_some_and(|links| links.contains_key(program.as_str()));
    Ok(crates
        .programs
        .iter()
        .filter(|program| !linked(program))
        .cloned()
        .collect())
}

pub fn rust(lists: &Lists) -> String {
    let names =
        |names: &[String]| -> String { names.iter().map(|name| format!("{name:?}, ")).collect() };
    let pairs = |pairs: &[(String, String)]| -> String {
        pairs
            .iter()
            .map(|(program, view)| format!("({program:?}, {view:?}), "))
            .collect()
    };
    [
        format!(
            "pub const PROGRAMS: [(&str, &str); {}] = [{}];\n",
            lists.viewed.len(),
            pairs(&lists.viewed)
        ),
        format!(
            "pub const UNVIEWED: &[&str] = &[{}];\n",
            names(&lists.unviewed)
        ),
        format!(
            "pub const UNFOUNDED: &[&str] = &[{}];\n",
            names(&lists.unfounded)
        ),
        format!(
            "pub const RIDES: &[(&str, &str)] = &[{}];\n",
            pairs(&lists.rides)
        ),
        format!(
            "pub const VIEWS: [&str; {}] = [{}];\n",
            lists.alone.len(),
            names(&lists.alone)
        ),
        format!(
            "pub const NOTIFIERS: [(&str, &str); {}] = [{}];\n",
            lists.notifiers.len(),
            pairs(&lists.notifiers)
        ),
    ]
    .concat()
}

pub fn interfaces(programs: &[String]) -> String {
    let arms: String = programs
        .iter()
        .map(|program| {
            format!(
                "        {program:?} => of::<{}::{}>(),\n",
                module(program),
                title(program)
            )
        })
        .collect();
    format!(
        "fn interface(program: &str) -> Option<(BorshSchemaContainer, Tools)> {{\n    let interface = match program {{\n{arms}        _ => return None,\n    }};\n    Some(interface)\n}}\n"
    )
}

fn module(name: &str) -> String {
    name.replace('-', "_")
}

fn title(name: &str) -> String {
    let word = |word: &str| {
        let mut chars = word.chars();
        let first = chars
            .next()
            .map(|first| first.to_ascii_uppercase().to_string());
        first.unwrap_or_default() + chars.as_str()
    };
    name.split('-').map(word).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = "[features]\nprogram = []\n";
    const RIDES_CHAT: &str = "[package.metadata.ducktape]\nships = true\nrides = \"chat\"\n";
    const FOUNDING: &str = "[[programs]]\nid = \"base\"\n\n[[programs]]\nid = \"chat\"\n";

    fn write(root: &Path, at: &str, name: &str, body: &str) {
        let dir = root.join(at);
        fs::create_dir_all(&dir).unwrap();
        let manifest = format!("[package]\nname = \"{name}\"\n{body}");
        fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    }

    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        write(path, "system/base", "base", PROGRAM);
        write(path, "app/chat", "chat", PROGRAM);
        write(path, "app/lone", "lone", PROGRAM);
        let notifier = format!("[features]\nnotifier = []\n{RIDES_CHAT}");
        write(path, "app/chat-view", "chat-view", &notifier);
        let alone = "[package.metadata.ducktape]\nships = true\n";
        write(path, "app/explorer-view", "explorer-view", alone);
        write(path, "app/example-view", "example-view", "[dependencies]\n");
        root
    }

    fn pair(program: &str, view: &str) -> (String, String) {
        (program.to_owned(), view.to_owned())
    }

    fn derived(root: &Path) -> Result<Lists, String> {
        lists(&read(root)?, &founded(FOUNDING)?)
    }

    #[test]
    fn the_lists_come_from_the_crates_folders() {
        let root = tree();
        assert_eq!(read(root.path()).unwrap().manifests.len(), 6);
        let lists = derived(root.path()).unwrap();
        assert_eq!(lists.viewed, [pair("chat", "chat_view")]);
        assert_eq!(lists.unviewed, ["base"]);
        assert_eq!(lists.unfounded, ["lone"]);
        assert_eq!(lists.rides, [pair("chat", "chat_view")]);
        assert_eq!(lists.alone, ["explorer_view"]);
        assert_eq!(lists.notifiers, [pair("chat", "chat_view")]);
    }

    #[test]
    fn a_new_program_crate_is_found_by_its_folder_and_founding_asks_only_its_link() {
        let root = tree();
        let links = "[dependencies]\nbase = { path = \"b\" }\nchat = { path = \"c\" }\nlone = { path = \"l\" }\n";
        assert!(
            unlinked(&read(root.path()).unwrap(), links)
                .unwrap()
                .is_empty()
        );
        write(root.path(), "app/zed-pad", "zed-pad", PROGRAM);
        let crates = read(root.path()).unwrap();
        let lists = derived(root.path()).unwrap();
        assert_eq!(lists.unfounded, ["lone", "zed-pad"]);
        assert!(
            rust(&lists).contains("pub const UNFOUNDED: &[&str] = &[\"lone\", \"zed-pad\", ];")
        );
        let programs: Vec<String> = crates.programs.iter().cloned().collect();
        assert!(interfaces(&programs).contains("\"zed-pad\" => of::<zed_pad::ZedPad>(),"));
        assert_eq!(unlinked(&crates, links).unwrap(), ["zed-pad"]);
    }

    #[test]
    fn a_view_that_rides_no_program_or_a_taken_one_is_refused() {
        let root = tree();
        write(root.path(), "app/chat-pad", "chat-pad", RIDES_CHAT);
        let refused = derived(root.path()).unwrap_err();
        assert_eq!(
            refused,
            "chat-pad and chat-view both ride chat: a program packs one view"
        );
        fs::remove_dir_all(root.path().join("app/chat-pad")).unwrap();
        let gone = RIDES_CHAT.replace("chat", "gone");
        write(root.path(), "app/gone-view", "gone-view", &gone);
        let refused = derived(root.path()).unwrap_err();
        assert!(
            refused.starts_with("gone-view rides gone, which is no program"),
            "{refused}"
        );
    }

    #[test]
    fn founding_what_no_crate_holds_or_a_crate_out_of_its_folder_is_refused() {
        let root = tree();
        let crates = read(root.path()).unwrap();
        let refused = lists(&crates, &["gone".to_owned()]).unwrap_err();
        assert_eq!(
            refused,
            "founding.toml founds gone, which is no program crate"
        );
        write(root.path(), "app/odd", "even", "");
        let refused = read(root.path()).unwrap_err();
        let why = "even sits in another folder; make names a crate by its folder";
        assert!(refused.ends_with(why), "{refused}");
    }

    #[test]
    fn a_notifier_needs_the_program_its_view_rides() {
        let root = tree();
        let alone = "[features]\nnotifier = []\n[package.metadata.ducktape]\nships = true\n";
        write(root.path(), "app/explorer-view", "explorer-view", alone);
        let refused = derived(root.path()).unwrap_err();
        assert!(
            refused.starts_with("explorer-view builds a notifier but rides no program"),
            "{refused}"
        );
    }
}
