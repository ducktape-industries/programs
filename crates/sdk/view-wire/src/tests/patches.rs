use super::*;

/// `diff` then `apply` is the identity on the new tree, and the patches
/// are the ones a reader expects: a changed field is `Props`, a moved
/// key is `Move`, a new key `Insert`, a gone key `Remove`, and a node
/// of another kind `Replace`.
#[test]
fn a_diff_applied_to_the_old_tree_is_the_new_tree() {
    let old = column(vec![
        keyed("a", "one"),
        keyed("b", "two"),
        keyed("c", "three"),
        Node::Container(crate::ContainerNode {
            id: Some(ElementIdWire::Name("box".into())),
            style: StyleId(0),
            interactivity: Default::default(),
            children: vec![keyed("inner", "deep")],
        }),
    ]);
    let mut new = column(vec![
        keyed("c", "three"),
        keyed("a", "one!"),
        keyed("d", "four"),
        Node::Container(crate::ContainerNode {
            id: Some(ElementIdWire::Name("box".into())),
            style: StyleId(0),
            interactivity: Default::default(),
            children: vec![Node::empty()],
        }),
    ]);
    let mut applied = old.clone();
    let patches = diff(&mut applied.clone(), &mut new.clone());
    let mut kinds: Vec<&str> = patches
        .iter()
        .map(|patch| match patch {
            Patch::Replace { .. } => "replace",
            Patch::Props { .. } => "props",
            Patch::Insert { .. } => "insert",
            Patch::Remove { .. } => "remove",
            Patch::Move { .. } => "move",
        })
        .collect();
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        ["insert", "insert", "move", "props", "remove", "remove"],
        "{patches:#?}"
    );
    apply(&mut applied, patches, &held()).unwrap();
    assert_eq!(applied, new);
    // Nothing changed hands: both inputs of the diff are as they were.
    let mut untouched = old.clone();
    diff(&mut untouched, &mut new);
    assert_eq!(untouched, old);
    assert!(diff(&mut new.clone(), &mut new).is_empty());
}

// A keyed list edited in its middle sends the edit and nothing more: a
// row inserted is one `Insert`, a row taken out one `Remove`, a row moved
// one `Move`, and the rows around it are never sent again (J-086).
#[test]
fn a_keyed_list_edited_in_its_middle_sends_only_the_edit() {
    let rows = |keys: &[usize]| {
        column(
            keys.iter()
                .map(|key| keyed(&format!("row-{key}"), &format!("row {key}")))
                .collect(),
        )
    };
    let all: Vec<usize> = (0..100).collect();
    let mut inserted = all.clone();
    inserted.insert(50, 1_000);
    let mut removed = all.clone();
    removed.remove(50);
    let mut moved = all.clone();
    let row = moved.remove(70);
    moved.insert(20, row);
    let mut swapped = all.clone();
    swapped.swap(40, 41);
    for (new, want) in [
        (inserted, "insert"),
        (removed, "remove"),
        (moved, "move"),
        (swapped, "move"),
    ] {
        let (old, new) = (rows(&all), rows(&new));
        let patches = diff(&mut old.clone(), &mut new.clone());
        let kinds: Vec<&str> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Replace { .. } => "replace",
                Patch::Props { .. } => "props",
                Patch::Insert { .. } => "insert",
                Patch::Remove { .. } => "remove",
                Patch::Move { .. } => "move",
            })
            .collect();
        assert_eq!(kinds, [want], "{patches:#?}");
        let mut applied = old;
        apply(&mut applied, patches, &held()).unwrap();
        assert_eq!(applied, new, "{want}");
    }
}

