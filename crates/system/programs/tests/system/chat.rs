//! chat on ducktape's host, counted from the block's own writes: a message
//! is its point keys, an edit rewrites its row alone, and the row keeps its
//! body once. Search and tags read the messages; nothing indexes them.
use std::collections::BTreeSet;

use super::*;

use chat::tables::{HEADS, MESSAGE_IDS, MESSAGES, POSTED, ROOTS};

const ADA: u64 = 11;

// The keys of chat's state that one block wrote, `op` signed by Ada.
async fn chat_writes(net: &mut Net, op: &chat::Op) -> (BTreeSet<Vec<u8>>, Applied) {
    let submission = net.submission(&public(ADA), chat::PROGRAM, abi::encode(op));
    let applied = net.block(vec![submission]).await;
    let outcome = &applied.submissions[0].outcome;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let keys = applied.writes.programs[chat::PROGRAM]
        .keys()
        .cloned()
        .collect();
    (keys, applied)
}

#[test]
fn a_message_writes_its_point_keys_and_an_edit_its_row_alone() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let apps = vec![founding(chat::PROGRAM, &program("chat"))];
        let mut net = Net::found_with(context, dir.path(), apps, Vec::new(), MEMBER_CAP).await;
        let create = identity::Op::Create {
            name: "Ada".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&public(ADA), identity::PROGRAM, &create).await;
        let general = chat::Op::CreateChannel {
            channel_id: "general".into(),
            name: "general".into(),
            post_policy: chat::PostPolicy::Open,
        };
        net.apply(&public(ADA), chat::PROGRAM, &general).await;

        let body = "검색인덱스는 지웠다: one two three #ship #launch";
        let post = chat::Op::PostMessage {
            channel_id: "general".into(),
            message_id: "m1".into(),
            blocks: chat::parse_message(body),
            thread: None,
        };
        let (keys, applied) = chat_writes(&mut net, &post).await;
        let channel = || "general".to_owned();
        let row = MESSAGES.key(&(channel(), 1u64));
        let asked = abi::role::identity::Query::Account(public(ADA));
        let abi::role::identity::Reply::Account(Some(ada)) =
            net.ask(identity::PROGRAM, &asked).await
        else {
            panic!("Ada's key holds an account");
        };
        let author = chat::Principal::Account(ada);
        let points = BTreeSet::from([
            row.clone(),
            MESSAGE_IDS.key(&"m1".to_owned()),
            HEADS.key(&channel()),
            ROOTS.key(&(channel(), u64::MAX - 1)),
            // her first message in the room
            POSTED.key(&(channel(), author)),
        ]);
        assert_eq!(keys, points, "a root post writes its point keys alone");
        let stored = applied.writes.programs[chat::PROGRAM][&row]
            .clone()
            .unwrap();
        let held = stored
            .windows(body.len())
            .filter(|at| *at == body.as_bytes())
            .count();
        assert_eq!(held, 1, "the row keeps its body once");

        let edit = chat::Op::EditMessage {
            channel_id: "general".into(),
            seq: 1,
            blocks: chat::parse_message("다른 말 #other words"),
            base_rev: None,
        };
        let (keys, _) = chat_writes(&mut net, &edit).await;
        assert_eq!(
            keys,
            BTreeSet::from([row]),
            "an edit rewrites the row alone"
        );
    });
}
