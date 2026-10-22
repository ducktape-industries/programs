//! Sealed rooms (J-377 §4, J-389): each rule as meant and as an attack (F1–F8, R1–R3, 56's D1–D8)
//! over a test identity whose signing keys a test takes away, as identity's key removal does.
use ::program::{Error, Origin, Roster, Scheme, Sibling, Verifier, identity_role};

use super::*;
use crate::sealed::{DEVICE_WRAP_BYTES, KEYS_PER_ACCOUNT, WRAPPED};
use crate::{
    Block, DeviceKey, DeviceWrap, EpochWrap, Kind, MsgRow, Notice, PeerMessage, PeerWrite, SEALED,
    Standing,
};

const ROOM: &str = "dm-1-2";
const A1: [u8; 32] = [0xa1; 32];
const A2: [u8; 32] = [0xa2; 32];
const A3: [u8; 32] = [0xa3; 32];
const B1: [u8; 32] = [0xb1; 32];
const B2: [u8; 32] = [0xb2; 32];
const B3: [u8; 32] = [0xb3; 32];
const C1: [u8; 32] = [0xc1; 32];

fn laptop(account: u64) -> Vec<u8> {
    account.to_be_bytes().to_vec()
}

fn phone(account: u64) -> Vec<u8> {
    vec![account as u8, 2]
}

fn tablet(account: u64) -> Vec<u8> {
    vec![account as u8, 3]
}

fn scheme(key: &[u8]) -> Scheme {
    match key {
        [_, 3] => Scheme::Secp256r1,
        _ => Scheme::Ed25519,
    }
}

fn signature(scheme: Scheme, key: &[u8], namespace: &[u8], message: &[u8]) -> Vec<u8> {
    borsh::to_vec(&(scheme, key.to_vec(), namespace.to_vec(), message.to_vec())).unwrap()
}

fn statement(chain: &[u8], account: u64, device: [u8; 32]) -> Vec<u8> {
    let message = borsh::to_vec(&(chain.to_vec(), account, device)).unwrap();
    let framed = ("chat-view".to_owned(), "seal-device/v1".to_owned(), message);
    borsh::to_vec(&framed).unwrap()
}

fn signed_by(key: &[u8], statement: &[u8]) -> Vec<u8> {
    signature(scheme(key), key, b"ducktape/view-sign/v1", statement)
}

fn control(kind: &Kind, keys: Vec<::identity::Key>) -> ::identity::Control {
    use ::identity::{Control, Life};
    match kind {
        Kind::User => Control::User { keys },
        Kind::Program {
            manager,
            category,
            standing,
            script,
        } => Control::Program {
            manager: *manager,
            category: *category,
            life: match standing {
                Standing::Active => Life::Active { keys },
                Standing::Suspended => Life::Suspended { keys },
                Standing::Revoked => Life::Revoked,
            },
            script: script.clone(),
        },
        Kind::System(program) => Control::System {
            program: program.clone(),
        },
    }
}

fn identity_of(roster: &Roster, request: &[u8]) -> Result<Vec<u8>, Error> {
    let asked: Result<::identity::Query, _> = abi::decode(request);
    let Ok(::identity::Query::Get { number }) = asked else {
        return identity_role(roster, request);
    };
    let keys = (roster.keys.iter())
        .filter(|(_, held)| **held == number)
        .map(|(key, _)| ::identity::Key {
            scheme: scheme(key),
            key: key.clone(),
            label: None,
            added_at: 0,
        });
    let account = roster
        .profiles
        .get(&number)
        .map(|profile| ::identity::Account {
            number,
            card: ::identity::Card {
                name: profile.name.clone(),
                avatar: None,
                bio: None,
                updated_at: 0,
            },
            control: control(&profile.kind, keys.collect()),
        });
    Ok(abi::encode(&::identity::Reply::Account(account)))
}

fn keys(taken: &[Vec<u8>]) -> Roster {
    let held = (1..=7u64)
        .chain([AGENT])
        .flat_map(|n| [(laptop(n), n), (phone(n), n), (tablet(n), n)]);
    Roster {
        keys: held.filter(|(key, _)| !taken.contains(key)).collect(),
        profiles: roster().into_iter().map(|p| (p.number, p)).collect(),
    }
}

impl Chat {
    fn signed(&mut self, who: &Principal, key: Vec<u8>, op: Op) -> Result<(), Error> {
        self.height += 1;
        let ctx = (self.store).exec(self.env(Origin::Signed(key), Some(who.clone())));
        self.store.attempt(|| crate::Chat::execute(&ctx, op))
    }

