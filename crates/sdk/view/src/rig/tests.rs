use super::*;
use crate::methods::{ChainBlocks, ChainStatus, Query as Ask};

/// The boot set is seated, the dev key holds an account identity knows,
/// and the programs role lists every seat.
#[test]
fn a_network_is_founded_with_the_boot_set_and_a_signed_in_person() {
    let net = Network::new();
    assert_eq!(
        net.seats(),
        ["identity", "valset", "programs", "blobs", "access"].map(String::from)
    );
    let me = net.me();
    let asked: identity::Reply = net
        .query(
            identity::PROGRAM,
            &identity::Query::OfKey {
                key: net.key_of(me),
            },
        )
        .unwrap();
    assert_eq!(asked, identity::Reply::Number(Some(me)));
    net.found_roster();
    let programs::Reply::Roster {
        programs: entries, ..
    } = net
        .query(programs::PROGRAM, &programs::Query::Roster)
        .unwrap()
    else {
        panic!("programs")
    };
    let mut listed: Vec<_> = entries.into_iter().map(|entry| entry.program).collect();
    listed.sort();
    assert_eq!(
        listed,
        ["access", "blobs", "identity", "programs", "valset"]
    );
    assert_eq!(net.session().account, Some(me));
}

/// An agent holds a key of its own, put there by its manager with the
/// key's own consent, which identity checks; the agent signs as itself.
#[test]
fn an_agent_holds_a_key_its_consent_put_there() {
    let net = Network::new();
    let me = net.me();
    let vps = net.agent(me, "validator vps").unwrap();
    assert_eq!(
        net.agent(me, "validator vps").unwrap(),
        vps,
        "a name is one agent"
    );
    let asked: identity::Reply = net
        .query(
            identity::PROGRAM,
            &identity::Query::OfKey {
                key: net.key_of(vps),
            },
        )
        .unwrap();
    assert_eq!(asked, identity::Reply::Number(Some(vps)));
    let rename = identity::Op::SetName {
        account: vps,
        name: "renamed".into(),
    };
    // the agent signs as itself and renames its own account, as its
    // manager may; an account that is neither is refused
    net.submit(vps, identity::PROGRAM, &rename).unwrap();
    let ada = net.person("ada");
    let refused = net.submit(ada, identity::PROGRAM, &rename).unwrap_err();
    assert_eq!(refused.code, "unauthorized");
}

/// A submission is one block: the archive grows, and the keys the program
/// wrote are the block's changes.
#[test]
fn a_submission_is_a_block_with_its_changes() {
    let net = Network::new();
    let height = net.height();
    assert!(net.take_changes().is_empty(), "the founding is no change");
    let ada = net.person("ada");
    assert_eq!(net.height(), height + 1);
    let changes = net.take_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0, "identity");
    assert_eq!(changes[0].1.height, height + 1);
    assert!(!changes[0].1.writes.is_empty());
    assert_eq!(net.person("ada"), ada, "a name is one account");
    let status = net.status();
    assert_eq!(status.height, height + 1);
    let listed = |most| -> methods::BlockList {
        let page = methods::BlockPage {
            before: None,
            limit: 10,
            most,
        };
        let answer = net.answer("chain.blocks", None, &methods::encode(&page));
        methods::decode(&answer.unwrap()).unwrap()
    };
    let blocks = listed(0).blocks;
    assert_eq!(blocks[0].height, height + 1);
    assert_eq!(blocks[0].txs.len(), 1);
    assert_eq!(blocks[0].txs[0].target, "identity");
    assert_eq!(blocks[0].txs[0].payload, None);
    assert_eq!(blocks.last().unwrap().height, 1);
    assert_eq!(listed(0).listed_from, Some(1));
    let size = blocks[0].txs[0].size;
    assert!(size > 0);
    let carried = &listed(size).blocks[0].txs[0];
    assert_eq!(carried.payload.as_ref().map(Vec::len), Some(size as usize));
    assert_eq!(listed(size - 1).blocks[0].txs[0].payload, None);
    let read = |most| {
        let ask = methods::TxAsk {
            tx: methods::TxRef::At {
                height: height + 1,
                position: 0,
            },
            most,
        };
        let answer = net
            .answer("chain.tx", None, &methods::encode(&ask))
            .unwrap();
        methods::decode::<Option<methods::Located>>(&answer)
            .unwrap()
            .unwrap()
            .tx
    };
    assert_eq!(read(size - 1).payload, None);
    assert_eq!(
        read(size).payload.map(|payload| payload.len()),
        Some(size as usize)
    );
}

