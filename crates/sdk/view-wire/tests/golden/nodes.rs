//! Every `Node` variant at least once, in one tree.
use super::*;

/// Every `StyleRefinement`/`TextStyleRefinement` field set once, so each
/// field's bytes in a table entry are in `frame.bin` and its place in
/// `schema.txt`. gpui's `debug`/`debug_below` do not cross and are not set.
// One field at a time is far more readable here than one `StyleRefinement`
// literal with ~50 named fields, several nested (`overflow.x`, `text.color`).
#[allow(clippy::field_reassign_with_default)]
fn full_style() -> gpui::StyleRefinement {
    let mut style = gpui::StyleRefinement::default();
    style.display = Some(gpui::Display::Grid);
    style.visibility = Some(gpui::Visibility::Hidden);
    style.overflow.x = Some(gpui::Overflow::Scroll);
    style.overflow.y = Some(gpui::Overflow::Hidden);
    style.scrollbar_width = Some(gpui::px(4.0).into());
    style.allow_concurrent_scroll = Some(true);
    style.restrict_scroll_to_axis = Some(true);
    style.position = Some(gpui::Position::Absolute);
    style.inset.top = Some(gpui::px(1.0).into());
    style.inset.right = Some(gpui::px(1.0).into());
    style.inset.bottom = Some(gpui::px(1.0).into());
    style.inset.left = Some(gpui::px(1.0).into());
    style.size.width = Some(gpui::px(100.0).into());
    style.size.height = Some(gpui::px(50.0).into());
    style.min_size.width = Some(gpui::px(10.0).into());
    style.min_size.height = Some(gpui::px(10.0).into());
    style.max_size.width = Some(gpui::px(200.0).into());
    style.max_size.height = Some(gpui::px(200.0).into());
    style.aspect_ratio = Some(1.5);
    style.margin.top = Some(gpui::px(2.0).into());
    style.margin.right = Some(gpui::px(2.0).into());
    style.margin.bottom = Some(gpui::px(2.0).into());
    style.margin.left = Some(gpui::px(2.0).into());
    style.padding.top = Some(gpui::px(2.0).into());
    style.padding.right = Some(gpui::px(2.0).into());
    style.padding.bottom = Some(gpui::px(2.0).into());
    style.padding.left = Some(gpui::px(2.0).into());
    style.border_widths.top = Some(gpui::px(1.0).into());
    style.border_widths.right = Some(gpui::px(1.0).into());
    style.border_widths.bottom = Some(gpui::px(1.0).into());
    style.border_widths.left = Some(gpui::px(1.0).into());
    style.align_items = Some(gpui::AlignItems::Center);
    style.align_self = Some(gpui::AlignSelf::Center);
    style.align_content = Some(gpui::AlignContent::Center);
    style.justify_content = Some(gpui::JustifyContent::Center);
    style.gap.width = Some(gpui::px(2.0).into());
    style.gap.height = Some(gpui::px(2.0).into());
    style.flex_direction = Some(gpui::FlexDirection::Column);
    style.flex_wrap = Some(gpui::FlexWrap::Wrap);
    style.flex_basis = Some(gpui::px(10.0).into());
    style.flex_grow = Some(1.0);
    style.flex_shrink = Some(1.0);
    style.background = Some(gpui::Fill::Color(gpui::solid_background(gpui::red())));
    style.border_color = Some(gpui::blue());
    style.border_style = Some(gpui::BorderStyle::Dashed);
    style.corner_radii.top_left = Some(gpui::px(1.0).into());
    style.corner_radii.top_right = Some(gpui::px(1.0).into());
    style.corner_radii.bottom_left = Some(gpui::px(1.0).into());
    style.corner_radii.bottom_right = Some(gpui::px(1.0).into());
    style.box_shadow = Some(vec![gpui::BoxShadow {
        color: gpui::black(),
        offset: gpui::point(gpui::px(1.0), gpui::px(1.0)),
        blur_radius: gpui::px(2.0),
        spread_radius: gpui::px(0.0),
        inset: false,
    }]);
    style.mouse_cursor = Some(gpui::CursorStyle::PointingHand);
    style.opacity = Some(0.5);
    style.grid_cols = Some(gpui::GridTemplate {
        repeat: 2,
        min_size: gpui::GridTemplateMinSize::MinContent,
    });
    style.grid_rows = Some(gpui::GridTemplate {
        repeat: 1,
        min_size: gpui::GridTemplateMinSize::MaxContent,
    });
    style.grid_location = Some(gpui::GridLocation {
        row: gpui::GridPlacement::Line(1)..gpui::GridPlacement::Span(2),
        column: gpui::GridPlacement::Auto..gpui::GridPlacement::Auto,
    });
    style.text.color = Some(gpui::red());
    style.text.font_family = Some("Mono".into());
    style.text.font_features = Some(gpui::FontFeatures::default());
    style.text.font_fallbacks = Some(gpui::FontFallbacks::from_fonts(vec!["Sans".into()]));
    style.text.font_size = Some(gpui::px(14.0).into());
    style.text.line_height = Some(gpui::px(18.0).into());
    style.text.font_weight = Some(gpui::FontWeight::BOLD);
    style.text.font_style = Some(gpui::FontStyle::Italic);
    style.text.background_color = Some(gpui::black());
    style.text.underline = Some(gpui::UnderlineStyle {
        thickness: gpui::px(1.0),
        color: Some(gpui::red()),
        wavy: true,
    });
    style.text.strikethrough = Some(gpui::StrikethroughStyle {
        thickness: gpui::px(1.0),
        color: Some(gpui::red()),
    });
    style.text.white_space = Some(gpui::WhiteSpace::Nowrap);
    style.text.text_overflow = Some(gpui::TextOverflow::TruncateMiddle("...".into()));
    style.text.text_align = Some(gpui::TextAlign::Center);
    style.text.line_clamp = Some(3);
    style.text.letter_spacing = Some(gpui::px(-0.5));
    style
}

