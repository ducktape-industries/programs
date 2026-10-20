//! What a later build appends reads on this one (J-085, `codec.rs`
//! "Appends"): trailing fields and sparse keys are stepped over, and an
//! unknown variant blanks the node or the event it is in, nothing more.
use gpui::StyleRefinement;
use view_wire::{
    ContainerNode, Event, Interactivity, Node, Patch, Style, StyleId, Styles, TextNode,
    TextTransform, decode, encode,
};

const LATER: [u8; 5] = [0x81, 0x7f, 0x92, 0x01, 0x02];

fn text(transform: Option<TextTransform>) -> Node {
    Node::Text(TextNode {
        id: None,
        style: StyleId(0),
        content: "a".into(),
        transform,
    })
}

fn list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0x90 | items.len() as u8];
    items.iter().for_each(|item| bytes.extend_from_slice(item));
    bytes
}

fn unknown(node: &Node) -> bool {
    matches!(node, Node::Unknown { .. })
}

#[test]
fn a_node_of_a_later_kind_is_unknown_and_its_siblings_read() {
    let nodes: Vec<Node> = decode(&list(&[
        encode(&Node::Space),
        LATER.to_vec(),
        encode(&text(None)),
    ]))
    .unwrap();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0], Node::Space);
    assert!(unknown(&nodes[1]), "{:?}", nodes[1]);
    assert_eq!(nodes[2], text(None));
}

#[test]
fn a_later_variant_inside_a_node_blanks_that_node_alone() {
    let mut later = encode(&text(Some(TextTransform::Uppercase)));
    *later.last_mut().unwrap() = 0x05;
    let parent = Node::Container(ContainerNode {
        id: None,
        style: StyleId(0),
        interactivity: None,
        children: vec![Node::Space],
    });
    let mut bytes = encode(&parent);
    let at = bytes.len() - 2;
    assert_eq!(bytes[at..], [0x91, encode(&Node::Space)[0]]);
    bytes.truncate(at);
    bytes.extend(list(&[later, encode(&Node::Space)]));
    let Node::Container(ContainerNode { children, .. }) = decode::<Node>(&bytes).unwrap() else {
        panic!("the parent reads")
    };
    assert!(unknown(&children[0]), "{:?}", children[0]);
    let Node::Unknown { what } = &children[0] else {
        unreachable!()
    };
    assert!(what.starts_with("Node"), "{what}");
    assert_eq!(children[1], Node::Space);
}

#[test]
fn a_field_a_later_build_appended_is_stepped_over() {
    let mut bytes = encode(&text(None));
    let header = bytes.iter().position(|byte| *byte == 0x94).unwrap();
    bytes[header] = 0x95;
    bytes.extend([0x92, 0x2a, 0xa1, b'x']);
    assert_eq!(decode::<Node>(&bytes), Ok(text(None)));
}

#[test]
fn a_sparse_key_a_later_build_appended_is_stepped_over() {
    let interactivity = Interactivity {
        focusable: true,
        ..Default::default()
    };
    let mut bytes = encode(&interactivity);
    assert_eq!(bytes[0], 0x81, "one key set");
    bytes[0] = 0x82;
    bytes.extend([0x60, 0x92, 0x01, 0x02]);
    assert_eq!(decode::<Interactivity>(&bytes), Ok(interactivity));
}

#[test]
fn an_event_of_a_later_kind_is_unknown_and_the_others_read() {
    let hover = Event::Hover {
        handler: 3,
        hovered: true,
    };
    let theme = Event::Theme { dark: true };
    let events: Vec<Event> =
        decode(&list(&[encode(&hover), LATER.to_vec(), encode(&theme)])).unwrap();
    assert_eq!(events[0], hover);
    assert!(
        matches!(events[1], Event::Unknown { .. }),
        "{:?}",
        events[1]
    );
    assert_eq!(events[2], theme);
    let mut longer = encode(&hover);
    let header = longer.iter().position(|byte| *byte == 0x92).unwrap();
    longer[header] = 0x93;
    longer.push(0x07);
    assert_eq!(decode::<Event>(&longer), Ok(hover));
}

#[test]
fn a_style_field_a_later_build_appended_is_stepped_over() {
    let style = StyleRefinement {
        opacity: Some(0.5),
        ..Default::default()
    };
    let mut bytes = encode(&Style::new(&style));
    assert_eq!(bytes[0], 0xc4, "an entry is a bin");
    bytes.extend([0xe8, 0x07, 2, 0xaa, 0xbb]);
    bytes[1] += 5;
    assert_eq!(decode::<Style>(&bytes).unwrap().read(), Ok(style));
}

#[test]
fn a_patch_into_an_unknown_node_changes_nothing() {
    let mut styles = Styles::default();
    styles
        .extend(vec![Style::new(&StyleRefinement::default())])
        .unwrap();
    let mut root = Node::Container(ContainerNode {
        id: None,
        style: StyleId(0),
        interactivity: None,
        children: vec![Node::Unknown {
            what: "Node: later".into(),
        }],
    });
    let held = root.clone();
    let patches = vec![
        Patch::Props {
            path: vec![0],
            node: Node::Space,
        },
        Patch::Replace {
            path: vec![0, 1],
            node: Node::Space,
        },
        Patch::Remove {
            path: vec![0],
            index: 0,
        },
    ];
    view_wire::apply(&mut root, patches, &styles).unwrap();
    assert_eq!(root, held);
}

#[test]
fn an_unknown_value_is_stepped_over_without_recursion_or_trust() {
    let mut deep = vec![0x91, 0x81, 0x7f];
    deep.extend(std::iter::repeat_n(0x91, 100_000));
    deep.push(0xc0);
    let nodes: Vec<Node> = decode(&deep).unwrap();
    assert!(unknown(&nodes[0]));
    let lying = [0x91, 0x81, 0x7f, 0xdd, 0xff, 0xff, 0xff, 0xff];
    assert!(decode::<Vec<Node>>(&lying).is_err());
}
