//! The wire, committed: its bytes and its shape. `frame.bin` holds a
//! `Frame` with every `Node` variant in its tree and every style field in
//! its table, then every `Event`, every
//! `WidgetCommand`, and every unit variant those leave out
//! (`golden/units.rs`); `methods.bin` holds at least one request and reply
//! through every method in `methods::ALL`; each has a JSON twin beside it
//! for readable diffs. `tests/golden/schema.txt` (`golden/schema.rs`) holds
//! the shape of every type that crosses. The bytes fail on any sampled byte
//! that moves. The shape holds every field and variant, sampled or not, and
//! `golden/coverage.rs` fails when a sample leaves one out: on the tree side
//! every variant, and every struct field seen present, that `Frame`,
//! `Event` or `WidgetCommand` reaches; on the borsh side every variant,
//! `Option`'s `None` and `Some` included, that a method with no target
//! reaches. A failure means the wire changed; if that was intended,
//! regenerate with `WIRE_GOLDEN_WRITE=1`, which moves `WIRE_ID` (`build.rs`
//! hashes all three files).
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::PathBuf;

use gpui::{Bounds, Pixels, StyleRefinement, point, px, size};
use view_wire::list::{
    ListCommand, UniformListHorizontalSizing, UniformListScrollRequest, UniformListScrollStrategy,
    UniformListSizing,
};
use view_wire::manifest::{Manifest, Tint};
use view_wire::methods::{self, Method, Program};
use view_wire::{
    Action, ActionData, Anchor, AnchoredFitMode, AnchoredPositionMode, Aria, AriaCurrent,
    CanvasCommand, CanvasLineCap, CanvasLineJoin, CanvasSegment, CanvasShape, CanvasStroke,
    ContainerNode, DispatchPhase, Edit, ElementIdAtom, ElementIdWire, Error, Event, Frame,
    GroupRefinement, HasPopup, ImageData, ImageObjectFit, ImageStyle, Interactivity, Invalid,
    KeyClaim, ListAlignment, ListOffset, ListRequest, ListScroll, ListSizingBehavior, Live,
    MediaState, MediaStatus, MicCommand, Node, Patch, Playback, RemoteMedia, Request,
    RichTextHighlightStyle, RichTextHover, RichTextRuns, Style, StyleId, SvgSource,
    SvgTransformation, TextChange, TextNode, TextRange, TextToken, TextTransform, Tooltip,
    TooltipResponse, TrackState, WebCommand, WebState, WidgetCommand, click, interactivity,
    keyboard, mouse,
};

const MESSAGE: &str =
    "the wire changed: if intended, regenerate with WIRE_GOLDEN_WRITE=1 (this changes WIRE_ID)";

/// Bumped by hand with the enum: `node_variant` and `event_variant` fail to compile until
/// the fixture names the new one, and this count keeps the fixture honest.
const NODE_VARIANTS: usize = 21;
const EVENT_VARIANTS: usize = 39;

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn check(name: &str, actual: &[u8], json: &str) {
    let bin = golden(&format!("{name}.bin"));
    if std::env::var_os("WIRE_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, actual).unwrap();
        std::fs::write(golden(&format!("{name}.json")), json).unwrap();
        return;
    }
    let expected =
        std::fs::read(&bin).unwrap_or_else(|error| panic!("{}: {error}; {MESSAGE}", bin.display()));
    if expected != actual {
        let first = expected
            .iter()
            .zip(actual)
            .position(|(a, b)| a != b)
            .unwrap_or(expected.len().min(actual.len()));
        panic!(
            "{MESSAGE} ({name}.bin: {} bytes expected, {} built, first difference at byte {first})",
            expected.len(),
            actual.len()
        );
    }
}

fn id(name: &str) -> ElementIdWire {
    ElementIdWire::Name(name.into())
}

fn style() -> StyleId {
    StyleId(0)
}

fn text(content: &str) -> Node {
    Node::Text(TextNode {
        id: Some(id(content)),
        style: style(),
        content: content.into(),
        transform: None,
    })
}

fn boxed(content: &str) -> Box<Node> {
    Box::new(text(content))
}

fn key_state() -> keyboard::KeyState {
    keyboard::KeyState {
        key: keyboard::Key::Named(keyboard::Named::Enter),
        modified_key: keyboard::Key::Character("\n".into()),
        physical_key: keyboard::Physical::Unidentified(keyboard::NativeCode::MacOS(36)),
        location: keyboard::Location::Standard,
        modifiers: gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
    }
}