/// A hook keeping the last frame the chain told it of.
struct Tap;

impl Program for Tap {
    const NAME: &'static str = "tap";
    type Op = ();
    type Query = ();
    type Reply = Option<hook::Record>;

    fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &program::QueryCtx, (): ()) -> Result<Option<hook::Record>, Error> {
        ctx.record("told")
    }

    fn hook(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
        ctx.put("told", &record);
        Ok(())
    }
}

/// A hook is told of each frame, with the account its key acts as, and
/// its writes are the block's changes; the roster lists it.
#[test]
fn a_hook_is_told_of_each_frame_as_the_kernel_tells_it() {
    let net = Network::new();
    net.hook::<Tap>("tap");
    let told = || -> hook::Record { net.query::<(), Option<_>>("tap", &()).unwrap().unwrap() };
    let ada = net.person("ada");
    let created = told();
    assert_eq!(created.height, net.height());
    assert_eq!(created.position, 0);
    assert_eq!(created.target, "identity");
    assert!(created.applied);
    assert_eq!(created.account, None, "her key held no account before");
    assert!(
        net.take_changes()
            .iter()
            .any(|(program, _)| program == "tap")
    );
    net.submit(ada, "tap", &()).unwrap();
    let posted = told();
    assert_eq!(posted.target, "tap");
    assert_eq!(posted.account, Some(ada));
    assert_eq!(posted.signer, net.key_of(ada));
    net.system("tap", &()).unwrap();
    assert_eq!(told(), posted, "a system run is no signed frame");
    net.found_roster();
    let hooks: programs::Reply = net
        .query(programs::PROGRAM, &programs::Query::Hooks(net.height()))
        .unwrap();
    assert_eq!(hooks, programs::Reply::Hooks(vec!["tap".into()]));
}

/// A hook that writes and then refuses.
struct Balk;

impl Program for Balk {
    const NAME: &'static str = "balk";
    type Op = ();
    type Query = ();
    type Reply = Option<hook::Record>;

    fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &program::QueryCtx, (): ()) -> Result<Option<hook::Record>, Error> {
        ctx.record("told")
    }

    fn hook(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
        ctx.put("told", &record);
        Err(Error::new("refused", "not this frame"))
    }
}

/// A hook that refuses is undone, as the kernel undoes it, and the frame
/// it was told of stands.
#[test]
fn a_hook_that_refuses_is_undone_and_the_frame_stands() {
    let net = Network::new();
    net.hook::<Balk>("balk");
    let ada = net.person("ada");
    let at = net.height();
    net.submit(ada, "balk", &()).expect("the frame stands");
    assert_eq!(net.height(), at + 1);
    let told: Option<hook::Record> = net.query("balk", &()).unwrap();
    assert_eq!(told, None, "its writes are undone");
}

/// A typed override answers before the chain; a refused one refuses.
#[test]
fn an_override_answers_in_place_of_the_chain() {
    let net = Network::new();
    net.handle::<ChainStatus>(|()| {
        Ok(methods::NodeStatus {
            height: 77,
            ..Default::default()
        })
    });
    let status: methods::NodeStatus =
        methods::decode(&net.answer("chain.status", None, &[]).unwrap()).unwrap();
    assert_eq!(status.height, 77);
    net.refuse::<ChainBlocks>("down", "no archive");
    let refused = net
        .answer(
            "chain.blocks",
            None,
            &methods::encode(&methods::BlockPage::default()),
        )
        .unwrap_err();
    assert_eq!(refused.code, "down");
}

