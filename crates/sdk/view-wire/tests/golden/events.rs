//! Every `Event` variant at least once, and the `Frame` around the tree.
use super::*;

pub fn every_event() -> Vec<Event> {
    let modifiers = gpui::Modifiers::default();
    vec![
        Event::Message(3),
        Event::Click {
            handler: 1,
            event: click::Click::Keyboard {
                button: click::KeyboardButton::Enter,
                bounds: Bounds {
                    origin: at(0.0, 0.0),
                    size: size(px(10.0), px(10.0)),
                },
            },
        },
        Event::Click {
            handler: 1,
            event: click::Click::Mouse {
                down: click::ButtonEvent {
                    button: click::MouseButton::Left,
                    position: at(0.0, 0.0),
                    modifiers,
                    click_count: 1,
                },
                up: click::ButtonEvent {
                    button: click::MouseButton::Left,
                    position: at(0.0, 0.0),
                    modifiers,
                    click_count: 1,
                },
                first_mouse: true,
            },
        },
        Event::MouseDown {
            handler: 2,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseDown {
                button: click::MouseButton::Left,
                position: at(1.0, 2.0),
                modifiers,
                click_count: 1,
                first_mouse: false,
            },
        },
        Event::MouseUp {
            handler: 3,
            phase: DispatchPhase::Capture,
            event: interactivity::MouseUp {
                button: click::MouseButton::Right,
                position: at(1.0, 2.0),
                modifiers,
                click_count: 1,
            },
        },
        Event::MouseDownOut {
            handler: 4,
            event: interactivity::MouseDown {
                button: click::MouseButton::Middle,
                position: at(3.0, 4.0),
                modifiers,
                click_count: 2,
                first_mouse: true,
            },
        },
        Event::MouseUpOut {
            handler: 5,
            event: interactivity::MouseUp {
                button: click::MouseButton::Back,
                position: at(3.0, 4.0),
                modifiers,
                click_count: 1,
            },
        },
        Event::MousePressure {
            handler: 6,
            phase: DispatchPhase::Bubble,
            event: interactivity::MousePressure {
                pressure: 0.5,
                stage: interactivity::PressureStage::Normal,
                position: at(1.0, 1.0),
                modifiers,
            },
        },
        Event::MouseMove {
            handler: 7,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseMove {
                position: at(5.0, 6.0),
                pressed_button: Some(click::MouseButton::Left),
                modifiers,
            },
        },
        Event::MouseExit {
            handler: 8,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseExit {
                position: at(5.0, 6.0),
                pressed_button: None,
                modifiers,
            },
        },
        Event::MouseExit {
            handler: 8,
            phase: DispatchPhase::Bubble,
            event: interactivity::MouseExit {
                position: at(5.0, 6.0),
                pressed_button: Some(click::MouseButton::Right),
                modifiers,
            },
        },
        Event::ScrollWheel {
            handler: 9,
            phase: DispatchPhase::Bubble,
            event: interactivity::ScrollWheel {
                position: at(5.0, 6.0),
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -12.0 },
                modifiers,
                touch_phase: interactivity::TouchPhase::Moved,
            },
        },
        Event::ScrollWheel {
            handler: 9,
            phase: DispatchPhase::Bubble,
            event: interactivity::ScrollWheel {
                position: at(5.0, 6.0),
                delta: mouse::ScrollDelta::Lines { x: 1.0, y: 2.0 },
                modifiers,
                touch_phase: interactivity::TouchPhase::Ended,
            },
        },
        Event::Pinch {
            handler: 10,
            phase: DispatchPhase::Bubble,
            event: interactivity::Pinch {
                position: at(5.0, 6.0),
                delta: 1.1,
                modifiers,
                phase: interactivity::TouchPhase::Started,
            },
        },
        Event::KeyDown {
            handler: 11,
            phase: DispatchPhase::Bubble,
            event: interactivity::KeyDown {
                state: key_state(),
                repeat: false,
                prefer_character_input: false,
            },
        },
        Event::KeyUp {
            handler: 12,
            phase: DispatchPhase::Capture,
            event: interactivity::KeyUp { state: key_state() },
        },
        Event::KeyUp {
            handler: 12,
            phase: DispatchPhase::Capture,
            event: interactivity::KeyUp {
                state: key_state_on(keyboard::NativeCode::Android(1)),
            },
        },
        Event::KeyUp {
            handler: 12,
            phase: DispatchPhase::Capture,
            event: interactivity::KeyUp {
                state: key_state_on(keyboard::NativeCode::Windows(2)),
            },
        },
        Event::KeyUp {
            handler: 12,
            phase: DispatchPhase::Capture,
            event: interactivity::KeyUp {
                state: key_state_on(keyboard::NativeCode::Xkb(3)),
            },
        },
        Event::ModifiersChanged {
            handler: 13,
            event: interactivity::ModifiersChanged {
                modifiers,
                capslock: true,
            },
        },
        Event::Hover {
            handler: 14,
            hovered: true,
        },
        Event::FileDropExit { handler: 15 },
        Event::AuxClick {
            handler: 16,
            event: click::Click::Touch {
                position: at(7.0, 8.0),
                tap_count: 1,
                long_press: false,
            },
        },
        Event::TooltipRequest {
            request: 17,
            character_index: Some(3),
        },
        Event::Text {
            handler: 19,
            change: TextChange {
                generation: 2,
                revision: 4,
                edit: Some(Edit {
                    range: TextRange { start: 1, end: 1 },
                    len: 1,
                }),
                text: "xy".into(),
                cursor: TextRange { start: 2, end: 2 },
                preedit: Some(TextRange { start: 1, end: 2 }),
                tokens: vec![TextToken {
                    range: TextRange { start: 0, end: 1 },
                    id: "<@1>".into(),
                }],
            },
        },
        Event::Theme { dark: true },
        Event::Pointer { touch: true },
        Event::Viewport {
            width: 900.0,
            height: 501.0,
        },
        Event::Offset { minutes: 540 },
        Event::Select {
            handler: 25,
            index: 1,
        },
        Event::RichTextHover {
            handler: 26,
            event: RichTextHover {
                index: Some(2),
                position: at(9.0, 9.0),
                pressed_button: None,
                modifiers: gpui::Modifiers::default(),
            },
        },
        Event::RichTextHover {
            handler: 26,
            event: RichTextHover {
                index: Some(2),
                position: at(9.0, 9.0),
                pressed_button: Some(click::MouseButton::Left),
                modifiers: gpui::Modifiers::default(),
            },
        },
        Event::Bounds {
            handler: 27,
            x: 12.0,
            y: 34.0,
            width: 100.0,
            height: 50.0,
        },
        Event::Drag {
            handler: 29,
            dx: 4.0,
            dy: -2.0,
        },
        Event::ScrollOffset {
            handler: 31,
            x: 24.0,
            y: 50.0,
            relative_x: 0.2,
            relative_y: 0.1,
        },
        Event::UniformListRange {
            path: vec![id("root"), id("uniform")],
            route: 3,
            start: 0,
            end: 2,
            item_height: 40.0,
        },
        Event::UniformListState {
            path: vec![id("root"), id("uniform")],
            route: 3,
            top_index: 0,
            scrollable: true,
            scrolled_to_end: Some(false),
        },
        Event::ListRequest {
            handler: 32,
            request: ListRequest { start: 0, end: 3 },
        },
        Event::ListScroll {
            handler: 33,
            event: ListScroll {
                visible_start: 0,
                visible_end: 3,
                count: 3,
                is_scrolled: false,
                is_following_tail: true,
                offset: ListOffset::default(),
            },
        },
        Event::Response {
            id: 1,
            result: Err(Error::new("not_found", "no such room")),
            done: true,
        },
        Event::Response {
            id: 2,
            result: Ok(vec![1, 2, 3]),
            done: false,
        },
        Event::Resync,
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::CustomAction(3)),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::Value("42".into())),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::NumericValue(3.5)),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Page)),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::ScrollHint(accesskit::ScrollHint::TopLeft)),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::ScrollToPoint(accesskit::Point {
                x: 1.0,
                y: 2.0,
            })),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::SetScrollOffset(accesskit::Point {
                x: 3.0,
                y: 4.0,
            })),
        },
        Event::A11yAction {
            handler: 11,
            data: Some(ActionData::SetTextSelection(accesskit::TextSelection {
                anchor: accesskit::TextPosition {
                    node: accesskit::NodeId(1),
                    character_index: 0,
                },
                focus: accesskit::TextPosition {
                    node: accesskit::NodeId(1),
                    character_index: 3,
                },
            })),
        },
        Event::Back { handler: 81 },
        Event::Web {
            handler: 17,
            state: WebState {
                url: "https://example.org/".into(),
                title: "Example".into(),
                loading: true,
                can_go_back: true,
                can_go_forward: true,
            },
        },
        Event::Media {
            handler: 18,
            state: MediaState {
                status: MediaStatus::Failed,
                device: "FaceTime HD Camera".into(),
                playback: Playback::Recording,
                detail: "the driver stopped".into(),
                speaking: true,
                opens_settings: true,
                monitor_stopped: true,
                track: TrackState::Up { subscribers: 2 },
            },
        },
        Event::Media {
            handler: 20,
            state: MediaState {
                status: MediaStatus::Gone,
                track: TrackState::Ended {
                    code: "track_ended".into(),
                },
                ..MediaState::default()
            },
        },
        Event::FileDrop {
            handler: 21,
            files: vec![methods::Picked {
                name: "photo.png".into(),
                size: 3,
                file: 1,
            }],
        },
    ]
}