fn key_state_on(physical: keyboard::NativeCode) -> keyboard::KeyState {
    keyboard::KeyState {
        physical_key: keyboard::Physical::Unidentified(physical),
        ..key_state()
    }
}

fn at(x: f32, y: f32) -> gpui::Point<Pixels> {
    point(px(x), px(y))
}

#[path = "golden/coverage.rs"]
mod coverage;
#[path = "golden/events.rs"]
mod events_fixture;
#[path = "golden/nodes.rs"]
mod nodes;
#[path = "golden/schema.rs"]
mod schema;
#[path = "golden/units.rs"]
mod units_fixture;
#[path = "golden/widget.rs"]
mod widget_fixture;
use events_fixture::{event_variant, every_event, every_frame};
use nodes::{every_node, every_style, node_variant};
use units_fixture::{Units, every_unit};
use widget_fixture::every_widget_command;

#[test]
fn frame_and_events_are_the_committed_bytes() {
    let frame = every_frame();
    let events = every_event();
    let widget_commands = every_widget_command();
    let units = every_unit(&schema::wire());
    let root = frame.root.as_ref().unwrap();
    let Node::Container(ContainerNode { children, .. }) = root else {
        unreachable!()
    };
    let nodes: BTreeSet<_> = children.iter().chain([root]).map(node_variant).collect();
    assert_eq!(nodes.len(), NODE_VARIANTS, "every Node variant: {nodes:?}");
    let kinds: BTreeSet<_> = events.iter().map(event_variant).collect();
    assert_eq!(
        kinds.len(),
        EVENT_VARIANTS,
        "every Event variant: {kinds:?}"
    );

    for (entry, style) in frame.styles.iter().zip(every_style()) {
        assert_eq!(entry.read(), Ok(style));
    }

    let value = (frame, events, widget_commands, units);
    let bytes = view_wire::encode(&value);
    let json = serde_json::to_string_pretty(&value).unwrap();
    check("frame", &bytes, &json);
    assert_eq!(
        view_wire::decode::<(Frame, Vec<Event>, Vec<WidgetCommand>, Units)>(&bytes).unwrap(),
        value
    );
}

struct Golden;
impl Program for Golden {
    const NAME: &'static str = "golden";
    type Op = String;
    type Query = (u64, String);
    type Reply = Vec<u32>;

    fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
        unreachable!()
    }

    fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
        unreachable!()
    }
}

type Exchange = (String, Vec<u8>, Vec<u8>);

fn exchange<D: Method>(request: D::Request, reply: D::Reply) -> (Exchange, serde_json::Value)
where
    D::Request: serde::Serialize,
    D::Reply: serde::Serialize,
{
    let json = serde_json::json!({ "kind": D::KIND, "request": request, "reply": reply });
    (
        (
            D::KIND.into(),
            D::encode_request(&request),
            D::encode_reply(&reply),
        ),
        json,
    )
}