/// A node that gains its first interaction, or loses its last, is the
/// same node with other fields: one `Props` that carries the interactivity
/// it has now (or none), and its children stay where they are.
#[test]
fn a_node_that_gains_or_loses_its_interactivity_is_one_props_patch() {
    let boxed = |interactivity| {
        column(vec![Node::Container(crate::ContainerNode {
            id: Some(ElementIdWire::Name("box".into())),
            style: StyleId(0),
            interactivity,
            children: vec![keyed("inner", "deep")],
        })])
    };
    let plain = || boxed(None);
    let clicks = || {
        boxed(Some(Box::new(Interactivity {
            on_click: Some(7),
            ..Default::default()
        })))
    };
    for (old, new, carried) in [(plain(), clicks(), Some(7)), (clicks(), plain(), None)] {
        let mut applied = old.clone();
        let patches = diff(&mut old.clone(), &mut new.clone());
        let [Patch::Props { path, node }] = &patches[..] else {
            panic!("{patches:#?}")
        };
        assert_eq!(path, &[0]);
        assert_eq!(node.interactivity().and_then(|i| i.on_click), carried);
        assert!(node.children().is_empty());
        apply(&mut applied, patches, &held()).unwrap();
        assert_eq!(applied, new);
    }
    assert!(diff(&mut plain(), &mut plain()).is_empty());
}

