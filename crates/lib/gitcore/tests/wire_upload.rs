// Protocol v2 fetch tests: capability advertisement, ls-refs, the closure a fetch lists in pages, and reading back the streamed pack.

#[path = "common/mod.rs"]
mod common;

use common::{fixture, git_binary, oid_list, oid_text, sha1, temp_dir};
use gitcore::server::admit_pack;
use gitcore::walk::{Listed, Listing, Walk, closure};
use gitcore::wire::pktline::{self, Pkt, Reader};
use gitcore::wire::upload::{
    Command, Fetch, LsRefs, ShallowInfo, capability_advertisement, ls_refs_response, parse_command,
    respond, respond_shallow,
};
use gitcore::{Error, Hash, Kind, Limits, MemoryObjects, Objects, Oid, pack};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

fn repo() -> (MemoryObjects, BTreeMap<Vec<u8>, Oid>, Oid, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    admit_pack(
        &mut store,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    admit_pack(
        &mut store,
        fixture!("thin.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    let tag = store.put(Kind::Tag, fixture!("tag.bin")).unwrap();
    assert_eq!(tag, oid_text(fixture!("tag.oid"), Hash::Sha1));
    let base = oid_text(fixture!("base.oid"), Hash::Sha1);
    let tip = oid_text(fixture!("tip.oid"), Hash::Sha1);
    let mut refs = BTreeMap::new();
    refs.insert(b"refs/heads/main".to_vec(), tip);
    refs.insert(b"refs/heads/old".to_vec(), base);
    refs.insert(b"refs/tags/v1".to_vec(), tag);
    (store, refs, base, tip, tag)
}

fn request(command: &str, args: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, format!("command={command}").as_bytes());
    pktline::push_line(&mut out, b"agent=git/2.55.0");
    pktline::push_line(&mut out, b"object-format=sha1");
    out.extend_from_slice(pktline::delim());
    for arg in args {
        pktline::push_line(&mut out, arg.as_bytes());
    }
    out.extend_from_slice(pktline::flush());
    out
}

fn lines(bytes: &[u8]) -> Vec<String> {
    Reader::new(bytes)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            Pkt::Flush => "<flush>".into(),
            Pkt::Delim => "<delim>".into(),
            Pkt::ResponseEnd => "<end>".into(),
        })
        .collect()
}

#[test]
fn capability_advertisement_golden() {
    let body = capability_advertisement(Hash::Sha1, b"ducktape/0.1");
    assert_eq!(
        lines(&body),
        [
            "version 2",
            "agent=ducktape/0.1",
            "ls-refs=unborn",
            "fetch=wait-for-done",
            "server-option",
            "object-format=sha1",
            "<flush>"
        ]
    );
    assert!(
        lines(&capability_advertisement(Hash::Sha256, b"x"))
            .contains(&"object-format=sha256".to_string())
    );
}