pub fn every_frame() -> Frame {
    Frame {
        tooltip_responses: vec![TooltipResponse {
            request: 17,
            character_index: Some(3),
            content: Some(boxed("the tip")),
        }],
        styles: every_style().iter().map(Style::new).collect(),
        root: Some(every_node()),
        patches: vec![
            Patch::Replace {
                path: vec![0],
                node: text("replaced"),
            },
            Patch::Props {
                path: vec![1],
                node: text("re-propped"),
            },
            Patch::Insert {
                path: vec![],
                index: 2,
                node: text("inserted"),
            },
            Patch::Remove {
                path: vec![],
                index: 3,
            },
            Patch::Move {
                path: vec![],
                from: 4,
                to: 5,
            },
        ],
        requests: vec![Request {
            id: 1,
            kind: methods::HostLog::KIND.into(),
            payload: methods::HostLog::encode_request(&"hello".into()),
        }],
        cancels: vec![2],
        unchanged: false,
        busy: true,
    }
}

pub fn event_variant(event: &Event) -> &'static str {
    match event {
        Event::Message(_) => "Message",
        Event::Click { .. } => "Click",
        Event::MouseDown { .. } => "MouseDown",
        Event::MouseUp { .. } => "MouseUp",
        Event::MouseDownOut { .. } => "MouseDownOut",
        Event::MouseUpOut { .. } => "MouseUpOut",
        Event::MousePressure { .. } => "MousePressure",
        Event::MouseMove { .. } => "MouseMove",
        Event::MouseExit { .. } => "MouseExit",
        Event::ScrollWheel { .. } => "ScrollWheel",
        Event::Pinch { .. } => "Pinch",
        Event::KeyDown { .. } => "KeyDown",
        Event::KeyUp { .. } => "KeyUp",
        Event::ModifiersChanged { .. } => "ModifiersChanged",
        Event::Hover { .. } => "Hover",
        Event::FileDropExit { .. } => "FileDropExit",
        Event::AuxClick { .. } => "AuxClick",
        Event::TooltipRequest { .. } => "TooltipRequest",
        Event::Text { .. } => "Text",
        Event::Theme { .. } => "Theme",
        Event::Pointer { .. } => "Pointer",
        Event::Viewport { .. } => "Viewport",
        Event::Offset { .. } => "Offset",
        Event::Select { .. } => "Select",
        Event::RichTextHover { .. } => "RichTextHover",
        Event::Bounds { .. } => "Bounds",
        Event::Drag { .. } => "Drag",
        Event::ScrollOffset { .. } => "ScrollOffset",
        Event::UniformListRange { .. } => "UniformListRange",
        Event::UniformListState { .. } => "UniformListState",
        Event::ListRequest { .. } => "ListRequest",
        Event::ListScroll { .. } => "ListScroll",
        Event::Response { .. } => "Response",
        Event::Resync => "Resync",
        Event::A11yAction { .. } => "A11yAction",
        Event::Back { .. } => "Back",
        Event::Web { .. } => "Web",
        Event::Media { .. } => "Media",
        Event::FileDrop { .. } => "FileDrop",
        Event::Unknown { .. } => unreachable!("an event this build cannot read never crosses"),
    }
}
