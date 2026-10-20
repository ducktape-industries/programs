//! The row words, one state each: the fold of valset's answers, the strip's
//! window, and every Height, Behind and Status a row can read.
use view::methods::{Block, BlockList, BlockPage, NetworkStatus, Peer};

use crate::queries::{Node, fold};
use crate::recent::{Recent, WINDOW};
use crate::row::{Row, Status, synced};

fn member(key: u8, role: valset::Role) -> valset::Membership {
    valset::Membership {
        key: vec![key],
        address: Some(format!("10.0.0.{key}:4000")),
        halo: format!("10.0.0.{key}:9100"),
        role,
    }
}

fn node(key: u8, validator: bool) -> Node {
    Node {
        key: vec![key],
        address: None,
        validator,
        seat: None,
        operator: None,
    }
}

#[test]
fn the_fold_seats_validators_first_and_keeps_each_key_once() {
    use valset::Role::{Resident, Validator};
    let nodes = fold(
        &[vec![3], vec![1], vec![9]],
        vec![
            member(1, Validator),
            member(2, Resident),
            member(3, Validator),
            member(3, Validator),
            member(4, Resident),
        ],
    );
    let keys: Vec<(u8, Option<u32>)> = nodes.iter().map(|n| (n.key[0], n.seat)).collect();
    assert_eq!(
        keys,
        [
            (3, Some(0)),
            (1, Some(1)),
            (9, Some(2)),
            (2, None),
            (4, None)
        ]
    );
    assert!(nodes[2].validator && nodes[2].address.is_none());
    assert!(!nodes[3].validator);
}

fn led(from: u64, to: u64, by: impl Fn(u64) -> u8) -> BlockList {
    let blocks = (from..=to)
        .rev()
        .map(|height| Block {
            height,
            proposer: Some(vec![by(height)]),
            ..Block::default()
        })
        .collect();
    BlockList {
        blocks,
        listed_from: Some(0),
    }
}

#[test]
fn the_strip_asks_the_highest_missing_run_then_keeps_its_span() {
    let mut recent = Recent::default();
    let first = recent.next(100).unwrap();
    assert_eq!(
        first,
        BlockPage {
            before: Some(101),
            limit: 64,
            most: 0,
        }
    );
    recent.land(100, led(41, 100, |_| 1));
    assert_eq!(
        recent.next(100),
        Some(BlockPage {
            before: Some(41),
            limit: 4,
            most: 0,
        })
    );
    recent.land(100, led(37, 40, |_| 2));
    assert_eq!(recent.next(100), None);
    assert_eq!(recent.led.len() as u64, WINDOW);
    let fresh = recent.next(102).unwrap();
    assert_eq!(
        fresh,
        BlockPage {
            before: Some(103),
            limit: 2,
            most: 0,
        }
    );
    recent.land(102, led(101, 102, |_| 1));
    assert_eq!(recent.led.keys().next(), Some(&39));
    assert_eq!(recent.led(&[2], 40), Some(true));
    assert_eq!(recent.led(&[1], 102), Some(true));
    assert_eq!(recent.led(&[1], 39), Some(false));
    assert_eq!(recent.led(&[1], 200), None);
}

#[test]
fn the_lowest_listed_height_ends_the_asking() {
    let mut recent = Recent::default();
    let first = recent.next(10).unwrap();
    assert_eq!(
        first,
        BlockPage {
            before: Some(11),
            limit: 11,
            most: 0,
        }
    );
    let listed = BlockList {
        listed_from: Some(4),
        ..led(4, 10, |_| 1)
    };
    recent.land(10, listed);
    assert_eq!(recent.bottom, 4);
    assert_eq!(recent.next(10), None);
}

#[test]
fn a_node_that_lists_nothing_is_asked_again_at_the_next_head() {
    let mut recent = Recent::default();
    recent.land(10, BlockList::default());
    assert_eq!(recent.next(10), None);
    assert_eq!(
        recent.next(11),
        Some(BlockPage {
            before: Some(12),
            limit: 1,
            most: 0,
        })
    );
}

#[test]
fn a_residents_row_is_blank() {
    let resident = node(2, false);
    let blank = Row {
        signed: None,
        behind: None,
        status: Status::Blank,
    };
    let network = network(vec![peer(2, Some(100))]);
    assert_eq!(synced(&resident, &network), blank);
    assert_eq!(blank.status.word(), "—");
}

fn network(members: Vec<Peer>) -> NetworkStatus {
    NetworkStatus {
        height: 4_295,
        members,
    }
}

fn peer(key: u8, signed: Option<u64>) -> Peer {
    Peer {
        key: vec![key],
        signed,
    }
}

#[test]
fn every_word_a_validators_vote_gives() {
    let cases = [
        (Some(4_295), Some(0), Status::InSync),
        (Some(4_293), Some(2), Status::InSync),
        (Some(4_292), Some(3), Status::Behind(3)),
        (Some(4_276), Some(19), Status::Behind(19)),
        (Some(4_275), Some(20), Status::Quiet { since: Some(4_275) }),
        (None, None, Status::Quiet { since: None }),
    ];
    for (signed, behind, status) in cases {
        let row = synced(&node(1, true), &network(vec![peer(1, signed)]));
        assert_eq!(
            row,
            Row {
                signed,
                behind,
                status
            }
        );
    }
    let row = synced(&node(9, true), &network(vec![]));
    assert_eq!(row.status.word(), "Quiet");
    assert_eq!(Status::Behind(1_200).word(), "1,200 behind");
}