#[test]
fn parse_commands() {
    let ls = parse_command(
        &request(
            "ls-refs",
            &[
                "symrefs",
                "peel",
                "unborn",
                "ref-prefix HEAD",
                "ref-prefix refs/heads/",
            ],
        ),
        Hash::Sha1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        ls,
        Command::LsRefs(LsRefs {
            symrefs: true,
            peel: true,
            unborn: true,
            ref_prefixes: vec![b"HEAD".to_vec(), b"refs/heads/".to_vec()],
        })
    );
    let no_args = parse_command(b"0014command=ls-refs\n0000", Hash::Sha1)
        .unwrap()
        .unwrap();
    assert_eq!(no_args, Command::LsRefs(LsRefs::default()));

    let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let fetched = parse_command(
        &request(
            "fetch",
            &[
                "thin-pack",
                "no-progress",
                "ofs-delta",
                "include-tag",
                &format!("want {a}"),
                &format!("have {b}"),
                "done",
            ],
        ),
        Hash::Sha1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        fetched,
        Command::Fetch(Fetch {
            wants: vec![sha1(a)],
            haves: vec![sha1(b)],
            done: true,
            thin_pack: true,
            no_progress: true,
            include_tag: true,
            ofs_delta: true,
            sideband_all: false,
            wait_for_done: false,
            deepen: None,
            shallows: Vec::new(),
        })
    );

    let deepened = parse_command(
        &request(
            "fetch",
            &[
                "deepen 3",
                &format!("want {a}"),
                &format!("shallow {b}"),
                "done",
            ],
        ),
        Hash::Sha1,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        deepened,
        Command::Fetch(Fetch {
            wants: vec![sha1(a)],
            done: true,
            deepen: Some(3),
            shallows: vec![sha1(b)],
            ..Fetch::default()
        })
    );
    for refused in ["deepen 0", "deepen x"] {
        assert_eq!(
            parse_command(&request("fetch", &[refused]), Hash::Sha1),
            Err(Error::BadRequest),
            "{refused}"
        );
    }
    for unsupported in [
        "deepen-since 1700000000",
        "deepen-not refs/heads/main",
        "deepen-relative",
    ] {
        assert_eq!(
            parse_command(&request("fetch", &[unsupported]), Hash::Sha1),
            Err(Error::Unsupported),
            "{unsupported}"
        );
    }
    assert_eq!(
        parse_command(&request("fetch", &["filter blob:none"]), Hash::Sha1),
        Err(Error::Unsupported)
    );
    assert_eq!(
        parse_command(&request("fetch", &["want-ref refs/heads/main"]), Hash::Sha1),
        Err(Error::Unsupported)
    );
    assert_eq!(
        parse_command(&request("fetch", &["bogus"]), Hash::Sha1),
        Err(Error::BadRequest)
    );
    assert_eq!(
        parse_command(&request("push", &[]), Hash::Sha1),
        Err(Error::UnknownCommand)
    );
    assert_eq!(parse_command(b"0000", Hash::Sha1), Ok(None));
    assert_eq!(parse_command(b"", Hash::Sha1), Ok(None));
    assert_eq!(
        parse_command(b"0015agent=git/2.55.0\n0000", Hash::Sha1),
        Err(Error::BadRequest)
    );
    assert_eq!(
        parse_command(&request("fetch", &[]), Hash::Sha256),
        Err(Error::WrongHash {
            expected: Hash::Sha256,
            actual: Hash::Sha1
        })
    );
}

#[test]
fn ls_refs_lists_head_symref_and_peeled_tags() {
    let (store, refs, base, tip, tag) = repo();
    let command = LsRefs {
        symrefs: true,
        peel: true,
        unborn: true,
        ref_prefixes: Vec::new(),
    };
    let body = ls_refs_response(&store, &refs, Some(b"refs/heads/main"), &command, 10).unwrap();
    assert_eq!(
        lines(&body),
        [
            format!("{tip} HEAD symref-target:refs/heads/main"),
            format!("{tip} refs/heads/main"),
            format!("{base} refs/heads/old"),
            format!("{tag} refs/tags/v1 peeled:{base}"),
            "<flush>".into(),
        ]
    );

    let unborn = ls_refs_response(&store, &refs, Some(b"refs/heads/nope"), &command, 10).unwrap();
    assert_eq!(
        lines(&unborn)[0],
        "unborn HEAD symref-target:refs/heads/nope"
    );
    let no_unborn = LsRefs {
        unborn: false,
        ..command.clone()
    };
    let hidden = ls_refs_response(&store, &refs, Some(b"refs/heads/nope"), &no_unborn, 10).unwrap();
    assert!(lines(&hidden)[0].ends_with("refs/heads/main"));

    let only_tags = LsRefs {
        ref_prefixes: vec![b"refs/tags/".to_vec()],
        ..LsRefs::default()
    };
    let body = ls_refs_response(&store, &refs, Some(b"refs/heads/main"), &only_tags, 10).unwrap();
    assert_eq!(
        lines(&body),
        [format!("{tag} refs/tags/v1"), "<flush>".into()]
    );
    assert_eq!(
        lines(&ls_refs_response(&store, &BTreeMap::new(), None, &command, 10).unwrap()),
        ["<flush>"]
    );
}

struct Response {
    sections: Vec<String>,
    pack: Vec<u8>,
}

struct Budgeted<'a> {
    store: &'a MemoryObjects,
    reads: Cell<u64>,
}

impl Objects for Budgeted<'_> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<gitcore::Object>> {
        let left = self.reads.get();
        if left == 0 {
            return Err(Error::CapReached);
        }
        self.reads.set(left - 1);
        self.store.get(id)
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        self.store.has(id)
    }
}

fn listing(store: &MemoryObjects, command: &Fetch, reads: u64) -> (Vec<Listed>, usize) {
    let mut pages = 0;
    let mut listing = Listing::of(&command.wants, &command.haves);
    while let Some(walk) = listing.pending() {
        pages += 1;
        let budgeted = Budgeted {
            store,
            reads: Cell::new(reads),
        };
        listing.receive(closure(&budgeted, walk, usize::MAX).unwrap());
    }
    (listing.into_listed(), pages)
}