fn every_method() -> Vec<(Exchange, serde_json::Value)> {
    use methods::*;
    vec![
        exchange::<Query<Golden>>((7, "q".into()), vec![1, 2, 3]),
        exchange::<Submit<Golden>>(
            "op".into(),
            Submitted::Applied {
                frame: [7; 32],
                until: 1_300,
                output: b"receipt".to_vec(),
            },
        ),
        exchange::<ChainStatus>(
            (),
            NodeStatus {
                chain_id: "local#1".into(),
                time: 1,
                block_time_ms: 500,
                epoch_length: 100,
                height: 9,
                tip: [1; 32],
                root: [2; 32],
                epoch: 0,
                identity: vec![3; 32],
                contract: 1,
            },
        ),
        exchange::<ChainNetwork>(
            (),
            NetworkStatus {
                height: 9,
                members: vec![
                    Peer {
                        key: vec![3; 32],
                        signed: Some(9),
                    },
                    Peer {
                        key: vec![4; 32],
                        signed: None,
                    },
                ],
            },
        ),
        exchange::<InviteCreate>(
            CreateInvite { ttl_days: 7 },
            Invite {
                invite: "duck://invite/x".into(),
                notes: vec![Error {
                    code: "not_yet".into(),
                    message: "the invite is not indexed yet".into(),
                }],
            },
        ),
        exchange::<methods::Changes<Golden>>(
            (),
            Some(methods::Change {
                height: 9,
                keys: vec![b"a/1".to_vec(), b"b".to_vec()],
            }),
        ),
        exchange::<methods::Changes<Golden>>((), None),
        exchange::<ChainBlocks>(
            BlockPage {
                before: Some(10),
                limit: 2,
                most: 1,
            },
            BlockList {
                blocks: vec![Block {
                    height: 9,
                    id: [4; 32],
                    parent: [5; 32],
                    time: 1,
                    epoch: 0,
                    proposer: Some(vec![6; 32]),
                    txs: vec![Tx {
                        hash: [7; 32],
                        signer: vec![8; 32],
                        anchor: Tip {
                            height: 8,
                            id: [3; 32],
                        },
                        target: "chat".into(),
                        size: 1,
                        payload: Some(vec![9]),
                        receipt: Some(Receipt {
                            program: "chat".into(),
                            outcome: Outcome::Rejected(Error {
                                code: "unauthorized".into(),
                                message: "not a member".into(),
                            }),
                            events: vec![vec![1]],
                            nested: vec![Receipt {
                                program: "identity".into(),
                                outcome: Outcome::Applied { output: vec![2] },
                                events: Vec::new(),
                                nested: Vec::new(),
                            }],
                        }),
                    }],
                }],
                listed_from: Some(3),
            },
        ),
        exchange::<ChainBlock>(
            BlockAsk {
                by: BlockRef::Id([4; 32]),
                most: 0,
            },
            None,
        ),
        exchange::<ChainBlock>(
            BlockAsk {
                by: BlockRef::Height(9),
                most: u32::MAX,
            },
            Some(Block {
                height: 9,
                id: [4; 32],
                parent: [5; 32],
                time: 1,
                epoch: 0,
                proposer: None,
                txs: vec![Tx {
                    hash: [7; 32],
                    signer: vec![8; 32],
                    anchor: Tip {
                        height: 7,
                        id: [2; 32],
                    },
                    target: "chat".into(),
                    size: 5000,
                    payload: None,
                    receipt: None,
                }],
            }),
        ),
        exchange::<ChainTx>(
            TxAsk {
                tx: TxRef::At {
                    height: 9,
                    position: 0,
                },
                most: u32::MAX,
            },
            Some(Located {
                height: 9,
                position: 0,
                time: 1,
                tx: Tx {
                    hash: [7; 32],
                    signer: vec![8; 32],
                    anchor: Tip {
                        height: 7,
                        id: [2; 32],
                    },
                    target: "chat".into(),
                    size: 2,
                    payload: Some(vec![9, 9]),
                    receipt: None,
                },
            }),
        ),
        exchange::<ChainTx>(
            TxAsk {
                tx: TxRef::Hash([7; 32]),
                most: 0,
            },
            None,
        ),
        exchange::<BlobGet>("sha256:00".into(), Some(b"blob".to_vec())),
        exchange::<BlobManifest>(
            "sha256:01".into(),
            Some(Manifested {
                size: 512,
                view: Some(CarriedView {
                    size: 256,
                    manifest: None,
                }),
            }),
        ),
        exchange::<BlobManifest>(
            "sha256:02".into(),
            Some(Manifested {
                size: 128,
                view: None,
            }),
        ),
        exchange::<BlobManifest>("sha1:03".into(), None),
        exchange::<HostSession>(
            (),
            Session {
                connected: true,
                chain_id: "local#1".into(),
                signer: "ab01".into(),
                account: Some(3),
                endpoint: "http://127.0.0.1:1".into(),
            },
        ),
        exchange::<HostVisible>((), true),
        exchange::<HostFloat>(
            Float {
                width: 320,
                height: 200,
                min_width: 240,
                min_height: 150,
                on_top: true,
            },
            Placement {
                floating: true,
                on_top: true,
            },
        ),
        exchange::<HostFloat>(
            Float {
                width: 320,
                height: 200,
                min_width: 240,
                min_height: 150,
                on_top: false,
            },
            Placement {
                floating: true,
                on_top: false,
            },
        ),
        exchange::<HostUnfloat>((), ()),
        exchange::<HostPlacement>(
            (),
            Placement {
                floating: false,
                on_top: false,
            },
        ),
        exchange::<HostBadge>(3, ()),
        exchange::<HostTitle>("general".into(), ()),
        exchange::<HostShow>(Place::Account, ()),
        exchange::<HostShow>(Place::CameraSettings, ()),
        exchange::<HostShow>(Place::MicrophoneSettings, ()),
        exchange::<CameraDevices>(
            (),
            vec![
                CameraDevice {
                    id: "0".into(),
                    name: "FaceTime HD Camera".into(),
                    default: true,
                    facing: Facing::Front,
                },
                CameraDevice {
                    id: "1".into(),
                    name: "Back Camera".into(),
                    default: false,
                    facing: Facing::Back,
                },
                CameraDevice {
                    id: "2".into(),
                    name: "USB Camera".into(),
                    default: false,
                    facing: Facing::External,
                },
            ],
        ),
        exchange::<MicrophoneDevices>(
            (),
            AudioDevices {
                inputs: vec![
                    AudioDevice {
                        id: "Built-in Microphone".into(),
                        name: "Built-in Microphone".into(),
                        default: true,
                        kind: AudioKind::Other,
                    },
                    AudioDevice {
                        id: "AirPods".into(),
                        name: "AirPods".into(),
                        default: false,
                        kind: AudioKind::Headphones,
                    },
                ],
                outputs: vec![
                    AudioDevice {
                        id: "Speaker".into(),
                        name: "Speaker".into(),
                        default: true,
                        kind: AudioKind::Speaker,
                    },
                    AudioDevice {
                        id: "Earpiece".into(),
                        name: "iPhone".into(),
                        default: false,
                        kind: AudioKind::Earpiece,
                    },
                    AudioDevice {
                        id: "Headphones".into(),
                        name: "Headphones".into(),
                        default: false,
                        kind: AudioKind::Headphones,
                    },
                ],
            },
        ),
        exchange::<MicrophoneChime>(Some("Headphones".into()), ()),
        exchange::<MicrophoneChime>(None, ()),
        exchange::<FilePick>(
            Pick { multiple: true },
            vec![
                Picked {
                    name: "tape.png".into(),
                    size: 3,
                    file: 1,
                },
                Picked {
                    name: "film.mov".into(),
                    size: 1 << 30,
                    file: 2,
                },
            ],
        ),
        exchange::<FilePick>(Pick { multiple: false }, Vec::new()),
        exchange::<OpPut>(
            PutBlobs {
                blobs: vec![
                    NewBlob {
                        kind: "chunk".into(),
                        body: BlobBody::Picked {
                            file: 1,
                            offset: 0,
                            len: 1 << 20,
                        },
                    },
                    NewBlob {
                        kind: "chunk".into(),
                        body: BlobBody::Bytes(b"tape".to_vec()),
                    },
                ],
            },
            Putting::Held(vec![[0x11; 32], [0x22; 32]]),
        ),
        exchange::<OpPut>(
            PutBlobs {
                blobs: vec![NewBlob {
                    kind: "chunk".into(),
                    body: BlobBody::Bytes(b"tape".to_vec()),
                }],
            },
            Putting::Sent(12),
        ),
        exchange::<FileRelease>(vec![1, 2], ()),
        exchange::<methods::FileRead>(
            methods::ReadFile {
                file: 1,
                offset: 1,
                len: 2,
            },
            b"ap".to_vec(),
        ),
        exchange::<methods::SocketConnect>(
            telegram_socket(),
            methods::SocketItem::Opened {
                protocol: Some("binary".into()),
            },
        ),
        exchange::<methods::SocketConnect>(
            telegram_socket(),
            methods::SocketItem::Binary(vec![0xef, 1, 2]),
        ),
        exchange::<methods::SocketConnect>(
            telegram_socket(),
            methods::SocketItem::Text("hello".into()),
        ),
        exchange::<methods::SocketConnect>(
            telegram_socket(),
            methods::SocketItem::Closed {
                code: 1000,
                reason: "bye".into(),
            },
        ),
        exchange::<methods::SocketSendMessage>(
            methods::SocketSend {
                socket: 1,
                body: vec![1, 2, 3],
            },
            (),
        ),
        exchange::<methods::HostRandom>(4, vec![9, 8, 7, 6]),
        exchange::<methods::StoreSecretGet>("dc/2".into(), Some(vec![1, 2, 3])),
        exchange::<methods::StoreSecretSet>(("dc/2".into(), Some(vec![1, 2, 3])), ()),
        exchange::<methods::HostDebug>((), true),
        exchange::<methods::KeySign>(
            methods::Signing {
                purpose: "seal-device/v1".into(),
                message: vec![1, 2, 3],
            },
            vec![7; 64],
        ),
        exchange::<FileSave>(
            SaveFile {
                name: "tape.png".into(),
                blobs: vec![[0x11; 32]],
            },
            true,
        ),
        exchange::<FileSave>(
            SaveFile {
                name: "tape.png".into(),
                blobs: Vec::new(),
            },
            false,
        ),
        exchange::<ClipboardFiles>(
            (),
            Pasted {
                files: vec![Picked {
                    name: "image.png".into(),
                    size: 3,
                    file: 3,
                }],
                text: String::new(),
            },
        ),
        exchange::<HostMethods>(
            (),
            Hosted {
                level: 1,
                kinds: vec!["link.open".into(), "host.methods".into()],
            },
        ),
        exchange::<LinkOpen>("duck://chat/room".into(), ()),
        exchange::<LinkWindow>("duck://chat/room".into(), ()),
        exchange::<HostRoute>((), "tx/00ff".into()),
        exchange::<HostId>("msg".into(), "msg-1".into()),
        exchange::<ClockTicks>(1000, ()),
        exchange::<HostLog>("hello".into(), ()),
        exchange::<HostWidget>(
            WidgetCommand::Focus {
                target: vec![id("input")],
            },
            (),
        ),
        exchange::<ClipboardRead>(
            (),
            Clipboard {
                text: "copied".into(),
            },
        ),
        exchange::<ClipboardWrite>("copied".into(), ()),
        exchange::<NotifyPost>(notification(), Delivery::Banner),
        exchange::<NotifyPost>(notification(), Delivery::Logged),
        exchange::<NotifyPost>(notification(), Delivery::Blocked),
        exchange::<NotifyPost>(
            Notification {
                title: "alice is calling".into(),
                body: "Huddle with 3 people".into(),
                tag: "call/c-1".into(),
                link: "duck://local/huddle/call/c-1".into(),
                kind: NoticeKind::Call,
                until: 1_700_000_045,
                end_link: "duck://local/huddle/call/c-1/leave".into(),
            },
            Delivery::Banner,
        ),
        exchange::<StoreGet>("reads/alice".into(), Some(vec![1, 2])),
        exchange::<StoreSet>(("reads/alice".into(), None), ()),
        exchange::<StoreSet>(("reads/alice".into(), Some(vec![1, 2])), ()),
        exchange::<NotifySeen>("#design".into(), ()),
        exchange::<NotifyNeeds>(
            Needs {
                items: vec![Need {
                    key: "ask/57".into(),
                    title: "tech asks: Which cache size?".into(),
                    body: "Going with 128 MB unless you pick".into(),
                    link: "duck://local/chat/j-212/12".into(),
                    opened_at: 1_700_000_000,
                    waits_on: Some(21),
                    also_closes: vec![17],
                }],
            },
            (),
        ),
        exchange::<NotifyNeeds>(Needs::default(), ()),
        exchange::<StoreChanges>((), "reads/alice".into()),
        exchange::<StoreList>("rooms/".into(), vec!["rooms/collapsed".into()]),
        exchange::<HostShowing>("general".into(), ()),
        exchange::<HostScreen>((), vec!["dm-3-5".into(), "general".into()]),
        exchange::<ChainHeads>(
            (),
            Head {
                height: 9,
                time: 1,
                id: [4; 32],
            },
        ),
        exchange::<ProgramDescribe>(
            ("chat".into(), vec![1, 2]),
            Some(Description {
                title: "Post in #design".into(),
                fields: vec![Field {
                    label: "from".into(),
                    value: Value::List(vec![Value::Account(3), Value::bytes(&[7; 40])]),
                }],
            }),
        ),
        exchange::<ProgramDescribe>(
            ("chat".into(), vec![1, 2]),
            Some(Description {
                title: "Post in #design".into(),
                fields: vec![
                    Field {
                        label: "from".into(),
                        value: Value::List(vec![Value::Account(3), Value::bytes(&[7; 40])]),
                    },
                    Field {
                        label: "text".into(),
                        value: Value::text("hi"),
                    },
                    Field {
                        label: "signer".into(),
                        value: Value::Key(vec![1; 32]),
                    },
                    Field {
                        label: "program".into(),
                        value: Value::Program("chat".into()),
                    },
                    Field {
                        label: "commit".into(),
                        value: Value::Hash(vec![2; 32]),
                    },
                    Field {
                        label: "sent".into(),
                        value: Value::Time(1_700_000_000_000),
                    },
                    Field {
                        label: "quantity".into(),
                        value: Value::Amount {
                            value: 1_000,
                            decimals: 2,
                        },
                    },
                ],
            }),
        ),
        exchange::<ProgramDescribe>(("chat".into(), vec![3]), None),
        exchange::<LivePublishTrack>(
            LivePublish {
                track: live_track("presence/3"),
                mode: LiveMode::Datagram,
                priority: 128,
            },
            Publishing::Waiting,
        ),
        exchange::<LivePublishTrack>(
            LivePublish {
                track: live_track("presence/3"),
                mode: LiveMode::Datagram,
                priority: 128,
            },
            Publishing::Subscribers(2),
        ),
        exchange::<LivePublishTrack>(
            LivePublish {
                track: live_track("doc"),
                mode: LiveMode::Stream,
                priority: 128,
            },
            Publishing::Cut { group: 4 },
        ),
        exchange::<LiveSendObject>(
            LiveSend {
                track: live_track("doc"),
                group: 4,
                object: 0,
                body: vec![1, 2],
                end: true,
            },
            (),
        ),
        exchange::<LiveSubscribeTrack>(
            LiveSubscribe {
                track: live_track("doc"),
                from: LiveStart::Latest,
            },
            LiveItem::Waiting,
        ),
        exchange::<LiveSubscribeTrack>(
            LiveSubscribe {
                track: live_track("doc"),
                from: LiveStart::Latest,
            },
            LiveItem::Opened {
                by: 3,
                mode: LiveMode::Stream,
                latest: Some(4),
            },
        ),
        exchange::<LiveSubscribeTrack>(
            LiveSubscribe {
                track: live_track("presence/3"),
                from: LiveStart::Next,
            },
            LiveItem::Opened {
                by: 3,
                mode: LiveMode::Datagram,
                latest: None,
            },
        ),
        exchange::<LiveSubscribeTrack>(
            LiveSubscribe {
                track: live_track("doc"),
                from: LiveStart::Latest,
            },
            LiveItem::Object {
                group: 4,
                object: 1,
                body: vec![7],
            },
        ),
        exchange::<LiveSubscribeTrack>(
            LiveSubscribe {
                track: live_track("doc"),
                from: LiveStart::Latest,
            },
            LiveItem::GroupEnd {
                group: 4,
                complete: false,
            },
        ),
        exchange::<LiveTracks>("lobby".into(), Watching::Waiting),
        exchange::<LiveTracks>(
            "lobby".into(),
            Watching::Announce(LiveAnnounce {
                name: "presence/3".into(),
                by: 3,
                mode: LiveMode::Datagram,
                up: true,
            }),
        ),
        exchange::<Watch<Golden>>((7, "q".into()), Some(vec![1, 2, 3])),
        exchange::<Watch<Golden>>((7, "q".into()), None),
        exchange::<methods::ChainFacts>(
            methods::FactsAsk {
                before: Some(10),
                limit: 2,
            },
            methods::FactsPage {
                blocks: vec![methods::Facts {
                    height: 9,
                    id: [4; 32],
                    parent: [5; 32],
                    time: 1,
                    epoch: 0,
                    proposer: Some(vec![6; 32]),
                    txs: 3,
                }],
                listed_from: Some(0),
            },
        ),
        exchange::<methods::ChainTxs>(
            methods::TxRange {
                before: Some(methods::TxPlace {
                    height: 9,
                    position: 2,
                }),
                oldest: 0,
                target: Some("chat".into()),
                limit: 1,
            },
            methods::TxPage {
                txs: vec![methods::TxSummary {
                    height: 9,
                    position: 1,
                    time: 1,
                    hash: [7; 32],
                    signer: vec![8; 32],
                    anchor: methods::Tip {
                        height: 8,
                        id: [3; 32],
                    },
                    target: "chat".into(),
                    size: 1,
                    applied: Some(false),
                }],
                next: Some(methods::TxPlace {
                    height: 9,
                    position: 1,
                }),
            },
        ),
        exchange::<methods::ChainTxs>(
            methods::TxRange {
                before: None,
                oldest: 9,
                target: None,
                limit: 1,
            },
            methods::TxPage {
                txs: vec![methods::TxSummary {
                    height: 9,
                    position: 0,
                    time: 1,
                    hash: [7; 32],
                    signer: vec![8; 32],
                    anchor: methods::Tip {
                        height: 8,
                        id: [3; 32],
                    },
                    target: "chat".into(),
                    size: 1,
                    applied: None,
                }],
                next: None,
            },
        ),
    ]
    .into_iter()
    .chain(
        Tint::ALL
            .map(|tint| carried_view(tint, None))
            .into_iter()
            .chain([carried_view(Tint::Blue, Some(1_200))])
            .map(|view| exchange::<BlobManifest>("sha256:00".into(), Some(view))),
    )
    .collect()
}

