//! What a reader changes on the Programs page: by its account, on a program
//! that fills no role, the op goes to the programs role; by its voices, a
//! motion goes to valset; with neither, nothing is pressable. The ops are read as the host receives them:
//! `requests::<Submit<P>>` answers only an `op.submit` addressed to `P`,
//! decoded from the bytes the view sent.
use super::*;
use crate::acting::{self, Edit, Form, Path};
use programs::{Change, Pending};
use valset::{Motion, Voted};
use view::methods::Submit;

pub(super) fn waiting() -> Vec<Pending> {
    vec![
        Pending {
            height: 120,
            change: Change::Remove("forge".into()),
            by: None,
        },
        Pending {
            height: 700,
            change: Change::Set(entry("chat", 0x77)),
            by: Some(3),
        },
    ]
}

const CODE: [u8; 32] = [0x7e; 32];

pub(super) fn reader(
    account: Option<u64>,
    signer: [u8; 32],
    votes: bool,
    scheduled: Vec<Pending>,
) -> TestAppContext {
    reader_on(account, signer, votes, scheduled, Vec::new(), 1000)
}

fn reader_on(
    account: Option<u64>,
    signer: [u8; 32],
    votes: bool,
    scheduled: Vec<Pending>,
    hooks: Vec<String>,
    block_ms: u64,
) -> TestAppContext {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host().handle::<ChainStatus>(move |()| {
        Ok(NodeStatus {
            block_time_ms: block_ms,
            ..status(12)
        })
    });
    programs_hooking(
        &cx,
        vec![entry("chat", 0xab), entry("identity", 0xcd)],
        vec![programs::View {
            name: "explorer".into(),
            view: BlobId::Sha256([0xef; 32]),
        }],
        scheduled,
        hooks,
    );
    cx.host().handle::<Query<Valset>>(move |query| {
        Ok(match query {
            valset::Query::Voices { .. } => valset::Reply::Voices(if votes {
                vec![VALIDATOR.to_vec()]
            } else {
                Vec::new()
            }),
            _ => valset::Reply::Validators(vec![VALIDATOR.to_vec()]),
        })
    });
    cx.host()
        .handle::<Submit<Programs>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host().handle::<Submit<Valset>>(|_| {
        Ok(view::testing::applied(abi::encode(&Voted::Counted {
            votes: 1,
            needed: 2,
        })))
    });
    let session = cx.host().stream::<HostSession>();
    cx.open::<Explorer>();
    session.send(Session {
        connected: true,
        chain_id: "test#1".into(),
        signer: abi::hex(&signer),
        account,
        endpoint: String::new(),
    });
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    cx
}

fn ada() -> TestAppContext {
    reader(Some(3), ADA, false, waiting())
}

fn upgrade(height: u64) -> programs::Op {
    programs::Op::Schedule(programs::Scheduled {
        height,
        change: Change::Set(programs::Entry {
            code: BlobId::Sha256(CODE),
            ..entry("chat", 0xab)
        }),
    })
}

#[test]
fn an_accounts_upgrade_goes_to_the_programs_as_it_was_typed() {
    let mut cx = ada();
    cx.simulate_click("explorer-upgrade-chat");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-programs-list").is_some(),
        "the press opened no transactions"
    );
    assert!(cx.has_text("Upgrade chat"), "{:?}", cx.texts());
    assert!(cx.has_text("Close"), "the form closes; Cancel is a row's");
    assert!(cx.has_text("Params kept: 3 bytes, as chat runs now."));
    let earliest = 12 + 1 + programs::NOTICE;
    assert!(cx.has_text(&format!(
        "Block {earliest} or later: the validators get 60 blocks to look, and can cancel it by vote until it lands."
    )));
    cx.simulate_input("explorer-form-code", &format!("sha256:{}", abi::hex(&CODE)));
    cx.simulate_click("explorer-form-submit");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<Programs>>();
    let height = 12 + programs::NOTICE + 30;
    assert_eq!(sent, [upgrade(height)]);
    assert_eq!(abi::encode(&sent[0]), abi::encode(&upgrade(height)));
    assert!(cx.host().requests::<Submit<Valset>>().is_empty());
    assert!(cx.find("explorer-form").is_none(), "the form closes");
    assert!(cx.has_text("Sent to the programs role: Schedule · chat."));
}

