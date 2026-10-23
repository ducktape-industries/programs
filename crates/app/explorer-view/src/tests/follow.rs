use super::*;

#[test]
fn an_identity_head_re_reads_the_accounts() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let heads = cx.host().stream::<Changes<Identity>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(!cx.has_text("Program · chess"));
    let asked = cx.host().requests::<Query<Identity>>().len();
    let chess = account(
        8,
        "chess",
        identity::Control::System {
            program: "chess".into(),
        },
    );
    cx.host().handle::<Query<Identity>>(move |_| {
        Ok(identity::Reply::Accounts(identity::PageResponse {
            height: 13,
            items: vec![ada(), scout(), forge(), chess.clone()],
            next: None,
        }))
    });
    heads.send(Some(view::methods::Change {
        height: 13,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(cx.has_text("Program · chess"), "{:?}", cx.texts());
    assert!(
        cx.has_text("Agent · external · managed by Ada · suspended"),
        "kept"
    );
    assert_eq!(cx.host().requests::<Query<Identity>>().len(), asked + 1);
}

#[test]
fn a_programs_head_re_reads_the_programs() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let heads = cx.host().stream::<Changes<Programs>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("2 programs"));
    let programs = vec![
        entry("chat", 0xab),
        entry("identity", 0xcd),
        entry("chess", 0x11),
    ];
    programs_with(&cx, programs, Vec::new(), Vec::new());
    heads.send(Some(view::methods::Change {
        height: 13,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(
        cx.has_text("3 programs") && cx.has_text("chess"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Nothing is scheduled against the programs role."));
}

// A busy network writes the programs role's neighbours nearly every block: a programs head heard
// while the roster is read lets that read land and reads once more after it. A read replaced for
// each head never landed, the roster being five reads in a row.
#[test]
fn the_programs_land_though_a_head_comes_in_every_block() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    let heads = cx.host().stream::<Changes<Programs>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let head = |height| {
        Some(view::methods::Change {
            height,
            keys: Vec::new(),
        })
    };
    heads.send(head(13));
    cx.run_until_parked();
    let one_read = cx.reports().len() as u64;
    let programs = vec![
        entry("chat", 0xab),
        entry("identity", 0xcd),
        entry("chess", 0x11),
    ];
    programs_with(&cx, programs, Vec::new(), Vec::new());
    for height in 14..14 + 2 * one_read {
        heads.send(head(height));
        cx.tick(Vec::new());
    }
    assert!(cx.has_text("3 programs"), "{:?}", cx.texts());
}

fn programs_env(height: u64, origin: program::Origin) -> program::Env {
    program::Env {
        height,
        origin,
        sender: None,
        ..program::MockHost::env(programs::PROGRAM)
    }
}

fn programs_swapping(program: &str, at: u64) -> (program::MockHost, BlobId, BlobId) {
    use program::{MockHost, Origin, Program as _};
    let store = MockHost::default();
    let entry = |code| programs::Entry {
        program: program.into(),
        code,
        params: Vec::new(),
    };
    let running = BlobId::Sha256([0xab; 32]);
    let genesis = programs::Genesis {
        programs: vec![entry(running)],
        views: Vec::new(),
        hooks: Vec::new(),
        roles: abi::role::REQUIRED
            .iter()
            .map(|role| (*role, program))
            .collect(),
    };
    Programs::init(
        &store.exec(programs_env(0, Origin::Root)),
        &abi::encode(&genesis),
    )
    .unwrap();
    let scheduled =
        program::blob_id(program::HashKind::Sha256, programs::CODE_KIND, b"code").unwrap();
    let held: program::Sibling = Box::new(|_, _| {
        Ok(abi::encode(&blobs::Reply::Held(Some(blobs::Holding {
            bundle: 0,
            len: 9,
            kind: programs::CODE_KIND.into(),
            since: 1,
        }))))
    });
    store
        .borrow_mut()
        .siblings
        .insert(MockHost::role(abi::role::BLOBS), held);
    let swap = programs::Op::ScheduleOver {
        scheduled: programs::Scheduled {
            height: at,
            change: programs::Change::Set(entry(scheduled)),
        },
        expected: Some(running),
    };
    let validators = Origin::Program(MockHost::role(abi::role::VALIDATORS));
    Programs::execute(&store.exec(programs_env(1, validators)), swap).unwrap();
    (store, running, scheduled)
}

#[test]
fn a_scheduled_swap_shows_once_a_head_reaches_its_height() {
    use program::Program as _;
    let mut cx = TestAppContext::new();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    let heads = cx.host().stream::<ChainHeads>();
    let (store, running, scheduled) = programs_swapping("chat", 14);
    let asked_at = tip.clone();
    cx.host().handle::<Query<Programs>>(move |query| {
        let env = programs_env(*asked_at.borrow() + 1, program::Origin::Root);
        Ok(Programs::query(&store.query(env), query).unwrap())
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    let running = crate::decode::short(running.digest());
    let scheduled = crate::decode::short(scheduled.digest());
    assert!(
        cx.has_text(&running) && cx.has_text("at 14"),
        "{:?}",
        cx.texts()
    );
    let mut reach = |height: u64| {
        *tip.borrow_mut() = height;
        heads.send(Head {
            height,
            time: T0 + height * 1000,
            id: [(height as u8).wrapping_add(100); 32],
        });
        cx.run_until_parked();
        cx.has_text(&running) && cx.has_text("at 14")
    };
    assert!(reach(13), "before its height");
    assert!(!reach(14), "at its height");
    assert!(
        cx.has_text(&scheduled) && !cx.has_text(&running),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Nothing is scheduled against the programs role."));
}

#[test]
fn a_change_reached_by_the_head_is_running_not_scheduled() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12u64));
    node(&mut cx, tip.clone());
    let chess = programs::Pending {
        height: 13,
        change: programs::Change::Set(entry("chess", 0x11)),
        by: None,
    };
    let at = tip.clone();
    cx.host().handle::<Query<Programs>>(move |query| {
        let height = *at.borrow();
        Ok(match query {
            programs::Query::Roster => {
                let mut programs = vec![entry("chat", 0xab), entry("identity", 0xcd)];
                if height >= chess.height {
                    programs.push(entry("chess", 0x11));
                }
                programs::Reply::Roster {
                    height,
                    programs,
                    views: Vec::new(),
                }
            }
            programs::Query::Scheduled { .. } => {
                programs::Reply::Scheduled(programs::PageResponse {
                    height,
                    items: vec![chess.clone()],
                    next: None,
                })
            }
            programs::Query::Hooks(_) => programs::Reply::Hooks(Vec::new()),
            programs::Query::Sources => programs::Reply::Sources {
                height: 1,
                programs: Vec::new(),
            },
            programs::Query::Roles => programs::Reply::Roles(abi::Roles::from_iter([
                (abi::role::PROGRAMS, programs::PROGRAM),
                (abi::role::VALIDATORS, valset::PROGRAM),
                (abi::role::IDENTITY, identity::PROGRAM),
                (abi::role::BLOBS, "blobs"),
                (abi::role::AWAITS, "awaits"),
                (abi::role::EGRESS, "egress"),
                (abi::role::HALO, "halo"),
            ])),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("2 programs") && cx.has_text("at 13"));
    *tip.borrow_mut() = 13;
    heads.send(Head {
        height: 13,
        time: T0 + 13_000,
        id: [113; 32],
    });
    cx.run_until_parked();
    assert!(
        cx.has_text("3 programs") && cx.has_text("chess"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Nothing is scheduled against the programs role."));
}

#[test]
fn an_empty_programs_says_so() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    programs_with(&cx, Vec::new(), Vec::new(), Vec::new());
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-programs");
    cx.run_until_parked();
    assert!(cx.has_text("No programs"), "{:?}", cx.texts());
}

#[test]
fn refused_accounts_say_why_and_retry_reads_again() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host()
        .refuse::<Query<Identity>>("unavailable", "identity is not running here");
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(cx.has_text("identity is not running here"));
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.simulate_click("explorer-retry");
    cx.run_until_parked();
    assert!(cx.has_text("Program · forge"), "{:?}", cx.texts());
}

#[test]
fn a_pushed_head_asks_for_the_blocks_since_the_top() {
    let mut cx = TestAppContext::new();
    let heads = cx.host().stream::<ChainHeads>();
    let tip = Rc::new(RefCell::new(12u64));
    node(&mut cx, tip.clone());
    let bytes = Rc::new(RefCell::new(0usize));
    {
        let (tip, bytes) = (tip.clone(), bytes.clone());
        cx.host().handle::<ChainFacts>(move |ask| {
            let page = view::rig::facts_page(&chain(*tip.borrow()), &ask);
            *bytes.borrow_mut() += borsh::to_vec(&page).unwrap().len();
            Ok(page)
        });
    }
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-transactions");
    cx.run_until_parked();
    for head in 13..=15u64 {
        let asked = cx.host().requests::<ChainFacts>().len();
        let bytes_before = *bytes.borrow();
        *tip.borrow_mut() = head;
        heads.send(Head {
            height: head,
            time: T0 + head * 1000,
            id: [(head as u8).wrapping_add(100); 32],
        });
        cx.run_until_parked();
        let new: Vec<_> = cx.host().requests::<ChainFacts>()[asked..]
            .iter()
            .map(|ask| (ask.before, ask.limit))
            .collect();
        eprintln!(
            "AUDIT head {head}: {} chain.facts asks {new:?}, {} block bytes carried for 1 new block",
            new.len(),
            *bytes.borrow() - bytes_before
        );
        assert_eq!(new, [(None, 1)]);
    }
    *tip.borrow_mut() = 18;
    let asked = cx.host().requests::<ChainFacts>().len();
    heads.send(Head {
        height: 18,
        time: T0 + 18_000,
        id: [118; 32],
    });
    cx.run_until_parked();
    let new: Vec<_> = cx.host().requests::<ChainFacts>()[asked..]
        .iter()
        .map(|ask| (ask.before, ask.limit))
        .collect();
    assert_eq!(new, [(None, 3)]);
    assert!(
        cx.has_text("3 transactions in the last 19 blocks"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn an_accounts_activity_past_the_window_lists_its_frames_and_reads_no_transaction() {
    let mut cx = TestAppContext::new();
    let tip = Rc::new(RefCell::new(WINDOW as u64 + 100));
    node(&mut cx, tip.clone());
    let heads = cx.host().stream::<Changes<Activity>>();
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(cx.has_text("3 · tx recorded"), "{:?}", cx.texts());
    assert!(cx.has_text("2 tx"), "Ada's two, long out of the window");
    cx.simulate_click("explorer-account-3");
    cx.run_until_parked();
    assert!(cx.has_text("2 transactions recorded"), "{:?}", cx.texts());
    let label = |id: &str| cx.interactivity(id).aria.label.as_deref().map(String::from);
    assert_eq!(
        label("explorer-tx-11-1").as_deref(),
        Some("chat, Accepted, block 11 · 1")
    );
    assert_eq!(
        label("explorer-tx-11-0").as_deref(),
        Some("chat, Accepted, block 11 · 0")
    );
    for title in ["Post in #design", "Direct message"] {
        assert!(!cx.has_text(title), "a row shows no op title: {title}");
    }
    assert!(
        cx.host().requests::<ChainTx>().is_empty(),
        "no transaction is read to list them"
    );
    assert!(cx.host().requests::<ChainBlock>().is_empty());
    let asked = cx.host().requests::<Query<Activity>>().len();
    heads.send(Some(view::methods::Change {
        height: *tip.borrow() + 1,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Query<Activity>>().len(),
        asked + 2,
        "the counts and the open account's"
    );
    assert!(cx.host().requests::<ChainTx>().is_empty());
    cx.simulate_click("explorer-tx-11-0");
    cx.run_until_parked();
    assert!(
        cx.has_text("In block 11") && cx.has_text("hello there"),
        "a row of the account's activity opens by its place: {:?}",
        cx.texts()
    );
    let opened = TxAsk {
        tx: TxRef::At {
            height: 11,
            position: 0,
        },
        most: u32::MAX,
    };
    assert_eq!(cx.host().requests::<ChainTx>(), [opened]);
}

#[test]
fn without_the_activity_program_the_accounts_list_and_the_account_says_why() {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host()
        .refuse::<Query<Activity>>("not_found", "no program is called activity");
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_click("explorer-tab-accounts");
    cx.run_until_parked();
    assert!(cx.has_text("Ada") && cx.has_text("–"), "{:?}", cx.texts());
    cx.simulate_click("explorer-account-3");
    cx.run_until_parked();
    assert!(
        cx.has_text("no program is called activity") && cx.has_text("laptop"),
        "{:?}",
        cx.texts()
    );
    assert!(!cx.has_text("Programs used"));
}