fn carried_view(tint: Tint, width: Option<u32>) -> methods::Manifested {
    methods::Manifested {
        size: 2_048,
        view: Some(methods::CarriedView {
            size: 1_024,
            manifest: Some(Manifest {
                wire_id: "0123abcd".into(),
                name: "Chat".into(),
                description: "Rooms and threads.".into(),
                capabilities: methods::Capability::ALL.to_vec(),
                min_width: 480,
                targets: vec!["chat".into()],
                icon: "icons/chat.svg".into(),
                tint,
                width,
            }),
        }),
    }
}

fn telegram_socket() -> methods::SocketOpen {
    methods::SocketOpen {
        socket: 1,
        url: "wss://venus.web.telegram.org/apiws".into(),
        protocols: vec!["binary".into()],
    }
}

fn live_track(name: &str) -> methods::LiveTrack {
    methods::LiveTrack {
        room: "lobby".into(),
        name: name.into(),
    }
}

fn notification() -> methods::Notification {
    methods::Notification {
        title: "alice mentioned you".into(),
        body: "@bob hi".into(),
        tag: "room".into(),
        link: "duck://chat/room".into(),
        kind: methods::NoticeKind::Notice,
        until: 0,
        end_link: String::new(),
    }
}

type Methods = BTreeMap<String, Vec<(Vec<u8>, Vec<u8>)>>;