    fn tried(&mut self, who: &Principal, op: Op) -> Result<(), Error> {
        let ctx = self.next(who);
        self.store.attempt(|| crate::Chat::execute(&ctx, op))
    }

    fn take_keys(&mut self, taken: &[Vec<u8>]) {
        let roster = keys(taken);
        let answer: Sibling = Box::new(move |_, request| identity_of(&roster, request));
        let identity = MockHost::role(abi::role::IDENTITY);
        self.store.borrow_mut().siblings.insert(identity, answer);
    }

    fn offer(&mut self, who: &Principal, key: Vec<u8>, device: [u8; 32]) -> Result<(), Error> {
        let Principal::Account(account) = who else {
            panic!("only an account publishes a device key");
        };
        let sig = signed_by(&key, &statement(b"net", *account, device));
        self.signed(who, key, Op::PublishDeviceKey { key: device, sig })
    }

    fn publish(&mut self, who: &Principal, key: Vec<u8>, device: [u8; 32]) {
        if let Err(refused) = self.offer(who, key, device) {
            panic!("publish refused: {refused}");
        }
    }
}

fn code_of(result: Result<(), Error>) -> String {
    match result {
        Ok(()) => panic!("taken, where a refusal was due"),
        Err(refused) => refused.code,
    }
}

fn dm() -> Chat {
    let mut chat = Chat::default();
    chat.take_keys(&[]);
    let verifier: Verifier = Box::new(|scheme, key, namespace, message, sig| {
        sig == signature(scheme, key, namespace, message)
    });
    chat.store.borrow_mut().verifier = Some(verifier);
    let open = Op::CreateDmChannel {
        counterpart: 2,
        name: "ada and bo".into(),
    };
    chat.ok(&ADA, open);
    chat
}

fn sealed_dm() -> Chat {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&BO, laptop(2), B1);
    chat.ok(&ADA, seal(&[A1, B1]));
    chat
}

fn wrapped(device: [u8; 32]) -> DeviceWrap {
    DeviceWrap {
        device,
        wrapped: vec![device[0]; WRAPPED],
    }
}

fn seal(devices: &[[u8; 32]]) -> Op {
    Op::Seal {
        channel_id: ROOM.into(),
        wraps: devices.iter().copied().map(wrapped).collect(),
    }
}

fn rekey(epoch: u64, devices: &[[u8; 32]]) -> Op {
    Op::Rekey {
        channel_id: ROOM.into(),
        epoch,
        wraps: devices.iter().copied().map(wrapped).collect(),
    }
}

fn give(epoch: u64, device: [u8; 32]) -> Op {
    Op::Wrap {
        channel_id: ROOM.into(),
        wraps: vec![EpochWrap {
            epoch,
            device,
            wrapped: vec![9; WRAPPED],
        }],
    }
}

fn sealed_post(id: &str, epoch: u64, thread: Option<u64>) -> Op {
    Op::PostSealed {
        channel_id: ROOM.into(),
        message_id: id.into(),
        thread,
        epoch,
        nonce: [5; 24],
        ct: b"opaque".to_vec(),
    }
}

fn device_keys(chat: &Chat, accounts: Vec<u64>) -> Vec<[u8; 32]> {
    let Reply::DeviceKeys(keys) = chat.ask(Query::DeviceKeys { accounts }) else {
        panic!("DeviceKeys answers DeviceKeys");
    };
    keys.into_iter().map(|key| key.key).collect()
}

fn epoch(chat: &Chat) -> Option<u64> {
    let asked = Query::Sealing {
        channel_ids: vec![ROOM.into(), "general".into()],
    };
    let Reply::Sealing(rooms) = chat.ask(asked) else {
        panic!("Sealing answers Sealing");
    };
    rooms
        .iter()
        .find(|(id, _)| id == ROOM)
        .map(|(_, epoch)| *epoch)
}

fn wraps(chat: &Chat, devices: &[[u8; 32]]) -> Vec<crate::WrapRow> {
    let asked = Query::Wraps {
        channel_id: ROOM.into(),
        devices: devices.to_vec(),
    };
    let Reply::Wraps(rows) = chat.ask(asked) else {
        panic!("Wraps answers Wraps");
    };
    rows
}

fn sealed(chat: &Chat, seqs: Vec<u64>) -> Vec<crate::SealedRow> {
    let asked = Query::Sealed {
        channel_id: ROOM.into(),
        seqs,
    };
    let Reply::Sealed(rows) = chat.ask(asked) else {
        panic!("Sealed answers Sealed");
    };
    rows
}