/// A style of one background alone: the gradient [`full_style`]'s solid
/// one leaves out.
fn backdrop(background: gpui::Background) -> StyleRefinement {
    StyleRefinement {
        background: Some(gpui::Fill::Color(background)),
        ..Default::default()
    }
}

/// A backdrop that also truncates: the `TextOverflow`s [`full_style`]'s
/// `TruncateMiddle` leaves out.
fn truncating(background: gpui::Background, overflow: gpui::TextOverflow) -> StyleRefinement {
    let mut style = backdrop(background);
    style.text.text_overflow = Some(overflow);
    style
}

/// The frame's style table. Its first entry is the empty style, which
/// every sample node names ([`style`]); the rest are the ones
/// [`full_interactivity`] names, [`full_style`] once and every other one
/// small.
pub fn every_style() -> Vec<StyleRefinement> {
    let stop = gpui::linear_color_stop(gpui::red(), 0.);
    vec![
        StyleRefinement::default(),
        full_style(),
        truncating(
            gpui::linear_gradient(90., stop, stop),
            gpui::TextOverflow::Truncate("…".into()),
        ),
        truncating(
            gpui::linear_gradient(180., stop, stop).color_space(gpui::ColorSpace::Oklab),
            gpui::TextOverflow::TruncateStart("…".into()),
        ),
        backdrop(gpui::solid_background(gpui::black())),
    ]
}