#[test]
fn every_method_carries_the_committed_bytes() {
    let (exchanges, json): (Vec<_>, Vec<_>) = every_method().into_iter().unzip();
    let mut built = Methods::new();
    for (kind, request, reply) in exchanges {
        built.entry(kind).or_default().push((request, reply));
    }
    let kinds: BTreeSet<&str> = built.keys().map(String::as_str).collect();
    let all: BTreeSet<&str> = methods::ALL.iter().copied().collect();
    assert_eq!(kinds, all, "an exchange per method in ALL");
    let bin = golden("methods.bin");
    if std::env::var_os("WIRE_GOLDEN_WRITE").is_some() {
        std::fs::write(&bin, methods::encode(&built)).unwrap();
        std::fs::write(
            golden("methods.json"),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
        return;
    }
    let committed: Methods = methods::decode(
        &std::fs::read(&bin)
            .unwrap_or_else(|error| panic!("{}: {error}; {MESSAGE}", bin.display())),
    )
    .unwrap();
    assert_eq!(moved(&committed, &built), Vec::<String>::new(), "{MESSAGE}");
}

fn moved(committed: &Methods, built: &Methods) -> Vec<String> {
    committed
        .iter()
        .filter(|(kind, bytes)| built.get(*kind) != Some(bytes))
        .map(|(kind, _)| kind.clone())
        .collect()
}

#[test]
fn a_new_method_passes_and_a_moved_or_dropped_one_fails() {
    let method = |kind: &str, byte: u8| (kind.to_string(), vec![(vec![byte], vec![])]);
    let committed: Methods = [method("a.one", 1), method("a.two", 2)].into();
    let grown: Methods = [method("a.one", 1), method("a.two", 2), method("a.new", 3)].into();
    assert!(moved(&committed, &grown).is_empty());
    let changed: Methods = [method("a.one", 9), method("a.two", 2)].into();
    assert_eq!(moved(&committed, &changed), ["a.one"]);
    let dropped: Methods = [method("a.one", 1)].into();
    assert_eq!(moved(&committed, &dropped), ["a.two"]);
}
