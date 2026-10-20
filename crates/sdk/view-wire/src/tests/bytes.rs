use super::*;

fn mixed() -> Vec<u8> {
    (0..50_000u32).map(|at| (at * 31 % 256) as u8).collect()
}

fn crosses_as_bin<T>(value: T, payload: &[u8])
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let wire = encode(&value);
    let header: &[u8] = match payload.len() {
        len @ 0..=0xff => &[0xc4, len as u8],
        len => &[0xc5, (len >> 8) as u8, len as u8],
    };
    let at = memchr::memmem::find(&wire, payload).expect("the bytes cross as they are");
    assert_eq!(&wire[at - header.len()..at], header);
    assert!(
        wire.len() <= payload.len() + 64,
        "{} bytes cross as {}",
        payload.len(),
        wire.len()
    );
    assert_eq!(decode::<T>(&wire).unwrap(), value);
}

#[test]
fn a_reply_crosses_as_bin() {
    let reply = Event::Response {
        id: 1,
        result: Ok(mixed()),
        done: true,
    };
    crosses_as_bin(reply, &mixed());
}

#[test]
fn a_request_payload_crosses_as_bin() {
    let request = || Request {
        id: 1,
        kind: "program.query".into(),
        payload: mixed(),
    };
    crosses_as_bin(request(), &mixed());
    crosses_as_bin(
        Frame {
            requests: vec![request()],
            ..Default::default()
        },
        &mixed(),
    );
    crosses_as_bin(
        methods::Call {
            target: "chat".into(),
            body: mixed(),
        },
        &mixed(),
    );
}

#[test]
fn a_picture_crosses_as_bin() {
    crosses_as_bin(ImageData::Encoded(mixed()), &mixed());
    crosses_as_bin(
        ImageData::Rgba {
            width: 125,
            height: 100,
            pixels: mixed(),
        },
        &mixed(),
    );
}

#[test]
fn an_svg_crosses_as_bin() {
    crosses_as_bin(picture(Some(mixed())), &mixed());
    let held = SvgSource::Data {
        hash: 7,
        bytes: None,
    };
    assert_eq!(decode::<SvgSource>(&encode(&held)).unwrap(), held);
}

#[test]
fn an_element_id_crosses_as_bin() {
    crosses_as_bin(ElementIdWire::Path(mixed()), &mixed());
    crosses_as_bin(ElementIdAtom::Path(mixed()), &mixed());
    crosses_as_bin(ElementIdWire::Uuid([0xc8; 16]), &[0xc8; 16]);
    crosses_as_bin(ElementIdAtom::Uuid([0xc8; 16]), &[0xc8; 16]);
    crosses_as_bin(ElementIdWire::OpaqueId([0xc8; 20]), &[0xc8; 20]);
    crosses_as_bin(
        ElementIdWire::NamedChild {
            base: ElementIdAtom::OpaqueId([0xc8; 20]),
            names: vec!["row".into()],
        },
        &[0xc8; 20],
    );
    let error = decode::<ElementIdWire>(&[0x81, 0x03, 0xc4, 0x02, 9, 9]).unwrap_err();
    assert!(error.contains("invalid length 2"), "{error}");
    let mut long = vec![0x81, 0x03, 0xc4, 17];
    long.extend([9; 17]);
    let error = decode::<ElementIdWire>(&long).unwrap_err();
    assert!(error.contains("a Uuid is 16 bytes"), "{error}");
}

#[test]
fn bytes_read_back_from_json() {
    fn json<T>(value: T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let text = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<T>(&text).unwrap(), value, "{text}");
    }
    json(Frame {
        root: Some(picture(Some(vec![1, 2, 200]))),
        requests: vec![Request {
            id: 1,
            kind: "program.query".into(),
            payload: vec![1, 2, 200],
        }],
        ..Default::default()
    });
    json(Event::Response {
        id: 1,
        result: Ok(vec![1, 2, 200]),
        done: true,
    });
    json(ImageData::Encoded(vec![1, 2, 200]));
    json(ElementIdWire::Path(b"a/b".to_vec()));
    json(ElementIdWire::Uuid([0xc8; 16]));
    json(ElementIdWire::NamedChild {
        base: ElementIdAtom::OpaqueId([0xc8; 20]),
        names: vec!["row".into()],
    });
    let error = serde_json::from_str::<ElementIdWire>(r#"{"Uuid":[1,2]}"#).unwrap_err();
    assert!(error.to_string().contains("invalid length 2"), "{error}");
    let long = format!(r#"{{"Uuid":{:?}}}"#, [1; 17]);
    let error = serde_json::from_str::<ElementIdWire>(&long).unwrap_err();
    assert!(error.to_string().contains("a Uuid is 16 bytes"), "{error}");
}

#[test]
fn an_array_of_integers_is_not_bytes() {
    let error = decode::<ImageData>(&[0x81, 0x00, 0x92, 0x01, 0x02]).unwrap_err();
    assert!(error.contains("expected bytes"), "{error}");
}

#[test]
fn a_bin_longer_than_its_bytes_is_refused() {
    let start = std::time::Instant::now();
    assert!(decode::<ImageData>(&[0x81, 0x00, 0xc6, 0xff, 0xff, 0xff, 0xff]).is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn bytes_after_the_value_are_refused() {
    let mut wire = encode(&Event::Resync);
    assert_eq!(decode::<Event>(&wire).unwrap(), Event::Resync);
    wire.push(0xc0);
    assert_eq!(
        decode::<Event>(&wire).unwrap_err(),
        "trailing MessagePack bytes"
    );
}

#[test]
fn a_text_transform_reads_absent_from_an_older_text_and_refused_unknown() {
    #[derive(serde::Serialize)]
    struct Before {
        id: Option<ElementIdWire>,
        style: StyleId,
        content: String,
    }
    #[derive(serde::Serialize)]
    enum Later {
        Uppercase,
        Sideways,
    }
    #[derive(serde::Serialize)]
    struct After {
        id: Option<ElementIdWire>,
        style: StyleId,
        content: String,
        transform: Option<Later>,
    }
    let words = "Confirm a change";
    let plain = TextNode {
        id: None,
        style: StyleId(0),
        content: words.into(),
        transform: None,
    };
    let before = Before {
        id: None,
        style: StyleId(0),
        content: words.into(),
    };
    assert_eq!(decode::<TextNode>(&encode(&before)), Ok(plain.clone()));
    let drawn = TextNode {
        transform: Some(TextTransform::Uppercase),
        ..plain
    };
    assert_eq!(decode::<TextNode>(&encode(&drawn)), Ok(drawn.clone()));
    let later = |transform| After {
        id: None,
        style: StyleId(0),
        content: words.into(),
        transform: Some(transform),
    };
    assert_eq!(
        decode::<TextNode>(&encode(&later(Later::Uppercase))),
        Ok(drawn)
    );
    assert!(decode::<TextNode>(&encode(&later(Later::Sideways))).is_err());
}
