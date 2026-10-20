//! Random walks of ops by three accounts: whatever order they come in, a
//! refused op writes nothing, the reactions agree with the messages, and
//! search and tags find what the messages hold.
use super::channels::edit;
use super::messages::tagged;
use super::*;
use crate::MsgRow;
use crate::state::{HEADS, MESSAGES, PARTICIPANTS, REACTIONS};

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1)
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len() as u64) as usize]
    }
}

const TEXTS: [&str; 4] = ["alpha beta", "beta #ship", "gamma #ship #it", "alpha"];
const EMOJI: [&str; 3] = ["👍", "🎉", "👀"];

#[test]
fn any_walk_keeps_reactions_and_search_in_step_with_the_messages() {
    for seed in 1..=24 {
        let mut rng = Rng(seed);
        let mut chat = Chat::with_channel(PostPolicy::Open);
        for step in 0..80 {
            let head = HEADS.get(&chat.reads(), &"general".into()).unwrap();
            let seq = 1 + rng.below(head.unwrap_or(0));
            let text = *rng.pick(&TEXTS);
            let op = match rng.below(6) {
                0 => post("general", &format!("m{step}"), text, None),
                1 => post("general", &format!("m{step}"), text, Some(seq)),
                2 => edit(seq, text),
                3 => delete(seq),
                n => react(seq, rng.pick(&EMOJI), n == 4),
            };
            let who = rng.pick(&[ADA, BO, CY]).clone();
            let before = chat.store.borrow().state.clone();
            if chat.run(&who, op).is_err() {
                assert_eq!(
                    chat.store.borrow().state,
                    before,
                    "seed {seed}: a refused op wrote"
                );
            }
        }
        in_step(&chat, seed);
    }
}

fn every_row(chat: &Chat) -> Vec<MsgRow> {
    let rows = MESSAGES.all(&chat.reads()).unwrap();
    rows.into_iter().map(|(_, row)| row.row()).collect()
}

fn in_step(chat: &Chat, seed: u64) {
    let rows: Vec<MsgRow> = every_row(chat);
    let reactions = REACTIONS.all(&chat.reads()).unwrap();
    for row in &rows {
        let chosen = |emoji: &str| {
            reactions
                .iter()
                .filter(|(ch, seq, e, _)| *ch == row.channel_id && *seq == row.seq && e == emoji)
                .count() as u64
        };
        let counted: u64 = row.reactions.iter().map(|r| r.count).sum();
        let marked = reactions
            .iter()
            .filter(|(_, seq, _, _)| *seq == row.seq)
            .count() as u64;
        assert_eq!(
            counted, marked,
            "seed {seed}: #{} counts its markers",
            row.seq
        );
        for reaction in &row.reactions {
            assert_eq!(reaction.count, chosen(&reaction.emoji), "seed {seed}");
        }
        if row.deleted {
            assert_eq!(marked, 0, "seed {seed}: a tombstone keeps no reactions");
        }
    }
    let row = |seq: u64| rows.iter().find(|row| row.seq == seq).unwrap();
    let participants = PARTICIPANTS.all(&chat.reads()).unwrap();
    for ((_, root, author), first) in &participants {
        let first_reply = row(*first);
        let opens_their_part = first_reply.thread == Some(*root) && first_reply.author == *author;
        assert!(opens_their_part, "seed {seed}: a participant with no reply");
    }
    for reply in rows.iter().filter(|row| row.thread.is_some()) {
        let key = (
            reply.channel_id.clone(),
            reply.thread.unwrap(),
            reply.author.clone(),
        );
        let first = participants
            .iter()
            .find(|(at, _)| *at == key)
            .map(|(_, first)| *first);
        assert!(
            first.is_some_and(|first| first <= reply.seq),
            "seed {seed}: a reply whose author takes no part"
        );
    }
    let newest_first = |held: &dyn Fn(&MsgRow) -> bool| -> Vec<u64> {
        rows.iter()
            .rev()
            .filter(|row| held(row))
            .map(|row| row.seq)
            .collect()
    };
    for word in ["alpha", "beta", "gamma", "ship"] {
        let holding = newest_first(&|row| row.text.contains(word));
        assert_eq!(chat.search(word), holding, "seed {seed}: search {word}");
    }
    let shipped = newest_first(&|row| row.tags.iter().any(|tag| tag == "ship"));
    assert_eq!(tagged(chat, "ship"), shipped, "seed {seed}: #ship");
}

#[test]
fn todays_ops_write_no_hold_and_tell_only_attribution() {
    for seed in 1..=24 {
        let mut rng = Rng(seed);
        let mut chat = Chat::with_channel(PostPolicy::Open);
        chat.post(&ADA, "root", "root", None);
        for step in 0..80 {
            let thread = *rng.pick(&[None, Some(1)]);
            let op = match rng.below(4) {
                0 => post("general", &format!("m{step}"), "hi <@2>", thread),
                1 => Op::Drafting {
                    channel_id: "general".into(),
                    thread,
                    text: "x".into(),
                    anew: rng.below(2) == 0,
                    doing: None,
                },
                2 => Op::Undraft {
                    channel_id: "general".into(),
                },
                _ => react(1, "👍", rng.below(2) == 0),
            };
            let who = rng.pick(&[ADA, BO, CY]).clone();
            let _ = chat.run(&who, op);
            for told in chat.store.take_emissions() {
                assert_eq!(
                    told.target,
                    attribution::PROGRAM,
                    "seed {seed}: step {step} told another program"
                );
            }
        }
        let host = chat.store.borrow();
        let held = (host.state.keys())
            .find(|key| key.starts_with(b"lease/") || key.starts_with(b"place-watcher/"));
        assert_eq!(held, None, "seed {seed}: today's ops wrote a hold");
    }
}