/// A non-default `Interactivity`: every field present, each conditional
/// style an entry of [`every_style`]. Reused wherever a node's own
/// `interactivity` only needs to be non-default, not this specific one.
fn full_interactivity() -> Interactivity {
    Interactivity {
        role: Some(gpui::Role::ListBox),
        aria: every_aria(),
        focusable: true,
        tab_stop: Some(true),
        tab_index: Some(1),
        tab_group: true,
        focus: Some(StyleId(1)),
        in_focus: Some(StyleId(2)),
        focus_visible: Some(StyleId(3)),
        focus_handle: Some(42),
        occlude: true,
        block_mouse_except_scroll: true,
        hover_listener_mode: interactivity::HoverListenerMode::InputModalityIndependent,
        group: Some("card".into()),
        hover: Some(StyleId(4)),
        active: Some(style()),
        group_hover: Some(GroupRefinement {
            group: "card".into(),
            style: style(),
        }),
        group_active: Some(GroupRefinement {
            group: "card".into(),
            style: style(),
        }),
        on_click: Some(60),
        on_aux_click: Some(61),
        consumes_click: true,
        on_mouse_down: Some(62),
        capture_mouse_down: Some(63),
        on_mouse_down_out: Some(64),
        on_mouse_up: Some(65),
        capture_mouse_up: Some(66),
        on_mouse_up_out: Some(67),
        on_mouse_pressure: Some(68),
        capture_mouse_pressure: Some(69),
        on_mouse_move: Some(70),
        on_mouse_exit: Some(71),
        on_scroll_wheel: Some(72),
        on_pinch: Some(73),
        capture_pinch: Some(74),
        on_key_down: Some(9),
        capture_key_down: Some(75),
        on_key_up: Some(76),
        capture_key_up: Some(77),
        on_modifiers_changed: Some(78),
        consumes_keys: vec!["escape".into(), "shift-tab".into()],
        on_hover: Some(79),
        on_file_drop_exit: Some(80),
        tooltip: Some(Tooltip {
            request: 50,
            hoverable: true,
            delay_ms: 300,
        }),
        on_back: Some(81),
        page: Some(2),
        on_file_drop: Some(82),
        file_drag_over: Some(style()),
    }
}

pub fn every_node() -> Node {
    let mut children = first_nodes();
    children.extend(full_nodes());
    Node::Container(ContainerNode {
        id: Some(id("root")),
        style: style(),
        interactivity: Default::default(),
        children,
    })
}

