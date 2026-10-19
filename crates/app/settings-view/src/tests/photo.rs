//! Change photo: the picked picture read in the view and opened in the crop
//! sheet, cut and saved as the avatar of the reader's account or of an agent
//! they manage, its bio kept; Remove photo; a file that is no picture.
use super::*;
use image::{DynamicImage, ImageFormat as Format, Rgba, RgbaImage};
use view::methods::{
    BlobBody, BlobGet, FilePick, FileRead, FileRelease, OpPut, Picked, Putting, ReadFile,
};

const HELD: [u8; 32] = [9; 32];
const BIO: &str = "Ducks, mostly.";
const SCOUT_BIO: &str = "Looks ahead.";

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, y| Rgba([x as u8, y as u8, 128, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(picture)
        .write_to(&mut bytes, Format::Png)
        .unwrap();
    bytes.into_inner()
}

fn held() -> Option<abi::BlobId> {
    Some(abi::BlobId::Sha256(HELD))
}

// Scout with a bio and `avatar`, in `life`.
fn scouting(avatar: Option<abi::BlobId>, life: identity::Life) -> identity::Account {
    let mut scout = scout();
    scout.card.avatar = avatar;
    scout.card.bio = Some(SCOUT_BIO.into());
    if let identity::Control::Program { life: now, .. } = &mut scout.control {
        *now = life;
    }
    scout
}

fn active() -> identity::Life {
    identity::Life::Active { keys: Vec::new() }
}

// Maya's card from here on, with a bio and `avatar`, read afresh.
fn card(cx: &mut TestAppContext, props: &StreamSender<HostSession>, avatar: Option<abi::BlobId>) {
    cards(cx, props, avatar, scout());
}

// Maya's card with a bio and `avatar`, and `agent` the one she manages.
fn cards(
    cx: &mut TestAppContext,
    props: &StreamSender<HostSession>,
    avatar: Option<abi::BlobId>,
    agent: identity::Account,
) {
    cx.host().handle::<Query<Identity>>(move |q| {
        Ok(match q {
            identity::Query::Get { number: 7 } => {
                let mut maya = maya(7, "Laptop key");
                maya.card.avatar = avatar;
                maya.card.bio = Some(BIO.into());
                identity::Reply::Account(Some(maya))
            }
            identity::Query::Managed { by: 7, .. } => {
                identity::Reply::Accounts(identity::PageResponse {
                    height: 42,
                    items: vec![agent.clone()],
                    next: None,
                })
            }
            identity::Query::Profile { number: 9 } => {
                identity::Reply::Profile(Some(policies::ada()))
            }
            q => panic!("unexpected query: {q:?}"),
        })
    });
    props.send(Session {
        signer: "abcd".into(),
        account: Some(7),
        endpoint: "http://127.0.0.1:19001".into(),
        ..Session::default()
    });
    cx.run_until_parked();
}

// The device holds `bytes` as file 3, and the reader picks it with `press`.
fn picks(cx: &mut TestAppContext, bytes: Vec<u8>, press: &str) {
    let size = bytes.len() as u64;
    cx.host().handle::<FilePick>(move |pick| {
        assert!(!pick.multiple, "one picture");
        Ok(vec![Picked {
            name: "duck.png".into(),
            size,
            file: 3,
        }])
    });
    cx.host().handle::<FileRead>(move |ask| {
        let whole = ReadFile {
            file: 3,
            offset: 0,
            len: size,
        };
        assert_eq!(ask, whole);
        Ok(bytes.clone())
    });
    cx.host().handle::<FileRelease>(|_| Ok(()));
    cx.simulate_click(press);
    cx.run_until_parked();
}

// The one blob the next `op.put` holds, a 256 px JPEG.
fn holds_the_square(cx: &TestAppContext) {
    cx.host().ends::<OpPut>(|put| {
        let [blob] = &put.blobs[..] else {
            panic!("one blob: {put:?}")
        };
        assert_eq!(blob.kind, "avatar");
        let BlobBody::Bytes(jpeg) = &blob.body else {
            panic!("the picture's bytes")
        };
        let saved = image::load_from_memory_with_format(jpeg, Format::Jpeg).unwrap();
        assert_eq!((saved.width(), saved.height()), (256, 256));
        Ok(Putting::Held(vec![HELD]))
    });
}

// identity takes the next `SetProfile` only as `set`.
fn sets(cx: &TestAppContext, set: identity::Op) {
    cx.host().handle::<Submit<Identity>>(move |op| {
        assert_eq!(op, set);
        Ok(view::testing::applied(Vec::new()))
    });
}

#[test]
fn a_picked_picture_is_cut_and_saved_as_the_avatar_with_the_bio_kept() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    assert!(cx.find("settings/photo/remove").is_none(), "no picture yet");
    picks(&mut cx, png(600, 400), "settings/photo/change");
    assert!(cx.find("settings/photo").is_some(), "the crop sheet opens");
    assert_eq!(cx.host().requests::<FileRelease>(), [vec![3]]);
    // a 600 by 400 picture zooms until its square is 256 across
    let zoom = cx.interactivity("settings/photo/zoom");
    assert_eq!(zoom.role, Some(view::Role::Splitter));
    assert_eq!(zoom.aria.numeric_value, Some(1.));
    assert_eq!(zoom.aria.max_numeric_value, Some(400. / 256.));
    cx.simulate_drag("settings/photo/zoom", 1000., 0.);
    let zoomed = cx.interactivity("settings/photo/zoom").aria.numeric_value;
    assert_eq!(zoomed, Some(400. / 256.), "the thumb stops at the end");
    cx.simulate_focus("settings/photo/zoom");
    cx.simulate_key_down("settings/photo/zoom", "left");
    let back = cx.interactivity("settings/photo/zoom").aria.numeric_value;
    assert!(back < zoomed, "an arrow zooms out: {back:?}");
    holds_the_square(&cx);
    let set = identity::Op::SetProfile {
        account: 7,
        avatar: held(),
        bio: Some(BIO.into()),
    };
    sets(&cx, set);
    cx.simulate_click("settings/photo/save");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
    assert!(
        cx.find("settings/photo").is_none(),
        "saved, the sheet closes"
    );
}

