//! Change photo: the picked picture read in the view and opened in the crop
//! sheet, cut and saved as the account's avatar with its bio kept; Remove
//! photo; a file that is no picture; a host that reads no picked file.
use super::*;
use image::{DynamicImage, ImageFormat as Format, Rgba, RgbaImage};
use view::methods::{
    BlobBody, BlobGet, FilePick, FileRead, FileRelease, OpPut, Picked, Putting, ReadFile,
};

const HELD: [u8; 32] = [9; 32];
const BIO: &str = "Ducks, mostly.";

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = RgbaImage::from_fn(width, height, |x, y| Rgba([x as u8, y as u8, 128, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(picture)
        .write_to(&mut bytes, Format::Png)
        .unwrap();
    bytes.into_inner()
}

// Maya's card from here on, with a bio and `avatar`, read afresh.
fn card(cx: &mut TestAppContext, props: &StreamSender<HostSession>, avatar: Option<abi::BlobId>) {
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
                    items: vec![scout()],
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

// The device holds `bytes` as file 3, and the reader picks it.
fn picks(cx: &mut TestAppContext, bytes: Vec<u8>) {
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
    cx.simulate_click("settings/photo/change");
    cx.run_until_parked();
}

#[test]
fn a_picked_picture_is_cut_and_saved_as_the_avatar_with_the_bio_kept() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    assert!(cx.find("settings/photo/remove").is_none(), "no picture yet");
    picks(&mut cx, png(600, 400));
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
    cx.host().handle::<Submit<Identity>>(|op| {
        let set = identity::Op::SetProfile {
            account: 7,
            avatar: Some(abi::BlobId::Sha256(HELD)),
            bio: Some(BIO.into()),
        };
        assert_eq!(op, set);
        Ok(view::testing::applied(Vec::new()))
    });
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
    card(&mut cx, &props, Some(abi::BlobId::Sha256(HELD)));
    let read = format!("sha256:{}", "09".repeat(32));
    assert_eq!(cx.host().requests::<BlobGet>(), [read], "the picture, once");
    cx.host().handle::<Submit<Identity>>(|op| {
        let set = identity::Op::SetProfile {
            account: 7,
            avatar: None,
            bio: Some(BIO.into()),
        };
        assert_eq!(op, set);
        Ok(view::testing::applied(Vec::new()))
    });
    cx.simulate_click("settings/photo/remove");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
}

#[test]
fn a_file_that_is_no_picture_opens_nothing_and_says_so() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    picks(&mut cx, b"not a picture".to_vec());
    assert!(cx.find("settings/photo").is_none());
    assert!(cx.has_text("That file isn’t a picture this app can open."));
}

#[test]
fn cancel_closes_the_sheet_and_puts_nothing() {
    let (mut cx, props) = seated("ready", false);
    card(&mut cx, &props, None);
    picks(&mut cx, png(200, 200));
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