/// The nodes the fixture first pinned, one of each variant but the root's
/// `Container`, kept verbatim so every value they pin stays pinned: a
/// sample is added beside them, in [`full_nodes`], never swapped in.
fn first_nodes() -> Vec<Node> {
    vec![
        Node::RichText {
            id: Some(id("rich")),
            style: style(),
            text: "rich text".into(),
            runs: RichTextRuns::Highlights(vec![(
                0..4,
                RichTextHighlightStyle {
                    color: None,
                    font_weight: Some(gpui::FontWeight::BOLD),
                    font_style: None,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                    fade_out: None,
                },
            )]),
            font_family_overrides: vec![(Range { start: 5, end: 9 }, "Mono".into())],
            clickable_ranges: vec![0..4, 5..9],
            on_click: Some(1),
            on_hover: Some(2),
            tooltip: None,
        },
        Node::Anchored {
            anchor: Anchor::TopLeft,
            fit: AnchoredFitMode::SnapToWindow,
            position: Some([1.0, 2.0]),
            position_mode: AnchoredPositionMode::Window,
            offset: Some([3.0, 4.0]),
            children: vec![text("anchored")],
        },
        Node::UniformList {
            id: id("uniform"),
            path: vec![id("root"), id("uniform")],
            route: 3,
            style: style(),
            interactivity: Default::default(),
            count: 100,
            measure_index: 0,
            sizing: UniformListSizing::Auto,
            horizontal_sizing: UniformListHorizontalSizing::FitList,
            y_flipped: false,
            scroll_request: Some(UniformListScrollRequest {
                index: 7,
                strategy: UniformListScrollStrategy::Center,
                offset: 0,
                strict: true,
            }),
            revision: 1,
            indices: vec![0, 1],
            children: vec![text("row 0"), text("row 1")],
        },
        Node::List {
            id: id("messages"),
            path: vec![id("root"), id("messages")],
            item_count: 3,
            alignment: ListAlignment::Bottom,
            overdraw: 200.0,
            sizing: ListSizingBehavior::Infer,
            following_tail: true,
            revision: 2,
            commands: vec![
                ListCommand::Reset { count: 3 },
                ListCommand::ScrollTo(ListOffset {
                    item_ix: 1,
                    offset_in_item: 4.0,
                }),
            ],
            request_handler: 4,
            scroll_handler: Some(5),
            range_start: 0,
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                role: Some(gpui::Role::ListBox),
                aria: Aria {
                    label: Some("rows".into()),
                    live: Some(Live::Polite),
                    busy: true,
                    required: true,
                    read_only: true,
                    invalid: Some(Invalid::True),
                    has_popup: Some(HasPopup::Listbox),
                    current: Some(AriaCurrent::False),
                    actions: vec![(Action::ScrollIntoView, 10), (Action::CustomAction, 11)],
                    custom_actions: vec![(3, "Pin".into())],
                    ..Default::default()
                },
                focusable: true,
                ..Default::default()
            })),
            children: vec![text("item")],
        },
        Node::ResizeHandle {
            id: id("handle"),
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                role: Some(gpui::Role::Splitter),
                aria: Aria {
                    label: Some("Resize".into()),
                    orientation: Some(gpui::Orientation::Vertical),
                    ..Default::default()
                },
                focusable: true,
                on_key_down: Some(9),
                ..Default::default()
            })),
            on_press: Some(6),
            on_release: Some(7),
            on_drag: Some(8),
            cursor: Some(mouse::Cursor::ResizingColumn),
            content: boxed("divider"),
        },
        Node::Deferred {
            priority: 1,
            content: boxed("later"),
        },
        Node::Sensor {
            id: id("sensor"),
            style: style(),
            on_bounds: Some(16),
            child: boxed("measured"),
        },
        Node::Web {
            id: id("page"),
            url: "https://example.com/".into(),
            style: style(),
            revision: 3,
            commands: vec![
                WebCommand::Load("https://example.org/".into()),
                WebCommand::Back,
            ],
            on_state: Some(17),
        },
        Node::Camera {
            id: id("camera"),
            style: style(),
            device: Some("0".into()),
            mirror: true,
            object_fit: ImageObjectFit::Cover,
            on_state: Some(18),
            publish: Some(live_track("cam/3")),
        },
        Node::Microphone {
            id: id("microphone"),
            style: style(),
            device: Some("Built-in Microphone".into()),
            output: Some("Headphones".into()),
            monitor: true,
            revision: 2,
            commands: vec![MicCommand::Record { seconds: 5 }, MicCommand::Stop],
            on_state: Some(19),
            publish: Some(live_track("mic/3")),
            muted: true,
        },
        Node::Remote {
            id: id("tile"),
            style: style(),
            track: live_track("cam/4"),
            media: RemoteMedia::Video {
                object_fit: ImageObjectFit::Cover,
            },
            on_state: Some(20),
        },
        Node::Remote {
            id: id("voice"),
            style: style(),
            track: live_track("mic/4"),
            media: RemoteMedia::Audio {
                output: Some("Headphones".into()),
            },
            on_state: Some(21),
        },
        Node::Artifact {
            id: id("artifact"),
            link: "duck://dognet-b5b6ea90/files/shared/artifacts/plan.html".into(),
            style: style(),
        },
        text("plain"),
        // drawn in capitals, named as written
        Node::Text(TextNode {
            id: Some(id("capitals")),
            style: style(),
            content: "Drawn in capitals".into(),
            transform: Some(TextTransform::Uppercase),
        }),
        Node::Image {
            id: Some(id("image")),
            hash: 42,
            data: Some(ImageData::Rgba {
                width: 1,
                height: 1,
                pixels: vec![255, 0, 0, 255],
            }),
            label: Some("a pixel".into()),
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::Contain,
            },
            loading: false,
            fallback: false,
            state_children: vec![text("while loading")],
            style: style(),
            interactivity: Default::default(),
        },
        Node::Svg {
            id: Some(id("svg")),
            source: SvgSource::Data {
                hash: 44,
                bytes: Some(b"<svg/>".to_vec()),
            },
            transformation: SvgTransformation {
                scale: [1.0, 1.0],
                translate: [0.0, 0.0],
                rotate: 0.0,
            },
            label: Some("a picture".into()),
            style: style(),
            interactivity: Default::default(),
        },
        Node::Field {
            id: ElementIdWire::CodeLocation {
                file: "view.rs".into(),
                line: 10,
                column: 2,
            },
            multiline: false,
            value: "x".into(),
            cursor: TextRange { start: 0, end: 1 },
            generation: 2,
            revision: 3,
            tokens: Default::default(),
            claims: Default::default(),
            options: Box::new(view_wire::InputOptions {
                invalid: Some(Invalid::True),
                required: true,
                read_only: true,
                text_input: view_wire::TextInput {
                    autocorrect: true,
                    autocapitalize: view_wire::Autocapitalize::Words,
                    suggestions: true,
                    input_action: view_wire::TextInputAction::Go,
                    virtual_keyboard: view_wire::VirtualKeyboard::Url,
                },
                ..Default::default()
            }),
            placeholder: "Name".into(),
            secure: false,
            on_change: Some(20),
            on_key: None,
            on_submit: Some(21),
            style: style(),
        },
        Node::Space,
        Node::Overlay {
            id: id("overlay"),
            label: Some("dialog".into()),
            style: style(),
            on_dismiss: Some(30),
            children: vec![text("base"), text("modal")],
        },
        Node::Sheet {
            id: id("sheet"),
            label: Some("Message actions".into()),
            style: style(),
            detents: vec![0.6, 1.0],
            on_dismiss: Some(31),
            child: boxed("actions"),
        },
        Node::Canvas {
            style: style(),
            commands: vec![CanvasCommand::Draw {
                shape: CanvasShape::Circle {
                    center: [4.0, 4.0],
                    radius: 3.0,
                },
                fill: Some(gpui::red()),
                even_odd: false,
                stroke: None,
            }],
        },
    ]
}