fn run_fetch(store: &MemoryObjects, command: &Fetch) -> Response {
    let (listed, _) = listing(store, command, 1000);
    let objects = listed
        .iter()
        .map(|item| Ok(store.get(&item.id)?.expect("listed objects are held")));
    let mut bytes = Vec::new();
    let mut sink = |chunk: &[u8]| bytes.extend_from_slice(chunk);
    respond(command, objects, &mut sink).unwrap();
    parsed(&bytes)
}

fn parsed(bytes: &[u8]) -> Response {
    let mut sections = Vec::new();
    let mut pack = Vec::new();
    for pkt in Reader::new(bytes) {
        match pkt.unwrap() {
            Pkt::Data(data) if sections.last().is_some_and(|s| s == "packfile") => {
                assert_eq!(data[0], 1);
                pack.extend_from_slice(&data[1..]);
            }
            Pkt::Data(data) => {
                sections.push(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            Pkt::Flush => sections.push("<flush>".into()),
            Pkt::Delim => sections.push("<delim>".into()),
            Pkt::ResponseEnd => sections.push("<end>".into()),
        }
    }
    Response { sections, pack }
}

fn read_pack(bytes: &[u8]) -> BTreeSet<Oid> {
    pack::read(bytes, Hash::Sha1, &Limits::generous(), |_| Ok(None))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn a_fetch_packs_what_the_closure_lists_for_a_clone_and_an_update() {
    let (store, _, base, tip, tag) = repo();
    let base_objects: BTreeSet<Oid> = oid_list(fixture!("base.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    let delta_objects: BTreeSet<Oid> = oid_list(fixture!("delta.oids"), Hash::Sha1)
        .into_iter()
        .collect();

    let clone = run_fetch(
        &store,
        &Fetch {
            wants: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(clone.sections, ["packfile", "<flush>"]);
    let mut everything = base_objects.clone();
    everything.extend(delta_objects.iter().copied());
    assert_eq!(read_pack(&clone.pack), everything);
    let mut client = MemoryObjects::new(Hash::Sha1);
    admit_pack(&mut client, &clone.pack, Hash::Sha1, &Limits::generous()).unwrap();
    assert!(client.has(&tip).unwrap());

    let update = run_fetch(
        &store,
        &Fetch {
            wants: vec![tip],
            haves: vec![base, sha1("1234567812345678123456781234567812345678")],
            ..Fetch::default()
        },
    );
    assert_eq!(
        update.sections,
        [
            "acknowledgments",
            "NAK",
            "ready",
            "<delim>",
            "packfile",
            "<flush>"
        ]
    );
    assert_eq!(read_pack(&update.pack), delta_objects);
    let mut client = MemoryObjects::new(Hash::Sha1);
    admit_pack(
        &mut client,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    admit_pack(&mut client, &update.pack, Hash::Sha1, &Limits::generous()).unwrap();
    assert!(client.has(&tip).unwrap());

    let up_to_date = run_fetch(
        &store,
        &Fetch {
            wants: vec![tip],
            haves: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(up_to_date.sections, ["packfile", "<flush>"]);
    assert!(read_pack(&up_to_date.pack).is_empty());

    let tagged = run_fetch(
        &store,
        &Fetch {
            wants: vec![tag],
            done: true,
            ..Fetch::default()
        },
    );
    let mut expected = base_objects;
    expected.insert(tag);
    assert_eq!(read_pack(&tagged.pack), expected);

    let tag_over_base = run_fetch(
        &store,
        &Fetch {
            wants: vec![tag],
            haves: vec![base],
            done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(read_pack(&tag_over_base.pack), [tag].into_iter().collect());
}

#[test]
fn a_fetch_waiting_for_done_answers_no_pack() {
    let (store, _, base, tip, _) = repo();
    let waiting = run_fetch(
        &store,
        &Fetch {
            wants: vec![tip],
            haves: vec![base],
            wait_for_done: true,
            ..Fetch::default()
        },
    );
    assert_eq!(waiting.sections, ["acknowledgments", "NAK", "<flush>"]);
    assert!(waiting.pack.is_empty());
}

#[test]
fn a_fetch_without_wants_is_refused() {
    let mut bytes = Vec::new();
    let mut sink = |chunk: &[u8]| bytes.extend_from_slice(chunk);
    let nothing = std::iter::empty::<gitcore::Result<gitcore::Object>>();
    assert_eq!(
        respond(&Fetch::default(), nothing, &mut sink),
        Err(Error::BadRequest)
    );
    assert!(bytes.is_empty());
}

#[test]
fn a_closure_listed_a_few_reads_at_a_time_lists_what_one_page_does() {
    let (store, _, base, tip, _) = repo();
    let clone = Fetch {
        wants: vec![tip],
        ..Fetch::default()
    };
    let (whole, one_page) = listing(&store, &clone, 1000);
    assert_eq!(one_page, 1);
    let (paged, pages) = listing(&store, &clone, 2);
    assert!(pages > 3, "{pages} pages");
    let ids = |listed: &[Listed]| listed.iter().map(|item| item.id).collect::<BTreeSet<_>>();
    assert_eq!(ids(&paged), ids(&whole));
    let update = Fetch {
        wants: vec![tip],
        haves: vec![base],
        ..Fetch::default()
    };
    assert_eq!(
        ids(&listing(&store, &update, 2).0),
        ids(&listing(&store, &update, 1000).0)
    );
}

#[test]
fn a_closure_lists_each_object_with_its_kind() {
    let (store, _, _, tip, tag) = repo();
    let fetch = Fetch {
        wants: vec![tag, tip],
        ..Fetch::default()
    };
    for item in listing(&store, &fetch, 1000).0 {
        let object = store.get(&item.id).unwrap().unwrap();
        assert_eq!(object.kind, item.kind, "{}", item.id);
    }
}

#[test]
fn a_closure_refuses_a_page_that_cannot_read_one_object() {
    let (store, _, _, tip, _) = repo();
    let starved = Budgeted {
        store: &store,
        reads: Cell::new(0),
    };
    assert_eq!(
        closure(&starved, Walk::of(&[tip], &[]), usize::MAX),
        Err(Error::CapReached)
    );
}

#[test]
fn fetch_pack_is_valid_for_git() {
    let git = git_binary();
    let (store, _, _, tip, _) = repo();
    let clone = run_fetch(
        &store,
        &Fetch {
            wants: vec![tip],
            done: true,
            ..Fetch::default()
        },
    );
    let dir = temp_dir("forge-git-fetch");
    assert!(
        std::process::Command::new(&git)
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .unwrap()
            .success()
    );
    let mut child = std::process::Command::new(&git)
        .args(["index-pack", "--stdin", "--strict"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), &clone.pack).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let fsck = std::process::Command::new(&git)
        .args(["fsck", "--connectivity-only", &tip.to_hex()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(fsck.status.success(), "{fsck:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_shallow_fetch_names_its_boundary_before_the_pack() {
    let (store, _, base, tip, _) = repo();
    let answer = |command: &Fetch, info: &ShallowInfo| {
        let object = store.get(&tip).unwrap().unwrap();
        let entries = std::iter::once(Ok(pack::entry(object.kind, &object.body)));
        let mut bytes = Vec::new();
        let mut sink = |chunk: &[u8]| bytes.extend_from_slice(chunk);
        respond_shallow(command, info, entries, &mut sink).unwrap();
        parsed(&bytes)
    };
    let boundary = ShallowInfo {
        shallow: vec![tip],
        unshallow: vec![base],
    };
    let deepened = Fetch {
        wants: vec![tip],
        done: true,
        deepen: Some(1),
        ..Fetch::default()
    };
    let response = answer(&deepened, &boundary);
    assert_eq!(
        response.sections,
        [
            "shallow-info".to_string(),
            format!("shallow {tip}"),
            format!("unshallow {base}"),
            "<delim>".into(),
            "packfile".into(),
            "<flush>".into(),
        ]
    );
    assert_eq!(read_pack(&response.pack), [tip].into_iter().collect());

    let from_a_shallow_client = Fetch {
        wants: vec![tip],
        done: true,
        shallows: vec![base],
        ..Fetch::default()
    };
    assert_eq!(
        answer(&from_a_shallow_client, &ShallowInfo::default()).sections,
        ["shallow-info", "<delim>", "packfile", "<flush>"]
    );

    let whole = Fetch {
        wants: vec![tip],
        done: true,
        ..Fetch::default()
    };
    assert_eq!(answer(&whole, &boundary).sections, ["packfile", "<flush>"]);

    let negotiating = Fetch {
        wants: vec![tip],
        wait_for_done: true,
        deepen: Some(1),
        ..Fetch::default()
    };
    let response = answer(&negotiating, &boundary);
    assert_eq!(response.sections, ["acknowledgments", "NAK", "<flush>"]);
    assert!(response.pack.is_empty());
}