/// Behind a FakeHost, a view's query reaches the program and a submit's
/// writes reach its `program.changes` subscription after the frame.
#[test]
fn the_chain_backs_a_fake_host() {
    use crate::methods::{Changes, Submit};
    use futures::StreamExt as _;
    let net = Network::new();
    let host = FakeHost::default();
    net.back(&host);
    let channel = crate::host::Host::default();
    let mut changes = channel.subscribe::<Changes<identity::Identity>>(());
    let asked = channel.query(identity::ask::OfKey {
        key: net.key_of(net.me()),
    });
    let created =
        channel.ask::<Submit<identity::Identity>>(identity::Op::CreateAgent { name: "bot".into() });
    host.accept(
        &crate::wire::Frame {
            requests: channel.drain_outbox(),
            ..Default::default()
        },
        &channel,
        None,
    );
    for event in host.take_events() {
        let crate::wire::Event::Response { id, result, done } = event else {
            panic!("an answer")
        };
        channel.fulfill(id, result, done);
    }
    assert_eq!(futures::executor::block_on(asked).unwrap(), Some(net.me()));
    let methods::Submitted::Applied { output, until, .. } =
        futures::executor::block_on(created).unwrap()
    else {
        panic!("the live node applies a CreateAgent")
    };
    assert_eq!(until, net.height() - 1 + 256, "anchor plus the window");
    let agent: u64 = methods::decode(&output).unwrap();
    assert!(agent > net.me());
    let change = futures::executor::block_on(changes.next())
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(change.height, net.height());
    assert!(!change.keys.is_empty());
}

/// A query the test answers itself wins over the chain for that program.
#[test]
fn a_typed_handler_on_the_host_wins_over_the_chain() {
    let net = Network::new();
    let host = FakeHost::default();
    net.back(&host);
    host.handle::<Ask<identity::Identity>>(|_| Ok(identity::Reply::Number(Some(999))));
    let channel = crate::host::Host::default();
    let asked = channel.query(identity::ask::OfKey { key: vec![1] });
    host.accept(
        &crate::wire::Frame {
            requests: channel.drain_outbox(),
            ..Default::default()
        },
        &channel,
        None,
    );
    for event in host.take_events() {
        let crate::wire::Event::Response { id, result, done } = event else {
            panic!("an answer")
        };
        channel.fulfill(id, result, done);
    }
    assert_eq!(futures::executor::block_on(asked).unwrap(), Some(999));
}

#[test]
fn a_held_blob_is_put_receipted_by_the_dev_validator_and_served() {
    let net = Network::new();
    let id = net.hold("pack", b"PACK").unwrap();
    let held: blobs::Reply = net
        .query(blobs::PROGRAM, &blobs::Query::Held { blob: id })
        .unwrap();
    let blobs::Reply::Held(Some(holding)) = held else {
        panic!("{held:?}");
    };
    assert_eq!(holding.kind, "pack");
    assert_eq!(net.blob(&id), Some(wire::framed("pack", b"PACK")));
}

/// `blob.manifest` answers off the roster's code as the app does: a
/// program's view out of its `ducktape.view` section with the manifest it
/// carries, a view-only blob as the view itself, a program with no view
/// as none, and a blob the chain does not name as `None`.
#[test]
fn blob_manifest_answers_what_the_rosters_code_declares() {
    let net = Network::new();
    let text =
        "ducktape.view.manifest\nChat\nRooms.\nchain,clipboard,\n480\n0123abcd\n\n\nblue\n1200";
    let view = pack::wrapper(&[("ducktape.view.manifest", text.as_bytes())]);
    net.code("chat", &pack::wrapper(&[("ducktape.view", &view)]));
    net.code("quiet", &pack::wrapper(&[("ducktape.describe", b"d")]));
    net.code_bare("explorer", &view);
    let ask = |program: &str| -> Option<methods::Manifested> {
        let id = match program {
            "explorer" => net.0.borrow().bare.as_ref().unwrap().1,
            _ => net.0.borrow().codes[program].0,
        };
        let name = format!("sha256:{}", abi::hex(id.digest()));
        let answer = net
            .answer("blob.manifest", None, &methods::encode(&name))
            .unwrap();
        methods::decode(&answer).unwrap()
    };
    let manifest = view_wire::manifest::Manifest::parse(text).unwrap();
    let carried = |size: usize| {
        Some(methods::CarriedView {
            size: size as u64,
            manifest: Some(manifest.clone()),
        })
    };
    let chat = ask("chat").unwrap();
    assert!(chat.size > view.len() as u64);
    assert_eq!(chat.view, carried(view.len()));
    let explorer = ask("explorer").unwrap();
    assert_eq!(explorer.size, view.len() as u64);
    assert_eq!(explorer.view, carried(view.len()));
    assert_eq!(ask("quiet").unwrap().view, None);
    let unnamed = format!("sha256:{}", "00".repeat(32));
    let answer = net
        .answer("blob.manifest", None, &methods::encode(&unnamed))
        .unwrap();
    assert_eq!(
        methods::decode::<Option<methods::Manifested>>(&answer).unwrap(),
        None
    );
}