/// The nodes beside [`first_nodes`] that show every field present and
/// every variant entered.
fn full_nodes() -> Vec<Node> {
    vec![
        Node::RichText {
            id: Some(ElementIdWire::View(1)),
            style: style(),
            text: "rich text".into(),
            runs: RichTextRuns::Highlights(vec![(
                0..4,
                RichTextHighlightStyle {
                    color: Some(gpui::red()),
                    font_weight: Some(gpui::FontWeight::BOLD),
                    font_style: Some(gpui::FontStyle::Italic),
                    background_color: Some(gpui::black()),
                    underline: Some(gpui::UnderlineStyle {
                        thickness: gpui::px(1.0),
                        color: Some(gpui::red()),
                        wavy: false,
                    }),
                    strikethrough: Some(gpui::StrikethroughStyle {
                        thickness: gpui::px(1.0),
                        color: Some(gpui::red()),
                    }),
                    fade_out: Some(0.5),
                },
            )]),
            font_family_overrides: vec![(Range { start: 5, end: 9 }, "Mono".into())],
            clickable_ranges: vec![0..4, 5..9],
            on_click: Some(1),
            on_hover: Some(2),
            tooltip: Some(40),
        },
        Node::RichText {
            id: None,
            style: style(),
            text: "runs".into(),
            // `rich_text::TextRun` is crate-private; go through gpui's own
            // (public) `TextRun` and the crate's `From` impl instead of
            // naming it.
            runs: RichTextRuns::Runs(vec![
                gpui::TextRun {
                    len: 4,
                    font: gpui::Font {
                        family: "Mono".into(),
                        features: gpui::FontFeatures::default(),
                        fallbacks: Some(gpui::FontFallbacks::from_fonts(vec!["Sans".into()])),
                        weight: gpui::FontWeight::BOLD,
                        style: gpui::FontStyle::Italic,
                    },
                    color: gpui::red(),
                    background_color: Some(gpui::black()),
                    underline: Some(gpui::UnderlineStyle {
                        thickness: gpui::px(1.0),
                        color: Some(gpui::red()),
                        wavy: true,
                    }),
                    strikethrough: Some(gpui::StrikethroughStyle {
                        thickness: gpui::px(1.0),
                        color: Some(gpui::red()),
                    }),
                }
                .into(),
            ]),
            font_family_overrides: vec![],
            clickable_ranges: vec![],
            on_click: None,
            on_hover: None,
            tooltip: None,
        },
        Node::Anchored {
            anchor: Anchor::TopLeft,
            fit: AnchoredFitMode::SnapToWindowWithMargin([1.0, 2.0, 3.0, 4.0]),
            position: Some([1.0, 2.0]),
            position_mode: AnchoredPositionMode::Window,
            offset: Some([3.0, 4.0]),
            children: vec![text("anchored")],
        },
        Node::UniformList {
            id: ElementIdWire::Integer(2),
            path: vec![id("root"), id("uniform")],
            route: 3,
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                focusable: true,
                ..Default::default()
            })),
            count: 100,
            measure_index: 0,
            sizing: UniformListSizing::Auto,
            horizontal_sizing: UniformListHorizontalSizing::FitList,
            y_flipped: false,
            scroll_request: Some(UniformListScrollRequest {
                index: 7,
                strategy: UniformListScrollStrategy::Center,
                offset: 0,
                strict: true,
            }),
            revision: 1,
            indices: vec![0, 1],
            children: vec![text("row 0"), text("row 1")],
        },
        Node::List {
            id: id("messages"),
            path: vec![id("root"), id("messages")],
            item_count: 3,
            alignment: ListAlignment::Bottom,
            overdraw: 200.0,
            sizing: ListSizingBehavior::Infer,
            following_tail: true,
            revision: 2,
            commands: vec![
                ListCommand::Reset { count: 3 },
                ListCommand::Splice {
                    start: 0,
                    end: 1,
                    count: 2,
                },
                ListCommand::Remeasure { start: 0, end: 2 },
                ListCommand::ScrollTo(ListOffset {
                    item_ix: 1,
                    offset_in_item: 4.0,
                }),
                ListCommand::ScrollToRevealItem(2),
                ListCommand::SetFollowMode { tail: true },
            ],
            request_handler: 4,
            scroll_handler: Some(5),
            range_start: 0,
            style: style(),
            interactivity: Some(Box::new(full_interactivity())),
            children: vec![text("item")],
        },
        Node::Image {
            id: Some(ElementIdWire::NamedInteger("img".into(), 3)),
            hash: 42,
            data: Some(ImageData::Rgba {
                width: 1,
                height: 1,
                pixels: vec![255, 0, 0, 255],
            }),
            label: Some("a pixel".into()),
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::Contain,
            },
            loading: false,
            fallback: false,
            state_children: vec![text("while loading")],
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                focusable: true,
                ..Default::default()
            })),
        },
        Node::Image {
            id: Some(ElementIdWire::Uuid([9; 16])),
            hash: 43,
            data: Some(ImageData::Encoded(vec![0, 255])),
            label: None,
            image_style: ImageStyle {
                grayscale: true,
                object_fit: ImageObjectFit::Fill,
            },
            loading: false,
            fallback: false,
            state_children: vec![],
            style: style(),
            interactivity: Default::default(),
        },
        Node::Image {
            id: None,
            hash: 44,
            data: Some(ImageData::Resource("image:7".into())),
            label: None,
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::Cover,
            },
            loading: true,
            fallback: false,
            state_children: vec![],
            style: style(),
            interactivity: Default::default(),
        },
        Node::Image {
            id: None,
            hash: 45,
            data: Some(ImageData::Refusal("resource refused".into())),
            label: None,
            image_style: ImageStyle {
                grayscale: false,
                object_fit: ImageObjectFit::ScaleDown,
            },
            loading: false,
            fallback: true,
            state_children: vec![],
            style: style(),
            interactivity: Default::default(),
        },
        Node::Svg {
            id: Some(ElementIdWire::Path(b"a/b".to_vec())),
            source: SvgSource::Data {
                hash: 44,
                bytes: Some(b"<svg/>".to_vec()),
            },
            transformation: SvgTransformation {
                scale: [1.0, 1.0],
                translate: [0.0, 0.0],
                rotate: 0.0,
            },
            label: Some("a picture".into()),
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                focusable: true,
                ..Default::default()
            })),
        },
        Node::Svg {
            id: None,
            source: SvgSource::Asset("icons/check.svg".into()),
            transformation: SvgTransformation {
                scale: [1.0, 1.0],
                translate: [0.0, 0.0],
                rotate: 0.0,
            },
            label: None,
            style: style(),
            interactivity: Default::default(),
        },
        Node::Svg {
            id: None,
            source: SvgSource::External("https://example.com/a.svg".into()),
            transformation: SvgTransformation {
                scale: [1.0, 1.0],
                translate: [0.0, 0.0],
                rotate: 0.0,
            },
            label: None,
            style: style(),
            interactivity: Default::default(),
        },
        Node::Field {
            id: ElementIdWire::OpaqueId([5; 20]),
            multiline: true,
            value: "hi @al".into(),
            cursor: TextRange { start: 6, end: 6 },
            generation: 1,
            revision: 9,
            tokens: Box::new([TextToken {
                range: TextRange { start: 3, end: 6 },
                id: "<@1>".into(),
            }]),
            claims: Box::new([KeyClaim {
                key: keyboard::Key::Named(keyboard::Named::Enter),
                modifiers: gpui::Modifiers::default(),
                command: true,
            }]),
            options: Box::new(view_wire::InputOptions {
                label: "notes".into(),
                description: Some("must not be empty".into()),
                invalid: Some(Invalid::True),
                required: true,
                read_only: true,
                ..Default::default()
            }),
            placeholder: "Notes".into(),
            secure: true,
            on_change: Some(22),
            on_key: Some(90),
            on_submit: None,
            style: style(),
        },
        Node::Canvas {
            style: style(),
            commands: vec![
                CanvasCommand::Push {
                    translate: [1.0, 1.0],
                    rotate: 0.1,
                    scale: [1.0, 1.0],
                    clip: Some([0.0, 0.0, 10.0, 10.0]),
                },
                CanvasCommand::Draw {
                    shape: CanvasShape::Rectangle {
                        position: [0.0, 0.0],
                        size: [4.0, 4.0],
                        radius: [1.0, 1.0, 1.0, 1.0],
                    },
                    fill: Some(gpui::red()),
                    even_odd: true,
                    stroke: Some(CanvasStroke {
                        color: gpui::black(),
                        width: 1.0,
                        cap: CanvasLineCap::Round,
                        join: CanvasLineJoin::Bevel,
                        dash: vec![1.0, 2.0],
                        dash_offset: 1,
                    }),
                },
                CanvasCommand::Draw {
                    shape: CanvasShape::Line {
                        from: [0.0, 0.0],
                        to: [4.0, 4.0],
                    },
                    fill: None,
                    even_odd: false,
                    stroke: None,
                },
                CanvasCommand::Draw {
                    shape: CanvasShape::Circle {
                        center: [4.0, 4.0],
                        radius: 3.0,
                    },
                    fill: Some(gpui::red()),
                    even_odd: false,
                    stroke: None,
                },
                CanvasCommand::Draw {
                    shape: CanvasShape::Path(vec![
                        CanvasSegment::Rectangle {
                            position: [0.0, 0.0],
                            size: [1.0, 1.0],
                            radius: [0.0, 0.0, 0.0, 0.0],
                        },
                        CanvasSegment::Circle {
                            center: [1.0, 1.0],
                            radius: 1.0,
                        },
                        CanvasSegment::Move([0.0, 0.0]),
                        CanvasSegment::Line([1.0, 1.0]),
                        CanvasSegment::Arc {
                            center: [1.0, 1.0],
                            radius: 1.0,
                            start: 0.0,
                            end: 1.0,
                        },
                        CanvasSegment::ArcTo {
                            a: [0.0, 0.0],
                            b: [1.0, 1.0],
                            radius: 0.5,
                        },
                        CanvasSegment::Ellipse {
                            center: [1.0, 1.0],
                            radius: [2.0, 1.0],
                            rotation: 0.2,
                            start: 0.0,
                            end: 1.0,
                        },
                        CanvasSegment::Bezier {
                            a: [0.0, 0.0],
                            b: [1.0, 1.0],
                            end: [2.0, 2.0],
                        },
                        CanvasSegment::Quadratic {
                            control: [1.0, 0.0],
                            end: [2.0, 1.0],
                        },
                        CanvasSegment::Close,
                    ]),
                    fill: None,
                    even_odd: false,
                    stroke: None,
                },
                CanvasCommand::Pop,
            ],
        },
        Node::Container(ContainerNode {
            id: Some(ElementIdWire::FocusHandle(11)),
            style: style(),
            interactivity: Some(Box::new(Interactivity {
                focusable: true,
                ..Default::default()
            })),
            children: vec![],
        }),
    ]
}

