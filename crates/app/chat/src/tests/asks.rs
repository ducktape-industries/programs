use super::*;
use crate::MsgRow;
use crate::asking::{Ask, Choice, answer, blocks, check, read};

fn cache() -> Ask {
    let choice = |label: &str, detail: &str| Choice {
        label: label.into(),
        detail: detail.into(),
    };
    Ask {
        to: vec![2, 3],
        question: "Which cache size should the node use?".into(),
        options: vec![
            choice("64 MB", "fits the small pods"),
            choice("128 MB", "today's measured peak"),
            choice("256 MB", ""),
        ],
        default: 1,
    }
}

#[test]
fn an_ask_reads_back_from_its_blocks() {
    let ask = cache();
    assert_eq!(check(&ask), Ok(()));
    assert_eq!(read(&blocks(&ask)), Some(ask));
}

#[test]
fn an_ask_typed_by_hand_reads_as_one() {
    let typed = "<@2> **Which cache?** #ask\n1. 64 MB \u{2014} small pods\n2. **128 MB**\n*Going with 128 MB.*";
    let ask = read(&parse_message(typed)).expect("an ask");
    assert_eq!(ask.to, [2]);
    assert_eq!(ask.question, "Which cache?");
    assert_eq!(ask.default, 1);
    let labels: Vec<(&str, &str)> = ask
        .options
        .iter()
        .map(|choice| (choice.label.as_str(), choice.detail.as_str()))
        .collect();
    assert_eq!(labels, [("64 MB", "small pods"), ("128 MB", "")]);
}

#[test]
fn a_message_that_is_no_ask_reads_as_none() {
    for text in [
        "<@2> **Which cache?**\n1. a\n2. **b**",
        "**Which cache?** #ask\n1. a\n2. **b**",
        "<@2> **Which cache?** #ask\n1. a\n2. b",
        "<@2> **Which cache?** #ask\n1. **a**\n2. **b**",
        "<@2> **Which cache?** #ask\n1. **a**",
        "<@2> **Which cache?** #ask\n1. a\n3. **b**",
        "<@2> **Which cache?** #ask\n1. a\n2. **a**",
        "<@2> **Which cache?** `#ask`\n1. a\n2. **b**",
        "<@2> **Which cache?**#ask\n1. a\n2. **b**",
        "<@2> **Which?** #ask\n1. a\n2. **b**\n3. c\n4. d\n5. e",
    ] {
        assert_eq!(read(&parse_message(text)), None, "{text}");
    }
}

#[test]
fn check_refuses_an_ask_it_could_not_read_back() {
    let with = |change: fn(&mut Ask)| {
        let mut ask = cache();
        change(&mut ask);
        check(&ask)
    };
    let refused = [
        with(|ask| ask.to.clear()),
        with(|ask| ask.question = " ".into()),
        with(|ask| ask.question = "one\ntwo".into()),
        with(|ask| ask.options.truncate(1)),
        with(|ask| {
            let more = ask.options[0].clone();
            ask.options.push(Choice {
                label: "512 MB".into(),
                ..more.clone()
            });
            ask.options.push(Choice {
                label: "1 GB".into(),
                ..more
            });
        }),
        with(|ask| ask.options[2].label = "64 mb".into()),
        with(|ask| ask.options[0].label = "a \u{2014} b".into()),
        with(|ask| ask.options[0].detail = "one\ntwo".into()),
        with(|ask| ask.default = 3),
    ];
    for (at, refusal) in refused.iter().enumerate() {
        assert!(refusal.is_err(), "case {at} passed");
    }
}

#[test]
fn a_reply_names_an_option_by_its_label_or_number() {
    let ask = cache();
    assert_eq!(answer(&ask, "128 MB"), Some(1));
    assert_eq!(answer(&ask, "  128 mb "), Some(1));
    assert_eq!(answer(&ask, "3"), Some(2));
    assert_eq!(answer(&ask, "4"), None);
    assert_eq!(answer(&ask, "0"), None);
    assert_eq!(answer(&ask, "neither, use 96 MB"), None);
}

#[test]
fn a_posted_ask_is_tagged_and_reads_back_from_its_row() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(
        &ADA,
        Op::PostMessage {
            channel_id: "general".into(),
            message_id: "ask-1".into(),
            blocks: blocks(&cache()),
            thread: None,
        },
    );
    let row = chat.message(1);
    assert_eq!(row.tags, ["ask"]);
    assert_eq!(read(&row.blocks), Some(cache()));
}

#[test]
fn repliers_answers_each_threads_first_reply_by_each_account() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "root", "<@2> **Which?** #ask\n1. a\n2. **b**", None);
    chat.post(&BO, "r1", "b", Some(1));
    chat.post(&CY, "r2", "neither", Some(1));
    chat.post(&BO, "r3", "a after all", Some(1));
    chat.post(&ADA, "other", "no thread here", None);
    let firsts = |reply: Reply| -> Vec<(u64, Vec<(u64, String)>)> {
        let Reply::Repliers(threads) = reply else {
            panic!("Repliers answers Repliers");
        };
        threads
            .into_iter()
            .map(|(root, rows)| {
                let rows = rows.into_iter().map(|row: MsgRow| (row.seq, row.text));
                (root, rows.collect())
            })
            .collect()
    };
    let asked = Query::Repliers {
        channel_id: "general".into(),
        roots: vec![5, 1, 99],
    };
    assert_eq!(
        firsts(chat.ask(asked)),
        [
            (5, vec![]),
            (1, vec![(2, "b".to_owned()), (3, "neither".to_owned())]),
            (99, vec![]),
        ]
    );
}

#[test]
fn a_notice_by_someone_asked_hides_none_of_their_answer() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "root", "<@3> **Which?** #ask\n1. a\n2. **b**", None);
    let notice = crate::Notice {
        kind: "pod".into(),
        group: "p".into(),
        subject: "pod p".into(),
        link: None,
        status: crate::message::Status::Note,
        detail: String::new(),
        concerns: vec![3],
    };
    chat.ok(
        &BO,
        Op::NotifyBy {
            channel_id: "general".into(),
            message_id: "built".into(),
            thread: Some(1),
            notice,
            by: CY,
        },
    );
    chat.post(&CY, "answer", "a", Some(1));
    let asked = Query::Repliers {
        channel_id: "general".into(),
        roots: vec![1],
    };
    let Reply::Repliers(threads) = chat.ask(asked) else {
        panic!("Repliers answers Repliers");
    };
    let answered: Vec<(u64, Vec<(u64, String)>)> = threads
        .into_iter()
        .map(|(root, rows)| {
            (
                root,
                rows.into_iter().map(|row| (row.seq, row.text)).collect(),
            )
        })
        .collect();
    assert_eq!(answered, [(1, vec![(3, "a".to_owned())])]);
}
