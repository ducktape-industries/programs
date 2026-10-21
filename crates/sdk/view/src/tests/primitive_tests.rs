//! How primitive elements lower onto the wire, frame after frame.
use super::*;

#[test]
fn a_blob_picture_crosses_as_its_ids_and_the_host_keeps_only_sha256_ones() {
    #[derive(Default, Serialize, Deserialize)]
    struct Blobbed;
    impl View for Blobbed {
        const NAME: &'static str = "Blobbed";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }
    impl Render for Blobbed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("blobbed")
                .child(blob_img(vec![format!("sha256:{}", "ab".repeat(32))]))
                .child(blob_img(vec!["sha256:not-hex".into()]))
        }
    }
    let mut driver = Driver::<Blobbed>::new();
    let first = driver.tick_with(vec![], wire::Frame::clone);
    let mut taken = wire::Frame {
        root: first.root.clone(),
        styles: first.styles.clone(),
        ..Default::default()
    };
    wire::sanitize(&mut taken, &mut wire::Styles::default()).expect("the host takes the frame");
    let mut sent = Vec::new();
    taken.root.expect("a whole tree").for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            sent.push(data.clone());
        }
    });
    assert_eq!(
        sent,
        [
            Some(wire::ImageData::Blobs(vec![format!(
                "sha256:{}",
                "ab".repeat(32)
            )])),
            None,
        ]
    );
}

#[test]
fn patches_reconstruct_the_rendered_tree_and_picture_bytes_are_not_retained() {
    #[derive(Default, Serialize, Deserialize)]
    struct Picture(u32);
    impl View for Picture {
        const NAME: &'static str = "Picture";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(0)
        }
    }
    impl Render for Picture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("picture-view")
                .child(img(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    b"shared-image".to_vec(),
                ))))
                .children((0..20).map(|i| {
                    div()
                        .id(format!("row/{i}"))
                        .child(format!("Row {i}: {}", if i == 0 { self.0 } else { 0 }))
                }))
        }
    }
    let mut driver = Driver::<Picture>::new();
    let first = driver.tick_with(vec![], wire::Frame::clone);
    let mut styles = wire::Styles::default();
    styles.extend(first.styles).unwrap();
    let mut mounted = first.root.unwrap();
    let mut pictures = 0;
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            assert!(data.is_some());
            *data = None;
            pictures += 1;
        }
    });
    assert_eq!(pictures, 1);
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));
    assert!(driver.tick_with(vec![], wire::Frame::clone).unchanged);
    driver.entity().update(driver.app_mut(), |view, cx| {
        view.0 = 1;
        cx.notify();
    });
    let frame = driver.tick_with(vec![], wire::Frame::clone);
    assert!(!frame.unchanged);
    assert!(!frame.patches.is_empty());
    styles.extend(frame.styles).unwrap();
    wire::apply(&mut mounted, frame.patches, &styles).unwrap();
    mounted.for_each_mut(&mut |node| {
        if let wire::Node::Image { data, .. } = node {
            *data = None;
        }
    });
    assert_eq!(driver.last_root.as_ref(), Some(&mounted));

    let resent = driver.tick_with(vec![wire::Event::Resync], wire::Frame::clone);
    assert!(
        resent.root.as_ref().is_some_and(|root| {
            let mut found = false;
            root.clone().for_each_mut(&mut |node| {
                if matches!(
                    node,
                    wire::Node::Image {
                        data: Some(wire::ImageData::Encoded(_)),
                        ..
                    }
                ) {
                    found = true;
                }
            });
            found
        }),
        "resync must resend bytes from a dropped first frame"
    );
}