/// Every `Aria` field the host reads past phase 1, set: `Invalid::True` and
/// `AriaCurrent::False` are the strings `"true"` and `"false"`, not booleans.
fn every_aria() -> Aria {
    Aria {
        author_id: Some("author-1".into()),
        label: Some("rows".into()),
        description: Some("a list of rows".into()),
        keyshortcuts: Some("Ctrl+K".into()),
        active_descendant: true,
        value: Some("42".into()),
        placeholder: Some("type here".into()),
        selected: Some(true),
        expanded: Some(true),
        disabled: Some(false),
        numeric_value: Some(3.0),
        numeric_value_step: Some(1.0),
        min_numeric_value: Some(0.0),
        max_numeric_value: Some(10.0),
        level: Some(1),
        position_in_set: Some(2),
        size_of_set: Some(5),
        row_index: Some(0),
        column_index: Some(1),
        row_count: Some(3),
        column_count: Some(4),
        toggled: Some(gpui::Toggled::Mixed),
        orientation: Some(gpui::Orientation::Horizontal),
        live: Some(Live::Polite),
        busy: true,
        required: true,
        read_only: true,
        invalid: Some(Invalid::True),
        has_popup: Some(HasPopup::Listbox),
        current: Some(AriaCurrent::False),
        actions: vec![(Action::ScrollIntoView, 10), (Action::CustomAction, 11)],
        custom_actions: vec![(3, "Pin".into())],
    }
}

pub fn node_variant(node: &Node) -> &'static str {
    match node {
        Node::RichText { .. } => "RichText",
        Node::Anchored { .. } => "Anchored",
        Node::UniformList { .. } => "UniformList",
        Node::List { .. } => "List",
        Node::Container(_) => "Container",
        Node::ResizeHandle { .. } => "ResizeHandle",
        Node::Deferred { .. } => "Deferred",
        Node::Sensor { .. } => "Sensor",
        Node::Text(_) => "Text",
        Node::Image { .. } => "Image",
        Node::Svg { .. } => "Svg",
        Node::Field { .. } => "Field",
        Node::Space => "Space",
        Node::Overlay { .. } => "Overlay",
        Node::Sheet { .. } => "Sheet",
        Node::Canvas { .. } => "Canvas",
        Node::Web { .. } => "Web",
        Node::Camera { .. } => "Camera",
        Node::Microphone { .. } => "Microphone",
        Node::Remote { .. } => "Remote",
        Node::Artifact { .. } => "Artifact",
        Node::Unknown { .. } => unreachable!("a node this build cannot read never crosses"),
    }
}
