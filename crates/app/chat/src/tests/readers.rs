//! Who reads a direct room: its peers by their own keys, a program as its own account, the reader
//! a proxy in chat's params names through `As`, and the kernel; no one else. Other rooms read for all.
use ::program::{Env, Error, MockHost, Origin, Roster, code};

use super::*;

const LONE_KEY: [u8; 32] = [9; 32];
const DM: &str = "dm-1-2";
const TEAM: &str = "team";

fn key_of(number: u64) -> Vec<u8> {
    number.to_be_bytes().to_vec()
}

fn listing(proxies: &[&str]) -> Vec<u8> {
    let proxies: Vec<String> = proxies.iter().map(|id| id.to_string()).collect();
    abi::encode(&proxies)
}

fn rooms_with(params: Vec<u8>) -> Chat {
    let mut chat = Chat::default();
    let keys = (1..=7).map(|number| (key_of(number), number)).collect();
    let profiles = roster().into_iter().map(|p| (p.number, p)).collect();
    chat.store.identity(Roster { keys, profiles });
    let entry = programs::Entry {
        program: crate::PROGRAM.into(),
        code: ::program::BlobId::Sha256([0; 32]),
        params,
    };
    chat.store.borrow_mut().siblings.insert(
        MockHost::role(abi::role::PROGRAMS),
        Box::new(move |_: Env, _: &[u8]| {
            let entry = Some(entry.clone());
            Ok(abi::encode(&programs::Reply::Program { height: 0, entry }))
        }),
    );
    let dm = Op::CreateDmChannel {
        counterpart: 2,
        name: "ada and bo".into(),
    };
    chat.ok(&ADA, dm);
    chat.ok(&ADA, create("general", PostPolicy::Open));
    chat.ok(&ADA, create(TEAM, PostPolicy::MembersOnly));
    chat.ok(&ADA, post(DM, "dm-1", "the plan is secret", None));
    chat.ok(&ADA, post("general", "open-1", "the plan is public", None));
    chat.ok(&ADA, post(TEAM, "team-1", "the plan is for the team", None));
    chat
}

fn rooms(proxies: &[&str]) -> Chat {
    rooms_with(listing(proxies))
}

impl Chat {
    fn read_as(&self, origin: Origin, query: Query) -> Result<Reply, Error> {
        crate::Chat::query(&self.store.query(self.env(origin, None)), query)
    }
}

fn roots(id: &str) -> Query {
    Query::Roots {
        channel_id: id.into(),
        viewer: vec![],
        page: PageRequest::default(),
    }
}

fn as_reader(reader: Principal, query: Query) -> Query {
    Query::As {
        reader,
        query: abi::encode(&query),
    }
}

fn reads_of(id: &str) -> Vec<Query> {
    let room = || id.to_string();
    vec![
        roots(id),
        Query::MessagesAround {
            channel_id: room(),
            seq: 1,
            viewer: vec![],
            page: PageRequest::default(),
        },
        Query::Thread {
            channel_id: room(),
            root_seq: 1,
            viewer: vec![],
            page: PageRequest::default(),
        },
        Query::ThreadAttention {
            channel_id: room(),
            author: ADA,
        },
        Query::Members {
            channel_id: room(),
            page: PageRequest::default(),
        },
        Query::Reactors {
            channel_id: room(),
            seqs: vec![1],
        },
        Query::Clients {
            channel_id: room(),
            seqs: vec![1],
        },
        Query::Drafts { channel_id: room() },
        Query::Repliers {
            channel_id: room(),
            roots: vec![1],
        },
        Query::TextSearch {
            text: "plan".into(),
            viewer: vec![],
            channel_id: Some(room()),
            page: PageRequest::default(),
        },
        Query::TagSearch {
            tag: "plan".into(),
            viewer: vec![],
            channel_id: Some(room()),
            page: PageRequest::default(),
        },
    ]
}

#[track_caller]
fn refused(answer: Result<Reply, Error>) -> String {
    match answer {
        Ok(reply) => panic!("answered {reply:?}"),
        Err(refusal) => refusal.code,
    }
}

#[track_caller]
fn texts(answer: Result<Reply, Error>) -> Vec<String> {
    match answer.unwrap() {
        Reply::Roots(page) | Reply::TextHits(page) => {
            page.items.into_iter().map(|row| row.text).collect()
        }
        other => panic!("answered {other:?}"),
    }
}

#[test]
fn a_key_on_no_account_reads_nothing_of_a_direct_room() {
    let chat = rooms(&[]);
    let lone = || Origin::Signed(LONE_KEY.to_vec());
    for query in reads_of(DM) {
        let asked = format!("{query:?}");
        assert_eq!(
            refused(chat.read_as(lone(), query)),
            code::UNAUTHORIZED,
            "{asked}"
        );
    }
    let by_id = Query::MessageById {
        message_id: "dm-1".into(),
    };
    assert_eq!(refused(chat.read_as(lone(), by_id)), code::UNAUTHORIZED);
}

#[test]
fn a_rooms_members_read_it_by_their_own_keys() {
    let chat = rooms(&[]);
    for member in [1, 2] {
        let me = || Origin::Signed(key_of(member));
        assert_eq!(texts(chat.read_as(me(), roots(DM))), ["the plan is secret"]);
        for query in reads_of(DM) {
            let asked = format!("{query:?}");
            let answer = chat.read_as(me(), query);
            assert!(answer.is_ok(), "{asked}: {answer:?}");
        }
    }
}