fn roots(chat: &Chat) -> Vec<MsgRow> {
    let asked = Query::Roots {
        channel_id: ROOM.into(),
        viewer: vec![],
        page: PageRequest::default(),
    };
    let Reply::Roots(page) = chat.ask(asked) else {
        panic!("Roots answers Roots");
    };
    page.items
}

fn device_notices(chat: &Chat) -> Vec<MsgRow> {
    let added = |row: &MsgRow| row.notice().is_some_and(|notice| notice.kind == "device");
    roots(chat).into_iter().filter(added).collect()
}

fn line() -> Vec<Block> {
    vec![Block::paragraph(SEALED)]
}

#[test]
fn a_published_key_is_live_while_identity_holds_its_signer() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&ADA, phone(1), A2);
    assert_eq!(device_keys(&chat, vec![1, 2]), vec![A1, A2]);
    chat.take_keys(&[phone(1)]);
    assert_eq!(
        device_keys(&chat, vec![1]),
        vec![A1],
        "removing a signing key in identity retires its device key"
    );
}

#[test]
fn a_key_whose_signer_does_not_act_as_the_sender_is_refused() {
    let mut chat = dm();
    let stray = chat.offer(&ADA, vec![9, 9], A1);
    assert_eq!(code_of(stray), code::UNAUTHORIZED, "a key on no account");
    let bos = chat.offer(&ADA, laptop(2), A1);
    assert_eq!(code_of(bos), code::UNAUTHORIZED, "a key of another account");
    assert!(device_keys(&chat, vec![1, 2]).is_empty());
}

#[test]
fn a_device_key_names_one_device_in_chat() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    let copied = chat.offer(&BO, laptop(2), A1);
    assert_eq!(code_of(copied), code::ALREADY_EXISTS, "another account");
    let moved = chat.offer(&ADA, phone(1), A1);
    assert_eq!(code_of(moved), code::ALREADY_EXISTS, "another signer");
    chat.publish(&ADA, laptop(1), A1);
    assert_eq!(device_keys(&chat, vec![1, 2]), vec![A1]);
    let theirs = chat.signed(&BO, laptop(2), Op::RetireDeviceKey { key: A1 });
    assert_eq!(code_of(theirs), code::UNAUTHORIZED);
    chat.signed(&ADA, laptop(1), Op::RetireDeviceKey { key: A1 })
        .unwrap();
    assert!(device_keys(&chat, vec![1]).is_empty());
    let again = chat.offer(&BO, laptop(2), A1);
    assert_eq!(
        code_of(again),
        code::ALREADY_EXISTS,
        "a retired key stays taken"
    );
}

#[test]
fn an_account_holds_as_many_keys_as_two_accounts_wraps_fit_in_one_frame() {
    let one = borsh::to_vec(&wrapped(A1)).unwrap();
    assert_eq!(one.len(), DEVICE_WRAP_BYTES);
    let both = 2 * DEVICE_WRAP_BYTES as u64;
    let frame = abi::BLOCK_BYTES as u64;
    assert!(both * (KEYS_PER_ACCOUNT + 1) <= frame);
    assert!(both * (KEYS_PER_ACCOUNT + 2) > frame);
    assert_eq!(KEYS_PER_ACCOUNT, 72_314);
    let mut chat = dm();
    let ctx = chat.next(&ADA);
    crate::state::DEVICE_COUNTS.put(&ctx, &1, &KEYS_PER_ACCOUNT);
    let full = chat.offer(&ADA, laptop(1), A1);
    assert_eq!(code_of(full), code::WRONG_STATE);
    let ctx = chat.next(&ADA);
    crate::state::DEVICE_COUNTS.put(&ctx, &1, &(KEYS_PER_ACCOUNT - 1));
    chat.publish(&ADA, laptop(1), A1);
    let refilled = chat.offer(&ADA, phone(1), A2);
    assert_eq!(code_of(refilled), code::WRONG_STATE);
    chat.signed(&ADA, laptop(1), Op::RetireDeviceKey { key: A1 })
        .unwrap();
    chat.publish(&ADA, phone(1), A2);
}