#[test]
fn remove_photo_takes_the_picture_off_and_keeps_the_bio() {
    let (mut cx, props) = seated("ready", false);
    cx.host().handle::<BlobGet>(|_| Ok(Some(png(8, 8))));
    card(&mut cx, &props, held());
    let read = format!("sha256:{}", "09".repeat(32));
    assert_eq!(cx.host().requests::<BlobGet>(), [read], "the picture, once");
    let set = identity::Op::SetProfile {
        account: 7,
        avatar: None,
        bio: Some(BIO.into()),
    };
    sets(&cx, set);
    cx.simulate_click("settings/photo/remove");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
}

#[test]
fn a_manager_saves_an_agents_picture_with_the_agents_bio_kept() {
    let (mut cx, props) = seated("ready", false);
    cards(&mut cx, &props, None, scouting(None, active()));
    cx.simulate_click("settings/nav/agents");
    assert!(cx.find("settings/agents/12/photo/remove").is_none());
    picks(&mut cx, png(300, 300), "settings/agents/12/photo/change");
    assert!(cx.find("settings/photo").is_some(), "the crop sheet opens");
    holds_the_square(&cx);
    let set = identity::Op::SetProfile {
        account: 12,
        avatar: held(),
        bio: Some(SCOUT_BIO.into()),
    };
    sets(&cx, set);
    cx.simulate_click("settings/photo/save");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
    assert!(cx.find("settings/photo").is_none());
}