#[test]
fn an_account_outside_the_room_is_refused_whatever_viewer_it_names() {
    let chat = rooms(&[]);
    let named = Query::Roots {
        channel_id: DM.into(),
        viewer: vec![ADA, BO],
        page: PageRequest::default(),
    };
    let outsider = Origin::Signed(key_of(3));
    assert_eq!(refused(chat.read_as(outsider, named)), code::UNAUTHORIZED);
}

#[test]
fn an_open_or_members_only_room_reads_for_any_key() {
    let chat = rooms(&[]);
    let lone = || Origin::Signed(LONE_KEY.to_vec());
    assert_eq!(
        texts(chat.read_as(lone(), roots("general"))),
        ["the plan is public"]
    );
    assert_eq!(
        texts(chat.read_as(lone(), roots(TEAM))),
        ["the plan is for the team"]
    );
}

#[test]
fn a_search_leaves_out_the_rooms_its_reader_cannot_read() {
    let chat = rooms(&[]);
    let search = || Query::TextSearch {
        text: "plan".into(),
        viewer: vec![],
        channel_id: None,
        page: PageRequest::default(),
    };
    let mut lone = texts(chat.read_as(Origin::Signed(LONE_KEY.to_vec()), search()));
    lone.sort();
    assert_eq!(lone, ["the plan is for the team", "the plan is public"]);
    let mut bo = texts(chat.read_as(Origin::Signed(key_of(2)), search()));
    bo.sort();
    let all = [
        "the plan is for the team",
        "the plan is public",
        "the plan is secret",
    ];
    assert_eq!(bo, all);
}

#[test]
fn a_program_reads_as_its_own_account() {
    let mut chat = rooms(&[]);
    let forge = || Origin::Program("forge".into());
    assert_eq!(
        refused(chat.read_as(forge(), roots(DM))),
        code::UNAUTHORIZED
    );
    let dm = Op::CreateDmChannel {
        counterpart: 900,
        name: "ada and forge".into(),
    };
    chat.ok(&ADA, dm);
    chat.ok(&ADA, post("dm-1-900", "forge-1", "for forge alone", None));
    let read = texts(chat.read_as(forge(), roots("dm-1-900")));
    assert_eq!(read, ["for forge alone"]);
}

#[test]
fn the_kernel_reads_every_room() {
    let chat = rooms(&[]);
    assert_eq!(
        texts(chat.read_as(Origin::Root, roots(DM))),
        ["the plan is secret"]
    );
}

#[test]
fn a_proxy_in_chats_params_reads_for_the_reader_it_names() {
    let chat = rooms(&["harness"]);
    let harness = || Origin::Program("harness".into());
    let for_bo = as_reader(BO, roots(DM));
    assert_eq!(
        texts(chat.read_as(harness(), for_bo)),
        ["the plan is secret"]
    );
    let for_cy = as_reader(CY, roots(DM));
    assert_eq!(refused(chat.read_as(harness(), for_cy)), code::UNAUTHORIZED);
}

#[test]
fn an_as_from_a_program_off_the_list_reads_as_that_program() {
    let chat = rooms(&["harness"]);
    let bot = Origin::Program("bot".into());
    let for_bo = as_reader(BO, roots(DM));
    assert_eq!(refused(chat.read_as(bot, for_bo)), code::UNAUTHORIZED);
    let unlisted = rooms(&[]);
    let harness = Origin::Program("harness".into());
    let for_bo = as_reader(BO, roots(DM));
    assert_eq!(
        refused(unlisted.read_as(harness, for_bo)),
        code::UNAUTHORIZED
    );
}

#[test]
fn an_as_from_a_key_reads_as_that_keys_account() {
    let chat = rooms(&["harness"]);
    let for_bo = as_reader(BO, roots(DM));
    let cy = Origin::Signed(key_of(3));
    assert_eq!(refused(chat.read_as(cy, for_bo)), code::UNAUTHORIZED);
    let for_cy = as_reader(CY, roots(DM));
    let bo = Origin::Signed(key_of(2));
    assert_eq!(texts(chat.read_as(bo, for_cy)), ["the plan is secret"]);
}

#[test]
fn an_as_inside_an_as_is_refused() {
    let chat = rooms(&["harness"]);
    let harness = Origin::Program("harness".into());
    let nested = as_reader(BO, as_reader(BO, roots(DM)));
    assert_eq!(refused(chat.read_as(harness, nested)), code::INVALID_INPUT);
}

#[test]
fn chats_params_name_its_proxies_as_a_borsh_list_of_program_ids() {
    assert_eq!(crate::reader::proxied(&[]).unwrap(), Vec::<String>::new());
    let listed = crate::reader::proxied(&listing(&["harness"])).unwrap();
    assert_eq!(listed, ["harness"]);
    assert!(crate::reader::proxied(&[1, 2, 3]).is_err());
    let unreadable = rooms_with(vec![1, 2, 3]);
    let harness = Origin::Program("harness".into());
    let for_bo = as_reader(BO, roots(DM));
    let refusal = refused(unreadable.read_as(harness, for_bo));
    assert_eq!(refusal, code::INVALID_INPUT);
}