#[test]
fn a_dm_seals_only_when_the_wraps_cover_exactly_both_peoples_live_devices() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    let alone = chat.tried(&ADA, seal(&[A1]));
    assert_eq!(code_of(alone), code::WRONG_STATE, "Bo has no device yet");
    chat.publish(&BO, laptop(2), B1);
    chat.publish(&BO, phone(2), B2);
    chat.publish(&CY, laptop(3), C1);
    let refusals = [
        (vec![A1, B1], code::WRONG_STATE, "Bo's phone left out"),
        (
            vec![A1, B1, B2, C1],
            code::WRONG_STATE,
            "a device of neither",
        ),
        (
            vec![A1, B1, B2, A3],
            code::WRONG_STATE,
            "a key nobody published",
        ),
        (vec![A1, A1, B1, B2], code::INVALID_INPUT, "a device twice"),
    ];
    for (devices, want, why) in refusals {
        assert_eq!(code_of(chat.tried(&ADA, seal(&devices))), want, "{why}");
    }
    chat.take_keys(&[phone(2)]);
    let dead = chat.tried(&ADA, seal(&[A1, B1, B2]));
    assert_eq!(code_of(dead), code::WRONG_STATE, "Bo's phone is gone");
    assert_eq!(epoch(&chat), None);
    chat.ok(&BO, seal(&[A1, B1]));
    assert_eq!(epoch(&chat), Some(0));
    assert_eq!(wraps(&chat, &[A1, B1]).len(), 2);
    let twice = chat.tried(&ADA, seal(&[A1, B1]));
    assert_eq!(code_of(twice), code::WRONG_STATE, "sealed already");
}

#[test]
fn only_a_direct_room_between_two_people_is_sealed() {
    let mut chat = dm();
    let agent = Principal::Account(AGENT);
    let with_agent = Op::CreateDmChannel {
        counterpart: AGENT,
        name: "ada and agent".into(),
    };
    chat.ok(&ADA, with_agent);
    chat.publish(&ADA, laptop(1), A1);
    let refused = chat.offer(&agent, laptop(AGENT), B1);
    assert_eq!(
        code_of(refused),
        code::UNAUTHORIZED,
        "an agent publishes no device"
    );
    let to_agent = Op::Seal {
        channel_id: crate::dm_channel_id(1, AGENT),
        wraps: vec![wrapped(A1), wrapped(B1)],
    };
    assert_eq!(code_of(chat.tried(&ADA, to_agent)), code::UNAUTHORIZED);
    chat.ok(&ADA, create("general", PostPolicy::MembersOnly));
    let room = Op::Seal {
        channel_id: "general".into(),
        wraps: vec![wrapped(A1)],
    };
    assert_eq!(code_of(chat.tried(&ADA, room)), code::INVALID_INPUT);
    let asked = Query::Sealing {
        channel_ids: vec![crate::dm_channel_id(1, AGENT), "general".into()],
    };
    assert_eq!(chat.ask(asked), Reply::Sealing(Vec::new()));
}

