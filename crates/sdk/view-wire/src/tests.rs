use super::*;
use crate::styles::testing::{held, plain, sanitize_plain};

fn text(content: &str) -> Node {
    Node::Text(crate::TextNode {
        id: None,
        style: StyleId(0),
        content: content.into(),
        transform: None,
    })
}

fn field(key: &str, value: &str, placeholder: &str) -> Node {
    Node::Field {
        id: ElementIdWire::Name(key.into()),
        multiline: true,
        value: value.into(),
        cursor: TextRange::caret(value.len()),
        generation: 0,
        revision: 0,
        tokens: Default::default(),
        claims: Default::default(),
        options: Default::default(),
        placeholder: placeholder.into(),
        secure: false,
        on_change: Some(0),
        on_key: None,
        on_submit: None,
        style: StyleId(0),
    }
}

fn sanitized_root(root: Node) -> Node {
    let mut frame = Frame {
        root: Some(root),
        ..Frame::default()
    };
    sanitize_plain(&mut frame).unwrap();
    frame.root.unwrap()
}

fn sanitized_children(root: Node) -> Vec<Node> {
    let Node::Container(crate::ContainerNode { children, .. }) = sanitized_root(root) else {
        panic!("a sanitized column is still a column")
    };
    children
}

fn column(children: Vec<Node>) -> Node {
    Node::Container(crate::ContainerNode {
        id: None,
        style: StyleId(0),
        interactivity: Default::default(),
        children,
    })
}

fn keyed(key: &str, content: &str) -> Node {
    let mut node = text(content);
    let Node::Text(crate::TextNode { id, .. }) = &mut node else {
        unreachable!()
    };
    *id = Some(ElementIdWire::Name(key.into()));
    node
}

fn uniform(path: Vec<ElementIdWire>) -> Node {
    Node::UniformList {
        id: ElementIdWire::Name("list".into()),
        path,
        route: 1,
        style: StyleId(0),
        interactivity: Default::default(),
        count: 1,
        measure_index: 0,
        sizing: list::UniformListSizing::Auto,
        horizontal_sizing: list::UniformListHorizontalSizing::FitList,
        y_flipped: false,
        scroll_request: None,
        revision: 0,
        indices: vec![0],
        children: vec![text("row")],
    }
}

fn sensor(key: &str, on_bounds: Option<u32>, content: Node) -> Node {
    Node::Sensor {
        id: ElementIdWire::Name(key.into()),
        style: StyleId(0),
        on_bounds,
        child: Box::new(content),
    }
}

fn deep_chain_bytes(depth: usize) -> Vec<u8> {
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(move || {
            let mut node = Node::empty();
            for _ in 0..depth {
                node = Node::Deferred {
                    priority: 0,
                    content: Box::new(node),
                };
            }
            let frame = Frame {
                root: Some(node),
                ..Frame::default()
            };
            let bytes = encode(&frame);
            drop(frame);
            bytes
        })
        .expect("hostile frame thread")
        .join()
        .expect("hostile frame")
}

fn picture(bytes: Option<Vec<u8>>) -> Node {
    Node::Svg {
        id: None,
        source: SvgSource::Data { hash: 7, bytes },
        transformation: SvgTransformation {
            scale: [1., 1.],
            translate: [0., 0.],
            rotate: 0.,
        },
        label: None,
        style: StyleId(0),
        interactivity: Default::default(),
    }
}

mod accessibility_and_decode;
mod bytes;
mod patches;
mod primitives;
mod roundtrip;