#[test]
fn every_account_changes_a_program_that_fills_no_role_and_a_voter_proposes_a_roles() {
    let cx = reader(Some(6), STRANGER, false, waiting());
    assert!(cx.find("explorer-upgrade-chat").is_some());
    assert!(cx.find("explorer-remove-chat").is_some());
    assert!(cx.find("explorer-upgrade-identity").is_none());
    let mut cx = reader(Some(3), ADA, true, waiting());
    assert!(cx.has_text("Upgrade") && cx.has_text("Remove"));
    assert!(cx.find("explorer-upgrade-identity").is_some());
    assert!(cx.find("explorer-remove-identity").is_none());
    cx.simulate_click("explorer-upgrade-identity");
    cx.run_until_parked();
    assert!(
        cx.has_text("Propose upgrade · identity"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_reader_with_voices_and_no_account_proposes_the_same_change_to_the_vote() {
    let mut cx = reader(None, VALIDATOR, true, waiting());
    assert!(cx.has_text("Propose upgrade") && cx.has_text("Propose remove"));
    assert!(cx.find("explorer-upgrade-identity").is_some());
    assert!(cx.find("explorer-remove-identity").is_none());
    cx.simulate_click("explorer-upgrade-chat");
    cx.run_until_parked();
    assert!(cx.has_text("Propose upgrade · chat"));
    cx.simulate_input("explorer-form-code", &abi::hex(&CODE));
    cx.simulate_click("explorer-form-submit");
    cx.run_until_parked();
    let op = abi::encode(&upgrade(12 + 600));
    assert_eq!(
        cx.host().requests::<Submit<Valset>>(),
        [valset::Op::Vote(Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op,
        })]
    );
    assert!(cx.host().requests::<Submit<Programs>>().is_empty());
    assert!(cx.has_text("Vote counted, 1 of 2: Schedule · chat."));
}

#[test]
fn a_reader_with_neither_an_account_nor_voices_sees_who_and_presses_nothing() {
    let cx = reader(None, STRANGER, false, waiting());
    for press in [
        "explorer-upgrade-chat",
        "explorer-remove-chat",
        "explorer-upgrade-identity",
        "explorer-upgrade-explorer",
        "explorer-add",
        "explorer-cancel-0",
        "explorer-cancel-1",
    ] {
        assert!(cx.find(press).is_none(), "{press}");
    }
    assert!(cx.has_text("validators' vote"), "identity is a role's");
    assert!(cx.has_text("any account"), "chat fills no role");
    assert!(cx.has_text("by vote") && cx.has_text("by Ada"));
}

#[test]
fn access_says_any_account_changes_a_program_that_fills_no_role() {
    let cx = ada();
    assert!(cx.has_text("Access"));
    assert!(cx.has_text("changes by an account wait 60 blocks (about 1 min)"));
    assert!(cx.has_text(
        "Any account changes a program that fills no role. Only the validators' vote changes a role's program."
    ));
}

#[test]
fn the_waits_read_in_minutes_at_the_nodes_block_time() {
    let cx = reader_on(Some(3), ADA, false, waiting(), Vec::new(), 2000);
    assert!(
        cx.has_text("changes by an account wait 60 blocks (about 2 min)"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("validators can cancel for 23 min"));
}

#[test]
fn a_hook_is_upgraded_by_any_account_and_removed_only_by_vote() {
    let hooked = || vec!["chat".to_owned()];
    let cx = reader_on(Some(3), ADA, false, waiting(), hooked(), 1000);
    assert!(cx.find("explorer-hook-chat").is_some());
    assert!(cx.find("explorer-upgrade-chat").is_some());
    assert!(cx.find("explorer-remove-chat").is_none());
    let mut cx = reader_on(Some(3), ADA, true, waiting(), hooked(), 1000);
    assert!(cx.has_text("Upgrade") && cx.has_text("Propose remove"));
    cx.simulate_click("explorer-remove-chat");
    cx.run_until_parked();
    assert!(cx.has_text("Propose remove · chat"), "{:?}", cx.texts());
    cx.simulate_click("explorer-form-submit");
    cx.run_until_parked();
    let op = abi::encode(&programs::Op::Schedule(programs::Scheduled {
        height: 12 + 600,
        change: Change::Remove("chat".into()),
    }));
    assert_eq!(
        cx.host().requests::<Submit<Valset>>(),
        [valset::Op::Vote(Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op,
        })]
    );
    assert!(cx.host().requests::<Submit<Programs>>().is_empty());
}

#[test]
fn a_refusal_shows_in_the_form_in_the_hosts_words() {
    let mut cx = ada();
    let refusal = "Unlock a key first.";
    cx.host()
        .refuse::<Submit<Programs>>(view::methods::refusal::SESSION_LOCKED, refusal);
    cx.simulate_click("explorer-remove-chat");
    cx.run_until_parked();
    cx.simulate_click("explorer-form-submit");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-form-problem").is_some(),
        "the form says why"
    );
    assert!(cx.find("explorer-note").is_none(), "and nothing else does");
    assert!(
        cx.has_text(&format!("Not sent: {refusal}")),
        "{:?}",
        cx.texts()
    );
    cx.host().never::<Submit<Programs>>();
    cx.simulate_click("explorer-form-submit");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-form-problem").is_none(),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_scheduled_change_says_who_made_it_and_any_account_cancels_it() {
    let mut scheduled = waiting();
    scheduled.push(Pending {
        height: 800,
        change: Change::Set(entry("chat", 0x88)),
        by: None,
    });
    let mut cx = reader(Some(6), STRANGER, false, scheduled);
    assert!(cx.has_text("by Ada") && cx.has_text("by vote"));
    assert!(cx.has_text("validators can cancel for 12 min"));
    assert!(
        cx.find("explorer-cancel-2").is_none(),
        "the vote's change of chat is not an account's to cancel"
    );
    cx.simulate_click("explorer-cancel-1");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<Programs>>(),
        [programs::Op::Cancel {
            height: 700,
            program: "chat".into(),
        }]
    );
    let mut cx = reader(None, VALIDATOR, true, waiting());
    cx.simulate_click("explorer-cancel-0");
    cx.run_until_parked();
    let op = abi::encode(&programs::Op::Cancel {
        height: 120,
        program: "forge".into(),
    });
    assert_eq!(
        cx.host().requests::<Submit<Valset>>(),
        [valset::Op::Vote(Motion::Role {
            role: abi::role::PROGRAMS.into(),
            op,
        })]
    );
}