#[test]
fn a_sealed_room_takes_only_sealed_ops_and_ops_that_carry_no_text() {
    let mut chat = sealed_dm();
    chat.ok(&ADA, sealed_post("s1", 0, None));
    let room = || ROOM.to_string();
    let notice = Notice {
        kind: "pod".into(),
        group: "p".into(),
        subject: "pod p".into(),
        link: None,
        status: crate::message::Status::Note,
        detail: "the plan".into(),
        concerns: vec![1],
    };
    let carrying_text = [
        post(ROOM, "p1", "plain words", None),
        Op::Post {
            channel_id: room(),
            message_id: "p2".into(),
            text: "plain words".into(),
            thread: None,
        },
        Op::EditMessage {
            channel_id: room(),
            seq: 1,
            blocks: parse_message("plain words"),
            base_rev: None,
        },
        Op::Notify {
            channel_id: room(),
            message_id: "n1".into(),
            thread: None,
            notice: notice.clone(),
        },
        Op::NotifyWith {
            channel_id: room(),
            message_id: "n2".into(),
            thread: None,
            notice,
            by: None,
            text: Some("plain words".into()),
        },
        Op::Drafting {
            channel_id: room(),
            thread: None,
            text: "plain".into(),
            anew: false,
            doing: Some("writing the plan".into()),
        },
        Op::SetChannelMeta {
            channel_id: room(),
            key: "rules".into(),
            value: "plain words".into(),
        },
        Op::RemoveChannelMeta {
            channel_id: room(),
            key: "rules".into(),
        },
        Op::RenameChannel {
            channel_id: room(),
            name: "plain words".into(),
        },
        Op::SetPostPolicy {
            channel_id: room(),
            post_policy: PostPolicy::Open,
        },
        Op::Link {
            channel_id: room(),
            peer: None,
        },
        Op::FromPeer {
            channel_id: room(),
            write: PeerWrite::Delete {
                message: PeerMessage::There("m1".into()),
            },
        },
    ];
    for op in carrying_text {
        let refused = chat.tried(&ADA, op.clone()).unwrap_err();
        assert_eq!(refused.code, code::WRONG_STATE, "{op:?}");
        assert!(refused.message.contains("is sealed"), "{op:?}: {refused}");
    }
    let textless = [
        Op::Listen {
            channel_id: room(),
            listening: true,
        },
        Op::SetNotices {
            channel_id: room(),
            notices: None,
        },
        Op::Watch {
            channel_id: room(),
            watching: true,
        },
        Op::ChooseForMany {
            accounts: vec![1],
            channel_id: room(),
            listening: false,
            notices: None,
        },
        Op::Take {
            channel_id: room(),
            thread: None,
        },
        Op::Release {
            channel_id: room(),
            thread: None,
        },
        Op::Break {
            channel_id: room(),
            thread: None,
        },
        Op::Undraft { channel_id: room() },
        Op::AddReaction {
            channel_id: room(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::RemoveReaction {
            channel_id: room(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::DeleteMessage {
            channel_id: room(),
            seq: 1,
        },
    ];
    for op in textless {
        if let Err(refused) = chat.tried(&ADA, op.clone()) {
            panic!("{op:?} refused: {refused}");
        }
    }
}

#[test]
fn a_reaction_in_a_sealed_room_is_one_fully_qualified_emoji() {
    let mut chat = sealed_dm();
    chat.ok(&ADA, sealed_post("s1", 0, None));
    let reaction = |emoji: &str| Op::AddReaction {
        channel_id: ROOM.into(),
        seq: 1,
        emoji: emoji.into(),
    };
    for emoji in [
        "\u{1F44D}",
        "\u{2764}\u{FE0F}",
        "\u{1F44D}\u{1F3FD}",
        "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}",
        "1\u{FE0F}\u{20E3}",
    ] {
        if let Err(refused) = chat.tried(&BO, reaction(emoji)) {
            panic!("{emoji} refused: {refused}");
        }
    }
    for emoji in [
        "a",
        "1",
        "\u{4E2D}",
        "e\u{301}\u{301}\u{301}",
        "looks good to me",
        "\u{2764}",
        "\u{1F44D}\u{1F44D}",
    ] {
        let refused = chat.tried(&BO, reaction(emoji));
        assert_eq!(code_of(refused), code::INVALID_INPUT, "{emoji:?}");
    }
    chat.ok(&ADA, create("general", PostPolicy::Open));
    chat.post(&ADA, "g1", "hello", None);
    chat.ok(&BO, react(1, "looks good", true));
}

#[test]
fn a_sealed_post_names_the_rooms_current_epoch_exactly() {
    let mut chat = sealed_dm();
    let ahead = chat.tried(&ADA, sealed_post("s1", 1, None));
    assert_eq!(
        code_of(ahead),
        code::WRONG_STATE,
        "an epoch not yet installed"
    );
    chat.ok(&ADA, sealed_post("s1", 0, None));
    chat.ok(&BO, rekey(1, &[A1, B1]));
    let behind = chat.tried(&ADA, sealed_post("s2", 0, None));
    assert_eq!(code_of(behind), code::WRONG_STATE, "an epoch rekeyed past");
    chat.ok(&ADA, sealed_post("s2", 1, None));
    assert_eq!(epoch(&chat), Some(1));
}

#[test]
fn every_existing_read_shows_the_fixed_line_and_sealed_gives_the_bytes() {
    let mut chat = sealed_dm();
    chat.ok(&ADA, sealed_post("s1", 0, None));
    chat.ok(&BO, sealed_post("s2", 0, Some(1)));
    let roots = roots(&chat);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].blocks, line());
    assert_eq!(roots[0].text, SEALED);
    let Reply::Message(Some(by_id)) = chat.ask(Query::MessageById {
        message_id: "s1".into(),
    }) else {
        panic!("s1 is a message");
    };
    assert_eq!(by_id.blocks, line());
    let Reply::Thread { root, replies } = chat.ask(Query::Thread {
        channel_id: ROOM.into(),
        root_seq: 1,
        viewer: vec![],
        page: PageRequest::default(),
    }) else {
        panic!("Thread answers Thread");
    };
    assert_eq!(root.unwrap().blocks, line());
    assert_eq!(replies.items[0].blocks, line());
    let Reply::Messages(around) = chat.ask(Query::MessagesAround {
        channel_id: ROOM.into(),
        seq: 1,
        viewer: vec![],
        page: PageRequest {
            after: None,
            limit: Some(4),
        },
    }) else {
        panic!("MessagesAround answers Messages");
    };
    assert_eq!(around.len(), 2);
    assert!(around.iter().all(|row| row.blocks == line()));
    let Reply::Repliers(answered) = chat.ask(Query::Repliers {
        channel_id: ROOM.into(),
        roots: vec![1],
    }) else {
        panic!("Repliers answers Repliers");
    };
    assert_eq!(answered[0].1[0].text, SEALED);
    let rows = sealed(&chat, vec![2, 1, 9]);
    let seqs: Vec<u64> = rows.iter().map(|row| row.seq).collect();
    assert_eq!(
        seqs,
        [2, 1],
        "in the order asked, a seq with no body left out"
    );
    assert_eq!(rows[1].message_id, "s1");
    assert_eq!(rows[1].author, ADA);
    assert_eq!((rows[1].epoch, rows[1].nonce), (0, [5; 24]));
    assert!(rows[1].ct == b"opaque", "the bytes as posted");
}

#[test]
fn search_leaves_a_sealed_room_out_with_its_history() {
    let mut chat = dm();
    chat.ok(&ADA, post(ROOM, "p1", "before the seal #plan", None));
    assert_eq!(chat.search("before"), [1]);
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&BO, laptop(2), B1);
    chat.ok(&ADA, seal(&[A1, B1]));
    chat.ok(&ADA, sealed_post("s1", 0, None));
    assert!(chat.search("before").is_empty());
    assert!(chat.search("sealed").is_empty());
    let Reply::TagHits(tagged) = chat.ask(Query::TagSearch {
        tag: "plan".into(),
        viewer: vec![],
        channel_id: Some(ROOM.into()),
        page: PageRequest::default(),
    }) else {
        panic!("TagSearch answers TagHits");
    };
    assert!(tagged.items.is_empty());
}

#[test]
fn a_post_waits_for_a_rekey_once_a_device_holding_the_epoch_is_gone() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&ADA, phone(1), A2);
    chat.publish(&BO, laptop(2), B1);
    chat.ok(&ADA, seal(&[A1, A2, B1]));
    chat.ok(&BO, sealed_post("s1", 0, None));
    chat.take_keys(&[phone(1)]);
    let refused = chat.tried(&BO, sealed_post("s2", 0, None)).unwrap_err();
    assert_eq!(refused.code, code::WRONG_STATE);
    assert!(refused.message.contains("rekey first"), "{refused}");
    chat.ok(&BO, rekey(1, &[A1, B1]));
    chat.ok(&BO, sealed_post("s2", 1, None));
    chat.signed(&BO, laptop(2), Op::RetireDeviceKey { key: B1 })
        .unwrap();
    let retired = chat.tried(&ADA, sealed_post("s3", 1, None)).unwrap_err();
    assert!(retired.message.contains("rekey first"), "{retired}");
    chat.ok(&ADA, rekey(2, &[A1]));
    chat.ok(&ADA, sealed_post("s3", 2, None));
}

