//! The random nodes a hostile tree is made of: every value drawn to land
//! on or past a bound `sanitize` promises.
use super::*;

pub(super) fn gen_f32(rng: &mut Rng) -> f32 {
    match rng.next_range(20) {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => f32::MAX,
        4 => f32::MIN,
        5 => -(rng.skewed(1_000_000, 2) as f32),
        _ => rng.skewed(20_000, 2) as f32 - 5_000.0,
    }
}

pub(super) fn gen_id(rng: &mut Rng) -> ElementIdWire {
    if rng.poisons_ids() && rng.next_range(8) == 0 {
        return ElementIdWire::FocusHandle(rng.next_u64());
    }
    ElementIdWire::Name(gen_key(rng).into())
}

pub(super) fn gen_key(rng: &mut Rng) -> String {
    const POOL: [&str; 5] = ["App/a", "App/b", "dup", "x", "same-key"];
    (*rng.choose(&POOL)).to_string()
}

pub(super) fn gen_string(rng: &mut Rng) -> String {
    const POOL: [char; 6] = ['a', 'Z', 'é', '한', '😀', '\n'];
    let (cap, exponent) = match rng.next_range(300) {
        0 => (3 * LONG_STRING, 8),
        _ => (48, 2),
    };
    let target = rng.skewed(cap, exponent);
    let mut s = String::new();
    while s.len() < target {
        s.push(*rng.choose(&POOL));
    }
    s
}