/// One child shown or hidden beside unkeyed siblings is one patch: the
/// siblings before and after it match by position, so a `.when(..)`
/// banner beside a 1,080-row body does not send the body again (it did:
/// two removes and three inserts, 138 KB).
#[test]
fn an_unkeyed_child_shown_beside_its_siblings_is_one_insert() {
    let body = || column((0..1_080).map(|row| text(&format!("row {row}"))).collect());
    let header = || text("header");
    let notice = || text("offline");
    // a div beside a div: the kind alone does not say which one is new
    let banner = || {
        Node::Container(crate::ContainerNode {
            id: None,
            style: StyleId(1),
            interactivity: Default::default(),
            children: vec![text("offline")],
        })
    };
    let one = |old: Node, new: Node, want: &str| {
        let patches = diff(&mut old.clone(), &mut new.clone());
        let kinds: Vec<_> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Insert { index, .. } => format!("insert {index}"),
                Patch::Remove { index, .. } => format!("remove {index}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(kinds, [want], "{want}");
        let bytes = encode(&patches).len();
        assert!(bytes < 1_000, "{want}: {bytes} bytes");
        let mut applied = old;
        apply(&mut applied, patches, &held()).unwrap();
        assert_eq!(applied, new, "{want}");
    };
    for shown in [notice(), banner()] {
        one(
            column(vec![header(), body()]),
            column(vec![header(), shown.clone(), body()]),
            "insert 1",
        );
        one(
            column(vec![header(), shown.clone(), body()]),
            column(vec![header(), body()]),
            "remove 1",
        );
        one(
            column(vec![header(), body(), banner()]),
            column(vec![header(), body(), banner(), shown]),
            "insert 3",
        );
    }
}

/// The runs a list shares at the front and the back are diffed in place
/// and the middle by key, and the patches still rebuild the new tree, as
/// `diff_taking`'s do.
#[test]
fn a_list_matches_its_ends_by_position_and_its_middle_by_key() {
    let old = column(vec![
        text("top"),
        keyed("a", "one"),
        keyed("b", "two"),
        text("loose"),
        keyed("c", "three"),
        text("bottom"),
    ]);
    let new = column(vec![
        text("top!"),
        keyed("c", "three"),
        text("new"),
        keyed("a", "one!"),
        text("bottom!"),
    ]);
    let patches = diff(&mut old.clone(), &mut new.clone());
    let mut applied = old.clone();
    apply(&mut applied, patches.clone(), &held()).unwrap();
    assert_eq!(applied, new);
    // the ends were diffed in place, not sent again
    assert!(
        patches
            .iter()
            .filter(
                |patch| matches!(patch, Patch::Props { path, .. } if path == &[0] || path == &[4])
            )
            .count()
            == 2,
        "{patches:#?}"
    );
    let mut hollow = new.clone();
    assert_eq!(diff_taking(&mut old.clone(), &mut hollow), patches);
}

/// `diff_taking` emits the patches `diff` does, but the subtrees they carry
/// are moved out of the new tree, one empty stand-in left for each.
#[test]
fn a_taking_diff_moves_the_carried_subtrees_out() {
    let old = column(vec![
        keyed("a", "one"),
        column(vec![keyed("x", "x"), keyed("y", "y")]),
        column(vec![text("replaced by a text")]),
    ]);
    let new = column(vec![
        column(vec![keyed("y", "y"), keyed("x", "x"), keyed("z", "z")]),
        keyed("a", "one!"),
        keyed("b", "new"),
        text("a text now"),
    ]);
    let expected = diff(&mut old.clone(), &mut new.clone());
    let mut hollow = new.clone();
    let patches = diff_taking(&mut old.clone(), &mut hollow);
    assert_eq!(patches, expected);
    let carried: usize = patches
        .iter()
        .filter_map(|patch| match patch {
            Patch::Replace { node, .. } | Patch::Insert { node, .. } => Some(node.count()),
            _ => None,
        })
        .sum();
    let holes = patches
        .iter()
        .filter(|patch| matches!(patch, Patch::Replace { .. } | Patch::Insert { .. }))
        .count();
    assert!(carried > holes, "{patches:#?}");
    assert_eq!(
        hollow.count(),
        new.count() - carried + holes,
        "a carried subtree leaves one empty stand-in behind: {hollow:#?}"
    );
}

#[test]
fn a_patch_the_tree_cannot_take_is_refused() {
    let tree = column(vec![keyed("a", "one")]);
    let refused = |patch: Patch| apply(&mut tree.clone(), vec![patch], &held()).unwrap_err();
    assert_eq!(
        refused(Patch::Remove {
            path: vec![7],
            index: 0
        }),
        Refused::Invalid("a path to no node")
    );
    assert_eq!(
        refused(Patch::Remove {
            path: vec![],
            index: 1
        }),
        Refused::Invalid("an index past the list")
    );
    assert_eq!(
        refused(Patch::Insert {
            path: vec![0],
            index: 0,
            node: Node::empty()
        }),
        Refused::Invalid("a list edit on no list")
    );
    assert_eq!(
        refused(Patch::Props {
            path: vec![0],
            node: Node::Deferred {
                priority: 0,
                content: Box::new(Node::empty())
            }
        }),
        Refused::Invalid("props of another arity")
    );
}

#[test]
fn an_applied_patch_frame_is_a_sanitized_tree() {
    let mut tree = column((0..9).map(|i| keyed(&i.to_string(), "x")).collect());
    sanitize_tree(&mut tree, &held()).unwrap();
    let long = "y".repeat(1 << 20);
    apply(
        &mut tree,
        vec![
            Patch::Insert {
                path: vec![],
                index: 0,
                node: keyed("inserted", &long),
            },
            Patch::Replace {
                path: vec![1],
                node: column(vec![keyed("0", "leaf")]),
            },
        ],
        &held(),
    )
    .unwrap();
    let Node::Container(crate::ContainerNode { children, .. }) = &tree else {
        panic!()
    };
    assert_eq!(children[0].key(), Some("inserted"));
    assert_eq!(children[2].key(), Some("1"));
    let Node::Text(crate::TextNode { content, .. }) = &children[0] else {
        panic!()
    };
    assert_eq!(content, &long);
}

#[test]
fn a_field_validate_field_refuses_refuses_the_frame() {
    let mut frame = Frame {
        root: Some(column(vec![field("App/e", "é", "")])),
        ..Frame::default()
    };
    let Some(Node::Container(crate::ContainerNode { children, .. })) = &mut frame.root else {
        unreachable!()
    };
    let Node::Field { cursor, .. } = &mut children[0] else {
        unreachable!()
    };
    *cursor = TextRange::caret(1);
    assert_eq!(
        sanitize_plain(&mut frame),
        Err(Refused::Invalid("field cursor is off its text"))
    );
}