#[test]
fn a_rekey_covers_exactly_the_live_devices_already_holding_the_epoch() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&ADA, phone(1), A2);
    chat.publish(&BO, laptop(2), B1);
    chat.ok(&ADA, seal(&[A1, A2, B1]));
    chat.take_keys(&[phone(1)]);
    chat.publish(&ADA, tablet(1), A3);
    let refusals = [
        (
            1,
            vec![A1, B1, A3],
            code::WRONG_STATE,
            "a new device enters only by Wrap",
        ),
        (
            1,
            vec![A1, B1, A2],
            code::WRONG_STATE,
            "a device that is gone",
        ),
        (1, vec![A1], code::WRONG_STATE, "Bo's laptop left out"),
        (1, vec![A1, A1, B1], code::INVALID_INPUT, "a device twice"),
        (2, vec![A1, B1], code::WRONG_STATE, "an epoch skipped"),
        (0, vec![A1, B1], code::WRONG_STATE, "the epoch in place"),
    ];
    for (at, devices, want, why) in refusals {
        assert_eq!(code_of(chat.tried(&BO, rekey(at, &devices))), want, "{why}");
    }
    chat.ok(&BO, rekey(1, &[A1, B1]));
    assert_eq!(epoch(&chat), Some(1));
    assert!(wraps(&chat, &[A3]).is_empty(), "A3 holds nothing yet");
    assert!(device_notices(&chat).is_empty());
}