#[test]
fn the_dev_validator_opens_the_door_alone_and_a_person_enrolls_as_a_resident() {
    let net = Network::new();
    assert_eq!(net.roles(), MockHost::roles());
    let ada = net.person("ada");
    let enroll = valset::Op::Enroll {
        address: Some("10.0.0.2:4000".into()),
        halo: "10.0.0.2:9100".into(),
        invite: None,
    };
    let closed = net.submit(ada, valset::PROGRAM, &enroll).unwrap_err();
    assert_eq!(closed.code, program::code::UNAUTHORIZED);
    let open = valset::Op::Vote(valset::Motion::Door { open: true });
    let voted: valset::Voted =
        methods::decode(&net.submit(net.me(), valset::PROGRAM, &open).unwrap()).unwrap();
    assert_eq!(voted, valset::Voted::Enacted);
    net.submit(ada, valset::PROGRAM, &enroll).unwrap();
    let valset::Reply::Membership(Some(held)) = net
        .query(
            valset::PROGRAM,
            &valset::Query::Membership {
                key: net.key_of(ada),
            },
        )
        .unwrap()
    else {
        panic!("ada is a member")
    };
    assert_eq!(held.role, valset::Role::Resident);
}

/// The rig speaks the contract core's node speaks: ducktape
/// `crates/noded/src/wire.rs` `NODE_CONTRACT`, 9 since ducktape #2883,
/// whose `/v1/status` carries the roles as a table (`Status::roles`). The
/// app refuses a node on another contract, so a rig a number behind opens
/// no live window at all. Core's number moves, this one moves with it.
#[test]
fn the_rig_speaks_the_node_contract_core_speaks() {
    assert_eq!(wire::NODE_CONTRACT, 9);
}

#[test]
fn a_put_reads_a_held_file_and_its_blob_is_held_at_once() {
    use crate::methods::{BlobBody, NewBlob, Pick, Picked, PutBlobs, Putting};
    let net = Network::new();
    net.device(&[("a.txt", b"first"), ("b.txt", b"second")]);
    let pick = |multiple| -> Vec<u64> {
        let picked = net
            .answer("file.pick", None, &methods::encode(&Pick { multiple }))
            .unwrap();
        let picked: Vec<Picked> = methods::decode(&picked).unwrap();
        picked.into_iter().map(|picked| picked.file).collect()
    };
    let put = |file, len| {
        let body = BlobBody::Picked {
            file,
            offset: 0,
            len,
        };
        let kind = "chunk".to_owned();
        let ask = PutBlobs {
            blobs: vec![NewBlob { kind, body }],
        };
        net.answer("op.put", None, &methods::encode(&ask))
    };
    assert_eq!(pick(false), [1]);
    assert_eq!(pick(true), [2, 3]);
    net.device(&[]);
    assert!(pick(true).is_empty(), "cancelled");
    assert!(put(1, 5).is_ok(), "a pick before the latest is still held");
    let release = methods::encode(&Vec::from([1u64]));
    net.answer("file.release", None, &release).unwrap();
    let refused = put(1, 5).unwrap_err();
    assert_eq!(
        refused.code,
        methods::refusal::MALFORMED_REQUEST,
        "a released file's"
    );
    let Putting::Held(ids) = methods::decode(&put(3, 6).unwrap()).unwrap() else {
        panic!("a put is held at once")
    };
    let hex: String = ids[0].iter().map(|byte| format!("{byte:02x}")).collect();
    let id = methods::encode(&format!("sha256:{hex}"));
    let got: Option<Vec<u8>> =
        methods::decode(&net.answer("blob.get", None, &id).unwrap()).unwrap();
    assert_eq!(got.as_deref(), Some(&b"second"[..]));
}