#[test]
fn an_agents_refused_removal_says_why_beside_the_agent_alone() {
    let (mut cx, props) = seated("ready", false);
    cx.host().handle::<BlobGet>(|_| Ok(Some(png(8, 8))));
    cards(&mut cx, &props, held(), scouting(held(), active()));
    cx.host()
        .refuse::<Submit<Identity>>("unauthorized", "account 7 does not manage account 12");
    cx.simulate_click("settings/nav/agents");
    cx.simulate_click("settings/agents/12/photo/remove");
    cx.run_until_parked();
    let said = "That didn’t go through: account 7 does not manage account 12";
    assert!(cx.has_text(said), "{:?}", cx.texts());
    assert!(cx.find("settings/agents/12/photo/refused").is_some());
    cx.simulate_click("settings/nav/account");
    assert!(cx.find("settings/photo/remove").is_some());
    assert!(!cx.has_text(said), "Maya's own presses say nothing");
}

#[test]
fn a_revoked_agent_offers_no_photo() {
    let (mut cx, props) = seated("ready", false);
    cx.host().handle::<BlobGet>(|_| Ok(Some(png(8, 8))));
    cards(
        &mut cx,
        &props,
        None,
        scouting(held(), identity::Life::Revoked),
    );
    cx.simulate_click("settings/nav/agents");
    assert!(cx.find("settings/agents/12/label").is_some());
    for press in ["change", "remove"] {
        let id = format!("settings/agents/12/photo/{press}");
        assert!(cx.find(&id).is_none(), "{press}");
    }
}

#[test]
fn on_a_phone_an_agents_sheet_changes_and_removes_its_picture() {
    let (mut cx, props) = seated("ready", false);
    cx.host().handle::<BlobGet>(|_| Ok(Some(png(8, 8))));
    cards(&mut cx, &props, None, scouting(held(), active()));
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.simulate_click("settings/strip/agents");
    cx.simulate_click("settings/agents/12");
    let read = format!("sha256:{}", "09".repeat(32));
    assert_eq!(
        cx.host().requests::<BlobGet>(),
        [read],
        "row and sheet, once"
    );
    assert!(cx.has_text("Change photo…") && cx.has_text("Remove photo"));
    let set = identity::Op::SetProfile {
        account: 12,
        avatar: None,
        bio: Some(SCOUT_BIO.into()),
    };
    sets(&cx, set);
    cx.simulate_click("settings/agents/12/photo/remove");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
    picks(&mut cx, png(300, 300), "settings/agents/12/photo/change");
    assert!(
        cx.find("settings/photo").is_some(),
        "the crop sheet over it"
    );
    cx.simulate_click("settings/photo/cancel");
    cx.run_until_parked();
    assert!(cx.find("settings/photo").is_none());
    assert!(
        cx.find("settings/agents/12/sheet").is_some(),
        "back to its sheet"
    );
}

#[test]
fn a_file_that_is_no_picture_opens_nothing_and_says_so() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    picks(&mut cx, b"not a picture".to_vec(), "settings/photo/change");
    assert!(cx.find("settings/photo").is_none());
    assert!(cx.has_text("That file isn’t a picture this app can open."));
}

#[test]
fn cancel_closes_the_sheet_and_puts_nothing() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    picks(&mut cx, png(200, 200), "settings/photo/change");
    assert!(
        cx.find("settings/photo/zoom").is_none(),
        "under 256: no zoom"
    );
    cx.simulate_click("settings/photo/cancel");
    cx.run_until_parked();
    assert!(cx.find("settings/photo").is_none());
    assert!(cx.host().requests::<OpPut>().is_empty());
}

#[test]
fn a_host_that_reads_no_picked_file_offers_no_change_photo() {
    let cx = fixture("no-file-read", false);
    assert!(cx.has_text("Maya"));
    assert!(cx.find("settings/photo/change").is_none());
    assert!(
        fixture("ready", false)
            .find("settings/photo/change")
            .is_some()
    );
}
