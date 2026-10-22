//! The fence around users of another network: they write here only with the FromPeer a linked
//! channel's peer sends, and a local write names one only in a linked channel.
use ::program::{MockChain, code};

use super::linked::{ADA, CHIEF, THERE, by_id, chain, code_of, peer_post, post, signed};
use crate::{
    Block, ForeignName, Mark, Op, PageRequest, PeerWrite, PostPolicy, Principal, Query, Reply,
    Span, copy_id, parse_message,
};

fn foreigners(chain: &MockChain) -> Vec<ForeignName> {
    let asked = Query::Foreigners {
        page: PageRequest::default(),
    };
    let Reply::Foreigners(page) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!("Foreigners answers a page of names");
    };
    page.items
}

fn naming_chief() -> Vec<Block> {
    let span = Span {
        text: "@18@there".into(),
        marks: vec![Mark::Mention(Principal::Foreign(CHIEF))],
    };
    vec![Block::Paragraph(vec![span])]
}

fn typed(channel: &str, id: &str) -> Op {
    Op::Post {
        channel_id: channel.into(),
        message_id: id.into(),
        text: "ask @18@there".into(),
        thread: None,
    }
}

fn seat_chief(channel: &str, member: bool) -> Op {
    Op::SetMembership {
        channel_id: channel.into(),
        principal: Principal::Foreign(CHIEF),
        member,
    }
}

#[test]
fn a_user_of_another_network_writes_only_with_from_peer_in_a_channel_linked_to_their_network() {
    let chain = chain();
    let direct = [
        post("general", "c1", "straight into the linked room", None),
        post("quiet", "c2", "straight into an unlinked room", None),
        Op::Post {
            channel_id: "quiet".into(),
            message_id: "c3".into(),
            text: "as text".into(),
            thread: None,
        },
        Op::CreateChannel {
            channel_id: "theirs".into(),
            name: "theirs".into(),
            post_policy: PostPolicy::Open,
        },
        seat_chief("general", true),
        Op::Listen {
            channel_id: "quiet".into(),
            listening: true,
        },
        Op::AddReaction {
            channel_id: "general".into(),
            seq: 1,
            emoji: "👍".into(),
        },
    ];
    for op in direct {
        let refused = chain.submit_foreign(CHIEF, crate::PROGRAM, &op);
        assert_eq!(code_of(refused), code::UNAUTHORIZED, "{op:?}");
    }
    let unlinked = Op::FromPeer {
        channel_id: "quiet".into(),
        write: PeerWrite::Post {
            message_id: "q1".into(),
            blocks: parse_message("into a room linked to no one"),
            thread: None,
        },
    };
    let unlinked = chain.submit_foreign(CHIEF, crate::PROGRAM, &unlinked);
    assert_eq!(code_of(unlinked), code::UNAUTHORIZED);
    let theirs = Query::Channel {
        channel_id: "theirs".into(),
    };
    let held: Reply = chain.query(crate::PROGRAM, &theirs).unwrap();
    assert_eq!(held, Reply::Channel(None));
    assert!(
        foreigners(&chain).is_empty(),
        "a refused write records no one"
    );
    chain
        .submit_foreign(
            CHIEF,
            crate::PROGRAM,
            &peer_post("b1", "over the link", None),
        )
        .unwrap();
    let landed = by_id(&chain, &copy_id(&THERE, "b1"));
    assert_eq!(
        (landed.channel_id.as_str(), landed.author),
        ("general", Principal::Foreign(CHIEF))
    );
}

#[test]
fn a_local_write_names_a_user_of_another_network_only_in_a_linked_channel() {
    let chain = chain();
    let refusals = [
        typed("quiet", "q1"),
        Op::PostMessage {
            channel_id: "quiet".into(),
            message_id: "q2".into(),
            blocks: naming_chief(),
            thread: None,
        },
        seat_chief("quiet", true),
    ];
    for op in refusals {
        let refused = chain.submit(signed(ADA), crate::PROGRAM, &op);
        assert_eq!(code_of(refused), code::UNAUTHORIZED, "{op:?}");
    }
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("quiet", "q3", "no one from there", None),
        )
        .unwrap();
    let edits = [parse_message("now @18@there"), naming_chief()];
    for blocks in edits {
        let edit = Op::EditMessage {
            channel_id: "quiet".into(),
            seq: 1,
            blocks,
            base_rev: None,
        };
        let refused = chain.submit(signed(ADA), crate::PROGRAM, &edit);
        assert_eq!(code_of(refused), code::UNAUTHORIZED);
    }
    assert_eq!(by_id(&chain, "q3").text, "no one from there");
    chain
        .submit(signed(ADA), crate::PROGRAM, &seat_chief("quiet", false))
        .unwrap();
    assert!(
        foreigners(&chain).is_empty(),
        "a refused write records no one"
    );
    chain
        .submit(signed(ADA), crate::PROGRAM, &typed("general", "g1"))
        .unwrap();
    let Some(Block::Paragraph(spans)) = by_id(&chain, "g1").blocks.first().cloned() else {
        panic!("one paragraph");
    };
    let named: Vec<Mark> = spans.into_iter().flat_map(|span| span.marks).collect();
    assert_eq!(named, [Mark::Mention(Principal::Foreign(CHIEF))]);
    chain
        .submit(signed(ADA), crate::PROGRAM, &seat_chief("general", true))
        .unwrap();
}