pub(super) fn gen_field(rng: &mut Rng) -> Node {
    let value = gen_string(rng);
    let boundary = |rng: &mut Rng, text: &str| {
        let mut at = rng.next_range(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let hostile = rng.next_range(4) == 0;
    let cursor = match hostile {
        true => TextRange::caret(value.len() + 1),
        false => {
            let (a, b) = (boundary(rng, &value), boundary(rng, &value));
            TextRange::from(a.min(b)..a.max(b))
        }
    };
    let mut tokens = Vec::new();
    if !hostile && rng.next_bool() && !value.is_empty() {
        let end = boundary(rng, &value).max(value.chars().next().map_or(0, char::len_utf8));
        tokens.push(TextToken {
            range: TextRange::from(0..end),
            id: "<@1>".into(),
        });
    }
    let mut claims = Vec::new();
    if rng.next_bool() {
        claims.push(KeyClaim {
            key: keyboard::Key::Named(match hostile {
                true => keyboard::Named::Backspace,
                false => keyboard::Named::Enter,
            }),
            modifiers: Default::default(),
            command: false,
        });
    }
    Node::Field {
        id: gen_id(rng),
        multiline: rng.next_bool(),
        value,
        cursor,
        generation: rng.next_u64(),
        revision: rng.next_u64(),
        tokens: tokens.into(),
        claims: claims.into(),
        options: Box::new(InputOptions {
            label: gen_string(rng),
            description: Some(gen_string(rng)),
            disabled: rng.next_bool(),
            invalid: rng.next_bool().then_some(Invalid::True),
            required: rng.next_bool(),
            read_only: rng.next_bool(),
            text_input: TextInput {
                autocorrect: rng.next_bool(),
                autocapitalize: *rng.choose(&[
                    Autocapitalize::None,
                    Autocapitalize::Words,
                    Autocapitalize::Sentences,
                    Autocapitalize::Characters,
                ]),
                suggestions: rng.next_bool(),
                input_action: *rng.choose(&[TextInputAction::Unspecified, TextInputAction::Send]),
                virtual_keyboard: *rng.choose(&[
                    VirtualKeyboard::Text,
                    VirtualKeyboard::Url,
                    VirtualKeyboard::Numeric,
                ]),
            },
        }),
        placeholder: gen_string(rng),
        secure: rng.next_bool(),
        on_change: rng.next_bool().then(|| rng.next_u64() as u32),
        on_key: rng.next_bool().then(|| rng.next_u64() as u32),
        on_submit: rng.next_bool().then(|| rng.next_u64() as u32),
        style: PLAIN,
    }
}

pub(super) fn gen_text(rng: &mut Rng) -> Node {
    Node::Text(view_wire::TextNode {
        id: None,
        style: if rng.next_range(8) == 0 {
            gen_style(rng)
        } else {
            PLAIN
        },
        content: gen_string(rng),
        transform: rng
            .next_bool()
            .then_some(view_wire::TextTransform::Uppercase),
    })
}

pub(super) fn gen_svg(rng: &mut Rng) -> Node {
    let bytes = rng.next_bool().then(|| {
        let len = match rng.next_range(16) {
            0 => BIG_PICTURE + 1 + rng.next_range(64),
            _ => rng.skewed(4096, 2),
        };
        vec![b'<'; len]
    });
    if rng.next_bool() {
        let data = bytes.map(|bytes| {
            if rng.next_bool() {
                ImageData::Encoded(bytes)
            } else {
                ImageData::Rgba {
                    width: rng.next_u64() as u32,
                    height: rng.next_u64() as u32,
                    pixels: bytes,
                }
            }
        });
        return Node::Image {
            id: Some(gen_id(rng)),
            hash: rng.next_u64(),
            data,
            label: rng.next_bool().then(|| gen_string(rng)),
            image_style: ImageStyle {
                grayscale: rng.next_bool(),
                object_fit: ImageObjectFit::Contain,
            },
            loading: false,
            fallback: false,
            state_children: Vec::new(),
            style: gen_style(rng),
            interactivity: Default::default(),
        };
    }
    Node::Svg {
        id: Some(gen_id(rng)),
        source: SvgSource::Data {
            hash: rng.next_u64(),
            bytes,
        },
        transformation: SvgTransformation {
            scale: [gen_f32(rng), gen_f32(rng)],
            translate: [gen_f32(rng), gen_f32(rng)],
            rotate: gen_f32(rng),
        },
        label: rng.next_bool().then(|| gen_string(rng)),
        style: gen_style(rng),
        interactivity: Some(Box::new(Interactivity {
            hover: Some(gen_style(rng)),
            ..Default::default()
        })),
    }
}

pub(super) fn gen_leaf(rng: &mut Rng) -> Node {
    match rng.next_range(5) {
        0 => gen_text(rng),
        1 => {
            gen_native_style(rng);
            Node::Space
        }
        2 => gen_svg(rng),
        3 => match rng.next_range(3) {
            0 => gen_web(rng),
            _ => gen_media(rng),
        },
        _ => gen_field(rng),
    }
}

fn gen_url(rng: &mut Rng) -> String {
    let mut url = String::from("https://");
    let target = rng.next_range(2 * LONG_URL);
    while url.len() < target {
        url.push(*rng.choose(&['a', '/', '한', '%']));
    }
    url
}

pub(super) fn gen_web(rng: &mut Rng) -> Node {
    let url = gen_url(rng);
    let commands = (0..=rng.next_range(WEB_COMMANDS))
        .map(|_| match rng.next_range(3) {
            0 => view_wire::WebCommand::Load(gen_url(rng)),
            1 => view_wire::WebCommand::Back,
            _ => view_wire::WebCommand::Reload,
        })
        .collect();
    Node::Web {
        id: gen_id(rng),
        url,
        style: PLAIN,
        revision: rng.next_u64(),
        commands,
        on_state: rng.next_bool().then(|| rng.next_u64() as u32),
    }
}

fn gen_device(rng: &mut Rng) -> Option<String> {
    rng.next_bool()
        .then(|| "d".repeat(rng.next_range(2 * LONG_DEVICE)))
}

fn gen_track(rng: &mut Rng) -> view_wire::methods::LiveTrack {
    use program::live::{MAX_NAMESPACE, MAX_TRACK_NAME};
    let room = match rng.next_range(6) {
        0 => String::new(),
        1 => "call\nroom".into(),
        2 => "r".repeat(MAX_NAMESPACE + rng.next_range(2)),
        _ => "call".into(),
    };
    let name = match rng.next_range(8) {
        0 => "x".repeat(MAX_TRACK_NAME + rng.next_range(2)),
        1 => "mic/../1".into(),
        2 => "mic//1".into(),
        3 => String::new(),
        _ => format!("mic/{}", rng.next_u64()),
    };
    view_wire::methods::LiveTrack { room, name }
}

pub(super) fn gen_media(rng: &mut Rng) -> Node {
    let on_state = |rng: &mut Rng| rng.next_bool().then(|| rng.next_u64() as u32);
    let object_fit = |rng: &mut Rng| {
        *rng.choose(&[
            ImageObjectFit::Fill,
            ImageObjectFit::Contain,
            ImageObjectFit::Cover,
            ImageObjectFit::ScaleDown,
            ImageObjectFit::None,
        ])
    };
    match rng.next_range(3) {
        0 => Node::Camera {
            id: gen_id(rng),
            style: PLAIN,
            device: gen_device(rng),
            mirror: rng.next_bool(),
            object_fit: object_fit(rng),
            on_state: on_state(rng),
            publish: rng.next_bool().then(|| gen_track(rng)),
        },
        1 => {
            let commands = (0..=rng.next_range(MIC_COMMANDS))
                .map(|_| match rng.next_bool() {
                    true => view_wire::MicCommand::Record {
                        seconds: rng.next_u64() as u8,
                    },
                    false => view_wire::MicCommand::Stop,
                })
                .collect();
            Node::Microphone {
                id: gen_id(rng),
                style: PLAIN,
                device: gen_device(rng),
                output: gen_device(rng),
                monitor: rng.next_bool(),
                revision: rng.next_u64(),
                commands,
                on_state: on_state(rng),
                publish: rng.next_bool().then(|| gen_track(rng)),
                muted: rng.next_bool(),
            }
        }
        _ => Node::Remote {
            id: gen_id(rng),
            style: PLAIN,
            track: gen_track(rng),
            media: match rng.next_bool() {
                true => RemoteMedia::Video {
                    object_fit: object_fit(rng),
                },
                false => RemoteMedia::Audio {
                    output: gen_device(rng),
                },
            },
            on_state: on_state(rng),
        },
    }
}

pub(super) fn gen_container(rng: &mut Rng, children: Vec<Node>) -> Node {
    let styled = rng.next_range(8) == 0;
    let refinement = |rng: &mut Rng| styled.then(|| gen_style(rng));
    let group = |rng: &mut Rng| {
        refinement(rng).map(|style| GroupRefinement {
            group: "row".into(),
            style,
        })
    };
    Node::Container(view_wire::ContainerNode {
        id: None,
        style: refinement(rng).unwrap_or(PLAIN),
        interactivity: Some(Box::new(Interactivity {
            hover: refinement(rng),
            active: refinement(rng),
            group_hover: group(rng),
            group_active: group(rng),
            on_back: (rng.next_range(8) == 0).then(|| rng.next_u64() as u32),
            ..Default::default()
        })),
        children,
    })
}

pub(super) fn gen_list(rng: &mut Rng, children: Vec<Node>) -> Node {
    let overlay = rng.next_range(3) != 0 && rng.next_range(5) == 1;
    if overlay {
        Node::Overlay {
            id: gen_id(rng),
            label: rng.next_bool().then(|| gen_string(rng)),
            on_dismiss: Some(rng.next_u64() as u32),
            children,
            style: PLAIN,
        }
    } else {
        gen_container(rng, children)
    }
}