#[test]
fn a_new_device_is_wrapped_by_its_own_side_or_by_the_other_once_none_of_its_own_holds_the_key() {
    let mut chat = sealed_dm();
    chat.publish(&BO, phone(2), B2);
    let early = chat.tried(&ADA, give(0, B2));
    assert_eq!(
        code_of(early),
        code::UNAUTHORIZED,
        "Bo's laptop still holds the key"
    );
    chat.ok(&BO, give(0, B2));
    assert_eq!(device_notices(&chat).len(), 1);
    chat.take_keys(&[laptop(2), phone(2)]);
    chat.publish(&BO, tablet(2), B3);
    chat.ok(&ADA, give(0, B3));
    assert_eq!(device_notices(&chat).len(), 2);
    chat.publish(&CY, laptop(3), C1);
    let stranger = chat.tried(&ADA, give(0, C1));
    assert_eq!(code_of(stranger), code::UNAUTHORIZED, "a device of neither");
    let gone = chat.tried(&ADA, give(0, B1));
    assert_eq!(code_of(gone), code::WRONG_STATE, "a device that is gone");
    let ahead = chat.tried(&BO, give(1, B3));
    assert_eq!(
        code_of(ahead),
        code::INVALID_INPUT,
        "an epoch not yet installed"
    );
}

#[test]
fn a_bad_wrap_is_marked_then_replaced_and_only_a_devices_first_wrap_posts_the_notice() {
    let mut chat = sealed_dm();
    assert!(device_notices(&chat).is_empty(), "sealing posts no notice");
    let mark = Op::Unopenable {
        channel_id: ROOM.into(),
        epoch: 0,
        device: B1,
    };
    let by_ada = chat.signed(&ADA, laptop(1), mark.clone());
    assert_eq!(
        code_of(by_ada),
        code::UNAUTHORIZED,
        "only the key that published it"
    );
    chat.signed(&BO, laptop(2), mark).unwrap();
    assert!(wraps(&chat, &[B1])[0].unopenable);
    chat.ok(&ADA, give(0, B1));
    let rows = wraps(&chat, &[B1]);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].unopenable);
    assert_eq!(
        rows[0].wrapped,
        vec![9; WRAPPED],
        "the later wrap replaced it"
    );
    assert!(
        device_notices(&chat).is_empty(),
        "a device sealed in is no new device"
    );
    chat.publish(&ADA, phone(1), A2);
    chat.ok(&ADA, give(0, A2));
    chat.ok(&ADA, give(0, A2));
    let notices = device_notices(&chat);
    assert_eq!(notices.len(), 1, "a replacing wrap posts nothing more");
    assert_eq!(notices[0].notice().unwrap().concerns, [1, 2]);
    assert!(
        notices[0].text.contains("added a device"),
        "{}",
        notices[0].text
    );
}

#[test]
fn deleting_a_sealed_post_drops_its_body() {
    let mut chat = sealed_dm();
    chat.ok(&ADA, sealed_post("s1", 0, None));
    assert_eq!(sealed(&chat, vec![1]).len(), 1);
    let delete = Op::DeleteMessage {
        channel_id: ROOM.into(),
        seq: 1,
    };
    chat.ok(&ADA, delete);
    assert!(sealed(&chat, vec![1]).is_empty());
}

#[test]
fn a_wrap_is_refused_unless_it_is_exactly_the_bytes_seal_makes() {
    let mut chat = dm();
    chat.publish(&ADA, laptop(1), A1);
    chat.publish(&BO, laptop(2), B1);
    let sized = |len: usize| DeviceWrap {
        device: B1,
        wrapped: vec![1; len],
    };
    for len in [0, WRAPPED - 1, WRAPPED + 1] {
        let op = Op::Seal {
            channel_id: ROOM.into(),
            wraps: vec![wrapped(A1), sized(len)],
        };
        let refused = code_of(chat.tried(&ADA, op));
        assert_eq!(refused, code::INVALID_INPUT, "a seal's wrap of {len} bytes");
    }
    chat.ok(&ADA, seal(&[A1, B1]));
    for len in [WRAPPED - 1, WRAPPED + 1] {
        let op = Op::Rekey {
            channel_id: ROOM.into(),
            epoch: 1,
            wraps: vec![wrapped(A1), sized(len)],
        };
        let refused = code_of(chat.tried(&ADA, op));
        assert_eq!(
            refused,
            code::INVALID_INPUT,
            "a rekey's wrap of {len} bytes"
        );
        let op = Op::Wrap {
            channel_id: ROOM.into(),
            wraps: vec![EpochWrap {
                epoch: 0,
                device: A1,
                wrapped: vec![1; len],
            }],
        };
        let refused = code_of(chat.tried(&ADA, op));
        assert_eq!(refused, code::INVALID_INPUT, "a wrap of {len} bytes");
    }
    assert_eq!(epoch(&chat), Some(0));
    chat.ok(&ADA, give(0, A1));
    chat.ok(&ADA, rekey(1, &[A1, B1]));
}

