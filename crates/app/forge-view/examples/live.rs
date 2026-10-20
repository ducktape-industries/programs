//! `cargo run -p forge-view --example live`: Forge in the real app over a
//! fake node with a repository and the people who may push to it, under the
//! bounds a founding gives forge. `web` holds a history: a long branch and
//! tag list, a long source file, three open changes (a large diff, a small
//! one, and a release the size of a real one: 280 files, +78k −21k,
//! binaries, a file too long to diff), and a past of 300 more: 60 open, 60
//! closed and 180 merged into `dev`; `docs` is empty; `mine` is the
//! reader's own.
use forge::{Op, Revision};
use gitcore::wire::pktline;
use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Objects, Oid, Signature, Tree, TreeEntry, pack,
};
use view::rig;

fn main() {
    rig::run("forge", |net| {
        net.seat::<chat::Chat>(chat::PROGRAM);
        net.seat::<forge::Forge>(forge::PROGRAM);
        net.seat::<access::Access>(access::PROGRAM);
        net.init(forge::PROGRAM, &founding::forge_bounds())?;
        let ada = net.person("ada");
        for repo in ["web", "docs"] {
            net.submit(
                ada,
                forge::PROGRAM,
                &Op::Create {
                    repo: repo.into(),
                    hash: abi::HashKind::Sha256,
                },
            )?;
            net.submit(
                ada,
                access::PROGRAM,
                &access::Op::Set {
                    program: forge::PROGRAM.into(),
                    key: forge::repo_key(repo),
                    list: Some(identity::Ownership::Listed(vec![identity::Party::Account(
                        net.me(),
                    )])),
                },
            )?;
        }
        net.submit(
            net.me(),
            forge::PROGRAM,
            &Op::Create {
                repo: "mine".into(),
                hash: abi::HashKind::Sha256,
            },
        )?;
        let History {
            request,
            pack,
            main,
            branches,
        } = history();
        let pack = net.hold(forge::PACK_KIND, &pack)?;
        for (who, repo) in [(net.me(), "mine"), (ada, "web")] {
            net.submit(
                who,
                forge::PROGRAM,
                &Op::Push {
                    repo: repo.into(),
                    request: request.clone(),
                    pack: Some(pack),
                },
            )?;
        }
        for (from, title) in [
            ("refactor/split-every-module", "Split every module in two"),
            ("feature/search-0", "Search the index"),
            ("release", "Release 0.2"),
        ] {
            net.submit(
                ada,
                forge::PROGRAM,
                &Op::ChangeOpen {
                    repo: "web".into(),
                    from: Revision::Ref(format!("refs/heads/{from}").into_bytes()),
                    into: b"refs/heads/main".to_vec(),
                    title: title.into(),
                    body: String::new(),
                    reviewers: Vec::new(),
                },
            )?;
        }
        past(net, ada, main, &branches)?;
        Ok(())
    })
}

const PAST: usize = 300;

fn past(
    net: &rig::Network,
    ada: program::AccountNumber,
    main: Oid,
    branches: &[(String, Oid)],
) -> Result<(), view::host::Error> {
    let verbs = [
        "Fix", "Speed up", "Tidy", "Document", "Test", "Rename", "Split", "Guard",
    ];
    let things = [
        "the cache key",
        "search ranking",
        "the login form",
        "theme tokens",
        "the CI matrix",
        "the docs index",
        "a deps bump",
        "the page layout",
        "a slow render",
    ];
    let mut dev = main;
    for i in 0..PAST {
        let (name, tip) = &branches[i % branches.len()];
        let merged = i % 5 < 3;
        let into = match merged {
            true => "refs/heads/dev",
            false => "refs/heads/main",
        };
        let from = Revision::Ref(format!("refs/heads/{name}").into_bytes());
        let opened = net.submit(
            ada,
            forge::PROGRAM,
            &Op::ChangeOpen {
                repo: "web".into(),
                from: from.clone(),
                into: into.as_bytes().to_vec(),
                title: format!(
                    "{} {} ({})",
                    verbs[i % verbs.len()],
                    things[i % things.len()],
                    i + 1
                ),
                body: String::new(),
                reviewers: Vec::new(),
            },
        )?;
        let forge::OpReply::Change { n, .. } = borsh::from_slice(&opened).expect("a change") else {
            panic!("ChangeOpen answers its change");
        };
        let op = match (merged, i % 5) {
            (true, _) => {
                let op = Op::Merge {
                    repo: "web".into(),
                    into: into.as_bytes().to_vec(),
                    from,
                    expected_into: dev.to_string(),
                    expected_from: tip.to_string(),
                    result: tip.to_string(),
                    change: Some(n),
                };
                dev = *tip;
                op
            }
            (false, 3) => Op::ChangeClose {
                repo: "web".into(),
                n,
            },
            _ => continue,
        };
        net.submit(ada, forge::PROGRAM, &op)?;
    }
    Ok(())
}

