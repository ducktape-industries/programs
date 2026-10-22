//! The vote on the sheet: who operates a node, the motions on each row
//! and the door for a reader whose key votes, a press that enacts asked
//! first, the motions that wait, and the scheduled changes and who made
//! each; the words for each motion.
use super::*;
use crate::voting::{said, words};
use abi::role::halo;
use programs::{Change, Entry, Programs, Scheduled};
use valset::{Motion, Op, Pending, Voted};
use view::methods::{HostSession, Session, Submit, Submitted};

const CODE: [u8; 32] = [0x7e; 32];
const NOW: [u8; 32] = [0x11; 32];

fn entry(program: &str, code: [u8; 32]) -> Entry {
    Entry {
        program: program.into(),
        code: abi::BlobId::Sha256(code),
        params: b"params".to_vec(),
    }
}

fn roster() -> Vec<Entry> {
    vec![entry("chat", NOW), entry("valset", NOW)]
}

fn set_valset(height: u64) -> Motion {
    let op = programs::Op::ScheduleOver {
        scheduled: Scheduled {
            height,
            change: Change::Set(entry("valset", CODE)),
        },
        expected: Some(abi::BlobId::Sha256(NOW)),
    };
    Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: abi::encode(&op),
    }
}

pub(super) fn sheet(
    signer: &[u8],
    needed: u64,
    pending: Vec<Pending>,
    scheduled: Vec<programs::Pending>,
) -> TestAppContext {
    sheet_view(signer, needed, pending, scheduled).0
}

pub(super) fn sheet_view(
    signer: &[u8],
    needed: u64,
    pending: Vec<Pending>,
    scheduled: Vec<programs::Pending>,
) -> (TestAppContext, Entity<Nodes>) {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    sheet_over(cx, signer, needed, pending, scheduled)
}