#[test]
fn any_account_adds_a_program_and_a_reader_with_none_does_not() {
    let cx = reader(Some(6), STRANGER, false, Vec::new());
    assert!(cx.find("explorer-add").is_some());
    let cx = reader(None, VALIDATOR, true, Vec::new());
    assert!(cx.find("explorer-add").is_none());
}

#[test]
fn a_form_builds_its_op_or_says_why_not() {
    let network = Network {
        programs: vec![entry("chat", 0xab)],
        ..Network::default()
    };
    let form = |edit: Edit, code: &str, height: &str| Form {
        edit,
        path: Path::Account,
        code: TextField::new(code),
        height: TextField::new(height),
        name: TextField::new("trade"),
        params: TextField::new("0a0b"),
        view: false,
        problem: None,
    };
    let code = abi::hex(&CODE);
    let upgrade_chat = Edit::Upgrade {
        name: "chat".into(),
        view: false,
    };
    assert_eq!(
        acting::built(&form(upgrade_chat.clone(), &code, "1,000"), &network, 12),
        Ok(upgrade(1000))
    );
    for (edit, code, height, why) in [
        (
            upgrade_chat.clone(),
            "sha256:12",
            "1000",
            "A code id is sha256: and 64 hex digits.",
        ),
        (
            upgrade_chat.clone(),
            code.as_str(),
            "soon",
            "The height is a block number.",
        ),
        (
            upgrade_chat,
            code.as_str(),
            "12",
            "Block 12 has passed: pick a later one.",
        ),
        (
            Edit::Upgrade {
                name: "gone".into(),
                view: false,
            },
            code.as_str(),
            "1000",
            "gone no longer runs here.",
        ),
    ] {
        assert_eq!(
            acting::built(&form(edit, code, height), &network, 12)
                .map_err(|problem| problem.sentence),
            Err(why.to_owned())
        );
    }
    let added = acting::built(&form(Edit::Add, &code, "1000"), &network, 12).unwrap();
    assert_eq!(
        added,
        programs::Op::Schedule(programs::Scheduled {
            height: 1000,
            change: Change::Set(programs::Entry {
                program: "trade".into(),
                code: BlobId::Sha256(CODE),
                params: vec![10, 11],
            }),
        })
    );
    assert_eq!(acting::default_height(Path::Account, 12, 1000), 102);
    assert_eq!(acting::default_height(Path::Vote, 12, 1000), 612);
    assert_eq!(acting::default_height(Path::Vote, 12, 2000), 312);
}