fn device_rows(chat: &Chat, accounts: Vec<u64>) -> Vec<DeviceKey> {
    let Reply::DeviceKeys(keys) = chat.ask(Query::DeviceKeys { accounts }) else {
        panic!("DeviceKeys answers DeviceKeys");
    };
    keys
}

#[test]
fn a_device_key_is_taken_only_under_its_publishers_signature_over_this_account_network_and_key() {
    let mut chat = dm();
    let good = statement(b"net", 1, A1);
    let op = |sig| Op::PublishDeviceKey { key: A1, sig };
    let refusals = [
        (vec![7; 64], "a forged sig"),
        (
            signed_by(&phone(1), &good),
            "another of Ada's keys signed it",
        ),
        (
            signed_by(&laptop(1), &statement(b"net", 2, A1)),
            "over another account",
        ),
        (
            signed_by(&laptop(1), &statement(b"elsewhere", 1, A1)),
            "over another network",
        ),
        (
            signed_by(&laptop(1), &statement(b"net", 1, A2)),
            "over another key",
        ),
        (
            signed_by(
                &laptop(1),
                &borsh::to_vec(&(b"net".to_vec(), 1u64, A1)).unwrap(),
            ),
            "the bare message, not as chat-view's seal-device/v1",
        ),
        (
            signature(Scheme::Ed25519, &laptop(1), b"net", &good),
            "under the frame namespace, not VIEW_SIGN",
        ),
    ];
    for (sig, why) in refusals {
        let refused = chat.signed(&ADA, laptop(1), op(sig));
        assert_eq!(code_of(refused), code::INVALID_INPUT, "{why}");
    }
    assert!(device_keys(&chat, vec![1]).is_empty());
    let sig = signed_by(&laptop(1), &good);
    chat.signed(&ADA, laptop(1), op(sig.clone())).unwrap();
    let height = chat.height;
    let again = chat.signed(&ADA, laptop(1), op(vec![7; 64]));
    assert_eq!(
        code_of(again),
        code::INVALID_INPUT,
        "a republish is signed too"
    );
    chat.signed(&ADA, laptop(1), op(sig.clone())).unwrap();
    let row = DeviceKey {
        account: 1,
        key: A1,
        height,
        signer: laptop(1),
        scheme: Scheme::Ed25519,
        sig,
    };
    assert_eq!(device_rows(&chat, vec![1]), vec![row]);
}

#[test]
fn a_device_key_is_checked_in_the_scheme_identity_holds_for_its_signer() {
    let mut chat = dm();
    let good = statement(b"net", 1, A3);
    let as_ed25519 = signature(Scheme::Ed25519, &tablet(1), b"ducktape/view-sign/v1", &good);
    let op = Op::PublishDeviceKey {
        key: A3,
        sig: as_ed25519,
    };
    let wrong = chat.signed(&ADA, tablet(1), op);
    assert_eq!(
        code_of(wrong),
        code::INVALID_INPUT,
        "identity holds the tablet's key as P-256"
    );
    chat.publish(&ADA, tablet(1), A3);
    let rows = device_rows(&chat, vec![1]);
    let held: Vec<_> = rows
        .iter()
        .map(|row| (row.signer.clone(), row.scheme))
        .collect();
    assert_eq!(held, vec![(tablet(1), Scheme::Secp256r1)]);
}

#[test]
fn only_a_person_publishes_a_device_key() {
    let mut chat = dm();
    let agent = Principal::Account(AGENT);
    let refused = chat.offer(&agent, laptop(AGENT), C1);
    assert_eq!(
        code_of(refused),
        code::UNAUTHORIZED,
        "an agent runs on the network"
    );
    assert!(device_keys(&chat, vec![AGENT]).is_empty());
}

#[test]
fn the_device_statement_is_chat_views_seal_device_message_as_key_sign_frames_it() {
    assert_eq!(crate::VIEW_SIGN, "ducktape/view-sign/v1");
    assert_eq!(
        (crate::DEVICE_VIEW, crate::DEVICE_PURPOSE),
        ("chat-view", "seal-device/v1")
    );
    let message = borsh::to_vec(&(b"net".to_vec(), 1u64, A1)).unwrap();
    assert_eq!(crate::device_message(b"net", 1, &A1), message);
    assert_eq!(
        crate::device_statement(b"net", 1, &A1),
        statement(b"net", 1, A1)
    );
}
