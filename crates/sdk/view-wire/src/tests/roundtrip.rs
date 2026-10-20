use super::*;

#[test]
fn a_cut_child_is_reported_with_where_it_fell() {
    let mut frame = Frame {
        root: Some(column(vec![
            text("complete"),
            Node::Overlay {
                id: ElementIdWire::Name("App/o".into()),
                label: None,
                style: StyleId(0),
                on_dismiss: None,
                children: vec![text("a"), text("b"), text("c")],
            },
        ])),
        ..Default::default()
    };
    let report = sanitize_plain(&mut frame).unwrap();
    assert_eq!(
        report,
        SanitizeReport {
            nodes: 1,
            first: Some(vec![1]),
            ..Default::default()
        }
    );
    let mut received: Frame = decode(&encode(&frame)).unwrap();
    assert!(sanitize_plain(&mut received).unwrap().is_empty());
    let mut small = Frame {
        root: Some(column(vec![text("complete")])),
        ..Default::default()
    };
    let before = small.clone();
    assert!(sanitize_plain(&mut small).unwrap().is_empty());
    assert_eq!(small, before);
}

#[test]
fn a_kept_number_is_not_a_cut() {
    let rounded = gpui::Styled::rounded_full(gpui::StyleRefinement::default());
    let mut clamped = Frame {
        styles: vec![Style::new(&gpui::Styled::rounded_full(
            gpui::StyleRefinement::default(),
        ))],
        root: Some(column(Vec::new())),
        ..Default::default()
    };
    let mut styles = Styles::default();
    assert!(sanitize(&mut clamped, &mut styles).unwrap().is_empty());
    assert_eq!(
        styles[StyleId(0)].corner_radii.top_left,
        rounded.corner_radii.top_left
    );
}

#[test]
fn a_frame_round_trips_at_every_header_width() {
    for count in [0, 1, 16, 256, 2000, 70_000] {
        let frame = Frame {
            styles: plain(),
            root: Some(column(
                (0..count)
                    .map(|index| keyed(&index.to_string(), "한é"))
                    .collect(),
            )),
            ..Default::default()
        };
        assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    }
}

#[test]
fn a_frame_round_trips() {
    let frame = Frame {
        tooltip_responses: Vec::new(),
        styles: plain(),
        root: Some(column(vec![text("hello"), {
            let mut input = field("App/i", "x", "Name");
            let Node::Field {
                tokens, on_submit, ..
            } = &mut input
            else {
                unreachable!()
            };
            *tokens = Box::new([TextToken {
                range: TextRange::from(0..1),
                id: "<@1>".into(),
            }]);
            *on_submit = Some(4);
            input
        }])),
        patches: vec![Patch::Remove {
            path: vec![0, 1],
            index: 2,
        }],
        requests: vec![Request {
            id: 1,
            kind: "host.echo".into(),
            payload: b"hi".to_vec(),
        }],
        cancels: vec![2],
        unchanged: false,
        busy: false,
    };
    assert_eq!(decode::<Frame>(&encode(&frame)).unwrap(), frame);
    let events = vec![
        Event::Message(3),
        Event::Text {
            handler: 0,
            change: TextChange {
                generation: 1,
                revision: 3,
                edit: Some(Edit {
                    range: TextRange::from(1..2),
                    len: 1,
                }),
                text: "xy".into(),
                cursor: TextRange::caret(2),
                preedit: Some(TextRange::from(1..2)),
                tokens: Default::default(),
            },
        },
        Event::Select {
            handler: 3,
            index: 1,
        },
        Event::ScrollOffset {
            handler: 6,
            x: 24.0,
            y: 50.0,
            relative_x: 0.2,
            relative_y: 0.1,
        },
        Event::Response {
            id: 1,
            result: Err(Error::new("program", "nope")),
            done: true,
        },
        Event::Resync,
    ];
    assert_eq!(decode::<Vec<Event>>(&encode(&events)).unwrap(), events);
}