fn sheet_over(
    mut cx: TestAppContext,
    signer: &[u8],
    needed: u64,
    pending: Vec<Pending>,
    scheduled: Vec<programs::Pending>,
) -> (TestAppContext, Entity<Nodes>) {
    let members = vec![
        membership(&RESIDENT, "10.0.0.9:4000", valset::Role::Resident),
        membership(&OTHER, "10.0.0.2:4000", valset::Role::Validator),
        membership(&THIS, "10.0.0.1:4000", valset::Role::Validator),
    ];
    cx.host().handle::<Query<Valset>>(move |query| {
        Ok(match query {
            valset::Query::Validators => {
                valset::Reply::Validators(vec![THIS.to_vec(), OTHER.to_vec(), UNLISTED.to_vec()])
            }
            valset::Query::Memberships { .. } => valset::Reply::Memberships(page(members.clone())),
            valset::Query::Voices { key } => valset::Reply::Voices(match key == THIS {
                true => vec![THIS.to_vec()],
                false => Vec::new(),
            }),
            valset::Query::Motions { .. } => valset::Reply::Motions {
                needed,
                page: page(pending.clone()),
            },
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.host().handle::<Query<Programs>>(move |query| {
        Ok(match query {
            programs::Query::Program(_) => programs::Reply::Program {
                height: 4200,
                entry: None,
            },
            programs::Query::At(4200) => programs::Reply::Programs(roster()),
            programs::Query::Scheduled { .. } => {
                programs::Reply::Scheduled(page(scheduled.clone()))
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.host().handle::<Submit<Valset>>(move |_| {
        Ok(view::testing::applied(abi::encode(&Voted::Counted {
            votes: 1,
            needed: needed.max(2),
        })))
    });
    let session = cx.host().stream::<HostSession>();
    let nodes = cx.open::<Nodes>();
    session.send(Session {
        connected: true,
        chain_id: "Workshop".into(),
        signer: abi::hex(signer),
        account: Some(1),
        endpoint: String::new(),
    });
    cx.run_until_parked();
    (cx, nodes)
}

pub(super) fn submitted(cx: &TestAppContext) -> Vec<Op> {
    cx.host().requests::<Submit<Valset>>()
}

pub(super) fn operators() -> [identity::Profile; 2] {
    let agent = identity::Profile {
        number: 11,
        name: "validator vps".into(),
        kind: identity::Kind::Program {
            script: None,
            manager: 10,
            category: identity::Category::Agent,
            standing: identity::Standing::Active,
        },
    };
    let ada = identity::Profile {
        number: 10,
        name: "ada".into(),
        kind: identity::Kind::User,
    };
    [agent, ada]
}

// Every row's operator comes from one read of the accounts, never an ask per row; a key on a
// suspended agent operates nothing.
#[test]
fn a_row_says_who_operates_a_node_on_an_agent() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond(&mut cx);
    let parked = identity::Profile {
        number: 12,
        name: "parked".into(),
        kind: identity::Kind::Program {
            script: None,
            manager: 10,
            category: identity::Category::Agent,
            standing: identity::Standing::Suspended,
        },
    };
    let mut profiles = operators().to_vec();
    profiles.push(parked);
    let keys = vec![(OTHER.to_vec(), 11), (UNLISTED.to_vec(), 12)];
    serve_identity(&cx, keys, profiles);
    cx.open::<Nodes>();
    cx.run_until_parked();
    assert_eq!(
        texts_of(&cx, "nodes-operator-1"),
        ["operated by ada (validator vps)"]
    );
    for row in [0, 2, 3] {
        assert!(cx.find(&format!("nodes-operator-{row}")).is_none(), "{row}");
    }
    assert!(cx.has_text("10.0.0.1:4000"));
    let asked = cx.host().requests::<Query<identity::Identity>>();
    let listed = |query: &identity::Query| matches!(query, identity::Query::List { .. });
    assert!(!asked.is_empty() && asked.iter().all(listed), "{asked:?}");
}

#[test]
fn a_refused_operator_read_marks_every_row_and_keeps_the_list() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond(&mut cx);
    cx.host()
        .handle::<Query<identity::Identity>>(|query| match query {
            identity::Query::List { .. } => {
                Err(Error::new("unavailable", "identity is restarting"))
            }
            other => panic!("unexpected query: {other:?}"),
        });
    cx.open::<Nodes>();
    cx.run_until_parked();
    for row in 0..4 {
        assert_eq!(
            texts_of(&cx, &format!("nodes-operator-{row}")),
            ["operator unreadable: identity is restarting"],
            "{row}"
        );
    }
    assert!(cx.has_text("10.0.0.1:4000"));
}

#[test]
fn a_reader_who_does_not_vote_gets_no_controls() {
    let cx = sheet(&RESIDENT, 2, Vec::new(), Vec::new());
    for id in [
        "nodes-door",
        "nodes-motions",
        "nodes-scheduled",
        "nodes-promote-3",
    ] {
        assert!(cx.find(id).is_none(), "{id}");
    }
    assert!(cx.host().requests::<Query<Programs>>().is_empty());
}

#[test]
fn a_reader_who_votes_gets_each_rows_motions_and_the_door() {
    let mut cx = sheet(&THIS, 2, Vec::new(), Vec::new());
    for id in [
        "nodes-door-open",
        "nodes-door-close",
        "nodes-demote-0",
        "nodes-remove-1",
        "nodes-promote-3",
        "nodes-motions",
        "nodes-scheduled",
    ] {
        assert!(cx.find(id).is_some(), "{id}: {:?}", cx.texts());
    }
    assert!(cx.find("nodes-change").is_none() && cx.find("nodes-change-code").is_none());
    assert!(cx.has_text("Change a program in Explorer, Programs."));
    assert!(cx.find("nodes-promote-0").is_none() && cx.find("nodes-demote-3").is_none());
    assert!(cx.has_text("No motion waits for votes.") && cx.has_text("No change is scheduled."));
    cx.simulate_click("nodes-promote-3");
    cx.run_until_parked();
    assert_eq!(
        submitted(&cx),
        [Op::Vote(Motion::Promote {
            key: RESIDENT.to_vec()
        })]
    );
    assert!(cx.has_text("Vote counted: 1 of 2"));
    let voices = cx
        .host()
        .requests::<Query<Valset>>()
        .into_iter()
        .filter(|query| matches!(query, valset::Query::Voices { .. }))
        .count();
    assert_eq!(voices, 2);
}

#[test]
fn a_press_that_enacts_says_what_happens_and_asks_once() {
    let mut cx = sheet(&THIS, 1, Vec::new(), Vec::new());
    cx.simulate_click("nodes-door-open");
    cx.run_until_parked();
    assert!(cx.has_text("Open the door: anyone enrolls without an invite"));
    assert!(submitted(&cx).is_empty());
    cx.simulate_click("nodes-confirm-no");
    cx.run_until_parked();
    assert!(cx.find("nodes-confirm").is_none() && submitted(&cx).is_empty());
    cx.host()
        .handle::<Submit<Valset>>(|_| Ok(view::testing::applied(abi::encode(&Voted::Enacted))));
    cx.simulate_click("nodes-door-open");
    cx.run_until_parked();
    cx.simulate_click("nodes-confirm-yes");
    cx.run_until_parked();
    assert_eq!(submitted(&cx), [Op::Vote(Motion::Door { open: true })]);
    assert!(cx.has_text("Enacted") && cx.find("nodes-confirm").is_none());
}

#[test]
fn a_waiting_motion_reads_decoded_and_is_voted_with_its_own_bytes() {
    let waiting = Pending {
        motion: set_valset(5000),
        voters: vec![OTHER.to_vec()],
    };
    let mut cx = sheet(&THIS, 2, vec![waiting.clone()], Vec::new());
    let words = texts_of(&cx, "nodes-motion-0");
    assert_eq!(
        words,
        [
            "Set valset to code sha256:7e7e7e7e…7e7e at block 5,000 · params unchanged",
            "1 of 2 · yours has not voted",
            "Vote"
        ]
    );
    cx.simulate_click("nodes-motion-vote-0");
    cx.run_until_parked();
    cx.simulate_click("nodes-confirm-yes");
    cx.run_until_parked();
    assert_eq!(submitted(&cx), [Op::Vote(waiting.motion.clone())]);

    let voted = Pending {
        voters: vec![OTHER.to_vec(), THIS.to_vec()],
        ..waiting
    };
    let cx = sheet(&THIS, 3, vec![voted], Vec::new());
    assert_eq!(
        texts_of(&cx, "nodes-motion-0")[1..],
        ["2 of 3 · yours voted"]
    );
}

#[test]
fn a_scheduled_change_says_who_made_it_and_is_cancelled_by_a_vote() {
    let ahead = programs::Pending {
        height: 4300,
        change: Change::Set(entry("valset", CODE)),
        by: None,
    };
    let landed = programs::Pending {
        height: 4100,
        change: Change::Remove("chat".into()),
        by: None,
    };
    let by_account = programs::Pending {
        height: 4800,
        change: Change::Set(entry("chat", CODE)),
        by: Some(12),
    };
    let mut cx = sheet(&THIS, 2, Vec::new(), vec![landed, ahead, by_account]);
    assert_eq!(
        texts_of(&cx, "nodes-scheduled-0"),
        [
            "Set valset to code sha256:7e7e7e7e…7e7e at block 4,300 · params unchanged",
            "by vote · in 100 blocks",
            "Propose cancel"
        ]
    );
    assert_eq!(
        texts_of(&cx, "nodes-scheduled-1")[1],
        "by account 12 · validators can cancel for 10 min"
    );
    assert!(cx.find("nodes-scheduled-2").is_none());
    cx.simulate_click("nodes-scheduled-cancel-0");
    cx.run_until_parked();
    let cancel = programs::Op::Cancel {
        height: 4300,
        program: "valset".into(),
    };
    assert_eq!(
        submitted(&cx),
        [Op::Vote(Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op: abi::encode(&cancel)
        })]
    );
}

#[test]
fn a_scheduled_change_tells_no_wait_before_the_status_is_read() {
    let changes = vec![
        programs::Pending {
            height: 4300,
            change: Change::Set(entry("valset", CODE)),
            by: None,
        },
        programs::Pending {
            height: 4800,
            change: Change::Set(entry("chat", CODE)),
            by: Some(12),
        },
    ];
    let mut cx = TestAppContext::new();
    node(&mut cx);
    cx.host().never::<ChainStatus>();
    let (cx, _) = sheet_over(cx, &THIS, 2, Vec::new(), changes);
    assert_eq!(texts_of(&cx, "nodes-scheduled-0")[1], "by vote");
    assert_eq!(texts_of(&cx, "nodes-scheduled-1")[1], "by account 12");
}

#[test]
fn every_motion_reads_in_words() {
    let roster = roster();
    let key = vec![0xab; 32];
    let cases = [
        (
            Motion::Promote { key: key.clone() },
            "Promote abababab…abab to validator",
        ),
        (
            Motion::Demote { key: key.clone() },
            "Demote abababab…abab to resident",
        ),
        (
            Motion::Remove { key },
            "Remove abababab…abab from the network",
        ),
        (
            Motion::Door { open: false },
            "Close the door: enrolling takes an invite",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(&programs::Op::Cancel {
                    height: 4300,
                    program: "chat".into(),
                }),
            },
            "Cancel chat's change at block 4,300",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(&programs::Op::ScheduleOver {
                    scheduled: Scheduled {
                        height: 9,
                        change: Change::Set(Entry {
                            params: Vec::new(),
                            ..entry("chat", CODE)
                        }),
                    },
                    expected: Some(abi::BlobId::Sha256(NOW)),
                }),
            },
            "Set chat to code sha256:7e7e7e7e…7e7e at block 9 · params changed",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(&programs::Op::ScheduleOver {
                    scheduled: Scheduled {
                        height: 9,
                        change: Change::Set(entry("forge", CODE)),
                    },
                    expected: None,
                }),
            },
            "Set forge to code sha256:7e7e7e7e…7e7e at block 9 · a new program",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(&programs::Op::ScheduleOver {
                    scheduled: Scheduled {
                        height: 9,
                        change: Change::Publish {
                            entry: entry("access", CODE),
                            roles: vec![abi::role::ACCESS.into()],
                        },
                    },
                    expected: None,
                }),
            },
            "Publish access at code sha256:7e7e7e7e…7e7e at block 9 · a new program · fills access",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: abi::encode(&programs::Op::ScheduleOver {
                    scheduled: Scheduled {
                        height: 9,
                        change: Change::Publish {
                            entry: entry("forge", CODE),
                            roles: Vec::new(),
                        },
                    },
                    expected: None,
                }),
            },
            "Publish forge at code sha256:7e7e7e7e…7e7e at block 9 · a new program · no role",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: [
                    &[0u8][..],
                    &abi::encode(&Scheduled {
                        height: 9,
                        change: Change::Set(entry("chat", CODE)),
                    }),
                ]
                .concat(),
            },
            "A plain Schedule the programs role no longer takes",
        ),
        (
            Motion::Role {
                role: abi::role::PROGRAMS.into(),
                op: b"not an op".to_vec(),
            },
            "A program change this screen can't read",
        ),
        (
            lanes(halo::Op::Open {
                name: "chat".into(),
                cap: halo::Class::Member,
            }),
            "Open lane chat at class Member",
        ),
        (
            lanes(halo::Op::Set {
                name: "chat".into(),
                cap: halo::Class::Client,
                enabled: true,
            }),
            "Serve lane chat at class Client",
        ),
        (
            lanes(halo::Op::Set {
                name: "chat".into(),
                cap: halo::Class::Client,
                enabled: false,
            }),
            "Pause lane chat",
        ),
        (
            lanes(halo::Op::Close {
                name: "chat".into(),
            }),
            "Close lane chat",
        ),
        (
            Motion::Role {
                role: abi::role::HALO.into(),
                op: b"not an op".to_vec(),
            },
            "A lane change this screen can't read",
        ),
        (
            Motion::Role {
                role: abi::role::EGRESS.into(),
                op: vec![1, 2],
            },
            "An op for the egress role",
        ),
    ];
    for (motion, said) in cases {
        assert_eq!(words(&motion, &roster), said);
    }
}

