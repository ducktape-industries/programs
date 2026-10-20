//! Every op, once as meant and once as an attack: each refusal leaves the
//! store exactly as it was. The harness is the [`crate::Chat`] program over a
//! [`MockHost`], and the principals that act on it, each the sender of the
//! env it signs.
use ::program::{Env, Origin, code};
use ::program::{ExecCtx, MockHost, Program, QueryCtx};

use crate::{Op, PageRequest, PostPolicy, Principal, Query, ReactedBy, Reply, parse_message};

mod attachments;
mod attributed;
mod channels;
mod drafts;
mod gate;
mod legacy;
mod mentions;
mod messages;
mod order;
mod origin;
mod owned;
mod properties;
mod reactions;
mod rooms;

const ADA: Principal = Principal::Account(1);
const BO: Principal = Principal::Account(2);
const CY: Principal = Principal::Account(3);
const FORGE: Principal = Principal::Account(900);

fn program_of(who: &Principal) -> Option<&'static str> {
    match who {
        Principal::Account(900) => Some("forge"),
        Principal::Account(901) => Some("for"),
        Principal::Account(902) => Some("bot"),
        _ => None,
    }
}

struct Chat {
    store: MockHost,
    height: u64,
}

const AGENT: u64 = 10;
const SUSPENDED: u64 = 11;
const REVOKED: u64 = 12;

fn roster() -> Vec<crate::Profile> {
    use crate::{Category, Kind, Profile, Standing};
    let profile = |number: u64, kind| Profile {
        number,
        name: format!("user{number}"),
        kind,
    };
    let agent = |number, standing| {
        let kind = Kind::Program {
            script: None,
            manager: 1,
            category: Category::Agent,
            standing,
        };
        profile(number, kind)
    };
    let people = (1..=7).map(|number| profile(number, Kind::User));
    let programs = [900, 901, 902].map(|number| {
        let program = program_of(&Principal::Account(number)).unwrap();
        profile(number, Kind::System(program.into()))
    });
    people
        .chain(programs)
        .chain([
            agent(AGENT, Standing::Active),
            agent(SUSPENDED, Standing::Suspended),
            agent(REVOKED, Standing::Revoked),
        ])
        .collect()
}

impl Default for Chat {
    fn default() -> Chat {
        let store = MockHost::default();
        store.borrow_mut().verifier = Some(Box::new(|_, _, _, _, _| true));
        store.identity(roster());
        Chat { store, height: 0 }
    }
}

fn signer(who: &Principal) -> Origin {
    if let Some(program) = program_of(who) {
        return Origin::Program(program.into());
    }
    match who {
        Principal::Account(number) => Origin::Signed(number.to_be_bytes().to_vec()),
        Principal::Root => Origin::Root,
    }
}

impl Chat {
    fn with_channel(post_policy: PostPolicy) -> Chat {
        let mut chat = Chat::default();
        chat.ok(&ADA, create("general", post_policy));
        chat
    }

    fn env(&self, origin: Origin, sender: Option<Principal>) -> Env {
        Env {
            height: self.height,
            time: self.height * 1000,
            origin,
            sender,
            ..MockHost::env(crate::PROGRAM)
        }
    }

    fn next(&mut self, who: &Principal) -> ExecCtx {
        self.height += 1;
        self.store.exec(self.env(signer(who), Some(who.clone())))
    }

    fn reads(&self) -> QueryCtx {
        self.store.query(self.env(Origin::Root, None))
    }

    fn run(&mut self, who: &Principal, op: Op) -> Result<(), ::program::Error> {
        let ctx = self.next(who);
        crate::Chat::execute(&ctx, op)
    }

    #[track_caller]
    fn ok(&mut self, who: &Principal, op: Op) {
        if let Err(refusal) = self.run(who, op) {
            panic!("refused: {refusal}");
        }
    }

    #[track_caller]
    fn refused(&mut self, who: &Principal, op: Op) -> String {
        let ctx = self.next(who);
        self.store.refused(|| crate::Chat::execute(&ctx, op)).code
    }

    fn post(&mut self, who: &Principal, id: &str, text: &str, thread: Option<u64>) {
        self.ok(who, post("general", id, text, thread));
    }

    fn ask(&self, question: Query) -> Reply {
        crate::Chat::query(&self.reads(), question).unwrap()
    }

    fn message(&self, seq: u64) -> crate::MsgRow {
        crate::state::message(&self.reads(), "general", seq).unwrap()
    }

    fn channel(&self) -> crate::ChannelRow {
        crate::state::channel(&self.reads(), "general").unwrap()
    }

    fn search(&self, text: &str) -> Vec<u64> {
        let Reply::Hits(hits) = self.ask(Query::Search {
            text: text.into(),
            viewer: vec![],
            channel_id: None,
            page: PageRequest::default(),
        }) else {
            panic!("a search answers hits");
        };
        let seqs: Vec<u64> = hits.hits.iter().map(|row| row.seq).collect();
        let Reply::TextHits(page) = self.ask(Query::TextSearch {
            text: text.into(),
            viewer: vec![],
            channel_id: None,
            page: PageRequest::default(),
        }) else {
            panic!("a text search answers a page of hits");
        };
        let paged: Vec<u64> = page.items.iter().map(|row| row.seq).collect();
        assert_eq!(paged, seqs, "TextSearch finds what Search does");
        seqs
    }
}

fn create(id: &str, post_policy: PostPolicy) -> Op {
    Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy,
    }
}

fn post(channel: &str, id: &str, text: &str, thread: Option<u64>) -> Op {
    Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: parse_message(text),
        thread,
    }
}

fn react(seq: u64, emoji: &str, on: bool) -> Op {
    let (channel_id, emoji) = ("general".to_string(), emoji.to_string());
    if on {
        Op::AddReaction {
            channel_id,
            seq,
            emoji,
        }
    } else {
        Op::RemoveReaction {
            channel_id,
            seq,
            emoji,
        }
    }
}

fn delete(seq: u64) -> Op {
    Op::DeleteMessage {
        channel_id: "general".into(),
        seq,
    }
}