#[test]
fn a_page_switch_sends_the_bytes_a_copying_diff_sends_and_keeps_the_tree_whole() {
    #[derive(Default, Serialize, Deserialize)]
    struct Pages {
        rows: bool,
        chrome: bool,
    }
    impl View for Pages {
        const NAME: &'static str = "Pages";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                rows: false,
                chrome: true,
            }
        }
    }
    impl Render for Pages {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let page = match self.rows {
                false => div().id("overview").child("nothing here"),
                true => div().id("rows").children((0..40).map(|row| {
                    div()
                        .id(format!("row/{row}"))
                        .child(format!("row {row}"))
                        .child("…")
                })),
            };
            match self.chrome {
                true => div()
                    .id("shell")
                    .child(div().id("title").child("title"))
                    .child(page)
                    .child("footer"),
                false => div().id("shell").child(page),
            }
        }
    }
    let mut whole = Driver::<Pages>::new();
    whole.tick_with(vec![], wire::Frame::clone);
    whole.entity().update(whole.app_mut(), |view, cx| {
        view.chrome = false;
        cx.notify();
    });
    whole.tick_with(vec![], wire::Frame::clone);
    let mut rendered = Driver::<Pages>::new();
    rendered.entity().update(rendered.app_mut(), |view, cx| {
        *view = Pages {
            rows: true,
            chrome: false,
        };
        cx.notify();
    });
    let rendered = rendered
        .tick_with(vec![], wire::Frame::clone)
        .root
        .expect("the rows tree");
    whole.entity().update(whole.app_mut(), |view, cx| {
        view.rows = true;
        cx.notify();
    });
    let frame = whole.tick_with(vec![], wire::Frame::clone);
    assert!(frame.patches.is_empty(), "{:#?}", frame.patches);
    assert_eq!(frame.root.as_ref(), Some(&rendered));
    assert_eq!(whole.last_root.as_ref(), Some(&rendered));

    let show = |driver: &mut Driver<Pages>, rows: bool| {
        driver.entity().update(driver.app_mut(), |view, cx| {
            view.rows = rows;
            cx.notify();
        });
    };
    let mut driver = Driver::<Pages>::new();
    let first = driver.tick_with(vec![], wire::Frame::clone);
    let mut styles = wire::Styles::default();
    styles.extend(first.styles).unwrap();
    let mut held = first.root.expect("a first tree");
    let overview = held.clone();
    let mut reference = Driver::<Pages>::new();
    reference.tick_with(vec![], wire::Frame::clone);
    show(&mut reference, true);
    let brought = reference.tick_with(vec![], wire::Frame::clone).styles;
    let rows = reference.last_root.clone().expect("the rows tree");
    let expected = wire::Frame {
        patches: wire::diff(&mut overview.clone(), &mut rows.clone()),
        styles: brought,
        ..wire::Frame::default()
    };
    assert!(
        expected.patches.iter().any(|patch| matches!(
            patch,
            wire::Patch::Insert { .. } | wire::Patch::Replace { .. }
        )),
        "{:#?}",
        expected.patches
    );

    show(&mut driver, true);
    let sent = driver.tick_with(vec![], wire::encode);
    assert_eq!(sent, wire::encode(&expected), "the bytes the host gets");
    styles.extend(expected.styles).unwrap();
    wire::apply(&mut held, expected.patches, &styles).unwrap();
    assert_eq!(held, rows);
    assert_eq!(
        driver.last_root.as_ref(),
        Some(&rows),
        "the kept tree is whole again"
    );
    assert!(driver.tick_with(vec![], wire::Frame::clone).unchanged);

    show(&mut driver, false);
    let back = driver.tick_with(vec![], wire::Frame::clone);
    assert!(!back.unchanged && !back.patches.is_empty());
    styles.extend(back.styles).unwrap();
    wire::apply(&mut held, back.patches, &styles).unwrap();
    assert_eq!(
        held, overview,
        "the frame after a switch still patches to the rendered tree"
    );
}

#[test]
fn primitive_sources_fallbacks_transformations_and_typed_ids_survive_lowering() {
    #[derive(Default, Serialize, Deserialize)]
    struct Primitives;
    impl View for Primitives {
        const NAME: &'static str = "Primitives";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self
        }
    }
    impl Render for Primitives {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let empty = gpui::RenderImage::new(Vec::new());
            div().children([
                img(Arc::new(empty))
                    .with_fallback(|| "fallback".into_any_element())
                    .id(gpui::ElementId::Integer(9))
                    .into_any_element(),
                svg()
                    .data(b"<svg/>")
                    .with_transformation(Transformation::translate(gpui::point(px(3.), px(4.))))
                    .id(gpui::ElementId::Integer(10))
                    .into_any_element(),
            ])
        }
    }

    let mut cx = crate::testing::TestAppContext::new();
    cx.open::<Primitives>();
    let children = cx.root().children().to_vec();
    let wire::Node::Image {
        id,
        data,
        fallback,
        state_children,
        ..
    } = &children[0]
    else {
        panic!()
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Integer(9)));
    assert!(matches!(data, Some(wire::ImageData::Refusal(reason)) if reason.contains("no frames")));
    assert!(*fallback);
    assert!(
        matches!(&state_children[0], wire::Node::Text (crate::wire::TextNode { content, .. }) if content == "fallback")
    );
    let wire::Node::Svg {
        id,
        source,
        transformation,
        ..
    } = &children[1]
    else {
        panic!()
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Integer(10)));
    assert!(
        matches!(source, wire::SvgSource::Data { bytes: Some(bytes), .. } if bytes == b"<svg/>")
    );
    assert_eq!(transformation.translate, [3., 4.]);
}