fn lanes(op: halo::Op) -> Motion {
    Motion::Role {
        role: abi::role::HALO.into(),
        op: abi::encode(&op),
    }
}

#[test]
fn no_press_votes_before_the_motions_are_read() {
    let mut cx = TestAppContext::new();
    node(&mut cx);
    respond_voter(&mut cx);
    cx.host().never::<Query<Programs>>();
    let session = cx.host().stream::<HostSession>();
    cx.open::<Nodes>();
    session.send(Session {
        signer: abi::hex(&THIS),
        ..Session::default()
    });
    cx.run_until_parked();
    assert!(cx.has_text("not yet"));
    let Some(view::wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("nodes-door-open")
    else {
        panic!("the door's button")
    };
    assert_eq!(interactivity.aria.disabled, Some(true));
    assert!(interactivity.on_click.is_none());
}

fn respond_voter(cx: &mut TestAppContext) {
    cx.host().handle::<Query<Valset>>(move |query| {
        Ok(match query {
            valset::Query::Validators => valset::Reply::Validators(vec![THIS.to_vec()]),
            valset::Query::Memberships { .. } => valset::Reply::Memberships(page(vec![])),
            valset::Query::Voices { .. } => valset::Reply::Voices(vec![THIS.to_vec()]),
            valset::Query::Motions { .. } => {
                return Err(Error::new("unavailable", "not yet"));
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

#[test]
fn a_vote_says_what_it_came_to() {
    let applied = |voted: Voted| Ok(view::testing::applied(abi::encode(&voted)));
    assert_eq!(said(applied(Voted::Enacted)), Ok("Enacted".into()));
    assert_eq!(
        said(Ok(view::testing::applied(Vec::new()))),
        Ok("Voted".into()),
        "landed, its output unreadable"
    );
    assert_eq!(
        said(applied(Voted::Counted {
            votes: 1,
            needed: 2
        })),
        Ok("Vote counted: 1 of 2".into())
    );
    let refusal = |code: &str, message: &str| Error::new(code, message);
    assert_eq!(
        said(Ok(Submitted::Refused {
            frame: [0; 32],
            until: 0,
            refusal: refusal("unauthorized", "only a validator votes"),
        })),
        Err("That didn’t go through: only a validator votes".into())
    );
    assert_eq!(
        said(Err(refusal(
            methods::refusal::RPC_CLIENT,
            "connection refused"
        ))),
        Err("Not sent: the node can’t be reached. Check the connection, then try again.".into())
    );
    assert_eq!(
        said(Err(refusal(
            methods::refusal::NOT_CONNECTED,
            "No node is connected."
        ))),
        Err("Not sent: not connected to a node.".into())
    );
    assert_eq!(
        said(Err(refusal(
            methods::refusal::CONSENT_REFUSED,
            "not confirmed"
        ))),
        Err("That didn’t go through: not confirmed".into())
    );
    assert_eq!(
        said(Err(refusal(
            "bad_signature",
            "The frame's signature does not verify."
        ))),
        Err("That didn’t go through: The frame's signature does not verify.".into())
    );
    assert_eq!(
        said(Err(refusal(methods::refusal::NODE_FAILED, "the node stopped"))),
        Err(
            "This may have gone through: no answer came back. What you see is read again from the node."
                .into()
        )
    );
    for code in [
        methods::refusal::SESSION_LOCKED,
        methods::refusal::STALE_CONNECTION,
        methods::refusal::HOST_FAULT,
    ] {
        assert_eq!(
            said(Err(refusal(code, "No key is unlocked."))),
            Err("Not sent: No key is unlocked.".into()),
            "{code}"
        );
    }
    assert_eq!(
        said(Ok(Submitted::InDoubt {
            frame: [0; 32],
            until: 0
        })),
        Err(
            "This may have gone through: no answer came back. What you see is read again from the node."
                .into()
        )
    );
}