const PROGRAMS: usize = 30;
const LINES: usize = 400;
const LEGACY: usize = 52;

type File = (String, Oid);

struct History {
    request: Vec<u8>,
    pack: Vec<u8>,
    main: Oid,
    branches: Vec<(String, Oid)>,
}

fn history() -> History {
    let mut store = MemoryObjects::new(Hash::Sha256);
    let s = &mut store;
    let readme = blob(s, b"# web\n\nThe site, its search and its docs.\n");
    let lib = blob(s, &source("lib", 1, 1200));
    let programs: Vec<File> = (0..PROGRAMS)
        .map(|n| {
            (
                format!("src/mod_{n:02}.rs"),
                blob(s, &source("m", n, LINES)),
            )
        })
        .collect();
    let legacy: Vec<File> = (0..LEGACY)
        .map(|n| {
            (
                format!("legacy/d{}/old_{n:02}.rs", n % 6),
                blob(s, &source("old", n, LINES)),
            )
        })
        .collect();
    let fonts: Vec<(String, Oid)> = ["Sans", "Mono"]
        .iter()
        .map(|name| (format!("{name}.ttf"), blob(s, &binary_megabyte(name.len()))))
        .collect();
    let lock = blob(s, &source("lock", 0, 3600));
    let base = |fonts_dir: &str, lock: Oid, programs: &[File]| {
        let mut files = vec![
            ("README.md".to_owned(), readme),
            ("src/lib.rs".to_owned(), lib),
            ("Cargo.lock".to_owned(), lock),
        ];
        files.extend(programs.iter().cloned());
        files.extend(
            fonts
                .iter()
                .map(|(name, id)| (format!("{fonts_dir}/{name}"), *id)),
        );
        files
    };
    let mut main_files = base("assets/fonts", lock, &programs);
    main_files.extend(legacy.iter().cloned());
    let main_tree = tree_of(s, &main_files);
    let main = commit(s, main_tree, &[], "Start the site");

    let zero = Hash::Sha256.zero();
    let mut commands = vec![
        (zero, main, "refs/heads/main".to_owned()),
        (zero, main, "refs/heads/dev".to_owned()),
    ];
    let mut branches = Vec::new();
    let words = [
        "search", "login", "cache", "theme", "ci", "docs", "deps", "layout", "perf",
    ];
    for n in 0..90 {
        let name = match n % 3 {
            0 => format!("feature/{}-{n}", words[n % words.len()]),
            1 => format!("fix/{}-{n}", words[n % words.len()]),
            _ => format!(
                "people/ada/a-rather-long-branch-name-about-the-{}-work-{n}",
                words[n % words.len()]
            ),
        };
        let readme = blob(
            s,
            format!("# web\n\nThe site, its search and its docs.\n\n{name}\n").as_bytes(),
        );
        let mut files = main_files.clone();
        files[0] = ("README.md".to_owned(), readme);
        let tree = tree_of(s, &files);
        let tip = commit(s, tree, &[main], &name);
        commands.push((zero, tip, format!("refs/heads/{name}")));
        branches.push((name, tip));
    }
    for n in 1..=12 {
        commands.push((zero, main, format!("refs/tags/v0.{n}.0")));
    }

    let mut split = Vec::new();
    for n in 0..PROGRAMS {
        split.push((
            format!("src/mod_{n:02}.rs"),
            blob(s, &source("a", n, LINES)),
        ));
        split.push((
            format!("src/mod_{n:02}_b.rs"),
            blob(s, &source("b", n, LINES)),
        ));
    }
    let mut files = base("assets/fonts", lock, &split);
    files.extend(legacy.iter().cloned());
    let tree = tree_of(s, &files);
    let tip = commit(s, tree, &[main], "Split every module in two");
    commands.push((
        zero,
        tip,
        "refs/heads/refactor/split-every-module".to_owned(),
    ));

    let mut touched = Vec::new();
    for (n, (path, _)) in programs.iter().enumerate() {
        let text = match n {
            0 => insert_after_every_line("m", n, LINES),
            n if n < 15 => edit_every_fortieth_line("m", n, LINES),
            _ => source("m", n, LINES),
        };
        touched.push((path.clone(), blob(s, &text)));
    }
    let lock = blob(s, &edit_every_fortieth_line("lock", 0, 3600));
    let mut files = base("app/assets/fonts", lock, &touched);
    let areas = [
        "ax", "render", "shell", "runtime", "editor", "backend", "tree", "ops",
    ];
    let parts = ["audit", "layers", "entities", "inputs", "seat", "tests"];
    for i in 0..192 {
        let lines = if i < 15 {
            1100 + i * 75
        } else {
            50 + (i * 37) % 500
        };
        let path = format!("src/{}/{}/new_{i:03}.rs", areas[i % 8], parts[(i / 8) % 6]);
        files.push((path, blob(s, &source("new", i, lines))));
    }
    let side = commit(s, main_tree, &[main], "Branch off for the release");
    let tree = tree_of(s, &files);
    let tip = commit(
        s,
        tree,
        &[main, side],
        "Merge pull request #255 from web/dev",
    );
    commands.push((zero, tip, "refs/heads/release".to_owned()));

    let ids: Vec<Oid> = store.ids().copied().collect();
    let objects: Vec<_> = ids
        .iter()
        .map(|id| store.get(id).unwrap().unwrap())
        .collect();
    let pack = pack::write(objects, Hash::Sha256).unwrap();
    let mut request = Vec::new();
    for (index, (old, new, name)) in commands.iter().enumerate() {
        let mut line = format!("{old} {new} {name}").into_bytes();
        if index == 0 {
            line.extend_from_slice(b"\0report-status side-band-64k");
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    History {
        request,
        pack,
        main,
        branches,
    }
}

fn source(prefix: &str, n: usize, lines: usize) -> Vec<u8> {
    let mut out = format!("//! {prefix} {n}: generated for the live example.\n\n");
    for line in 0..lines {
        out += &format!(
            "pub fn {prefix}_{n}_{line}(x: u64) -> u64 {{ x.wrapping_mul({line}).rotate_left({n}) ^ 0x{:x} }}\n",
            line * 7919 + n
        );
    }
    out.into_bytes()
}

fn edit_every_fortieth_line(prefix: &str, n: usize, lines: usize) -> Vec<u8> {
    let text = String::from_utf8(source(prefix, n, lines)).unwrap();
    text.lines()
        .enumerate()
        .map(|(i, line)| match i % 40 {
            7 => format!("{line} // edited\n"),
            _ => format!("{line}\n"),
        })
        .collect::<String>()
        .into_bytes()
}

fn insert_after_every_line(prefix: &str, n: usize, lines: usize) -> Vec<u8> {
    let text = String::from_utf8(source(prefix, n, lines)).unwrap();
    text.lines()
        .map(|line| format!("{line}\n// between\n"))
        .collect::<String>()
        .into_bytes()
}

fn binary_megabyte(seed: usize) -> Vec<u8> {
    let mut state = seed as u32 + 1;
    (0..1 << 20)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

fn blob(store: &mut MemoryObjects, content: &[u8]) -> Oid {
    store.put(Kind::Blob, content).unwrap()
}

fn tree_of(store: &mut MemoryObjects, files: &[File]) -> Oid {
    let mut dirs: std::collections::BTreeMap<&str, Vec<File>> = Default::default();
    let mut entries = Vec::new();
    for (path, id) in files {
        match path.split_once('/') {
            Some((dir, rest)) => dirs.entry(dir).or_default().push((rest.to_owned(), *id)),
            None => entries.push(entry(Mode::Regular, path, *id)),
        }
    }
    for (dir, files) in dirs {
        let id = tree_of(store, &files);
        entries.push(entry(Mode::Directory, dir, id));
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    tree(store, entries)
}

fn entry(mode: Mode, name: &str, id: Oid) -> TreeEntry {
    TreeEntry {
        mode,
        name: name.as_bytes().to_vec(),
        id,
    }
}

fn tree(store: &mut MemoryObjects, entries: Vec<TreeEntry>) -> Oid {
    store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap()
}

fn commit(store: &mut MemoryObjects, tree: Oid, parents: &[Oid], message: &str) -> Oid {
    let ada = Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time: 1_791_000_000,
        offset_minutes: 0,
    };
    let commit = Commit {
        tree,
        parents: parents.to_vec(),
        author: ada.clone(),
        committer: ada,
        extra: Vec::new(),
        message: format!("{message}\n").into_bytes(),
    };
    store.put(Kind::Commit, &commit.serialize()).unwrap()
}
