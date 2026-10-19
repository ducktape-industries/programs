use super::*;

/// What `sanitize` refuses a tree for on a field's account (`validate_field`).
const FIELD_REFUSALS: [&str; 5] = [
    "field text is longer than a u32 offset reaches",
    "field cursor is off its text",
    "field token is off its text",
    "field token id is empty",
    "field claims a key the engine owns",
];

/// What `sanitize` refuses a tree for on a sheet's or the root's account:
/// a shape the generator draws at random too.
const SHAPE_REFUSALS: [&str; 6] = [
    "a sheet has at most eight detents",
    "a sheet detent is a fraction of the body",
    "sheet detents ascend",
    "a sheet inside a sheet",
    "a second sheet in one frame",
    "on_back below the root",
];

/// What `sanitize` refuses a tree for on a piece's live track's account
/// (`LiveTrack::check`).
const TRACK_REFUSALS: [&str; 2] = [
    "a live room is one line of plain text",
    "a live track's name is `/`-separated segments, none empty or `..`",
];

fn field_refusal(refused: &Refused) -> bool {
    matches!(refused.reason(), Refused::Invalid(reason)
        if FIELD_REFUSALS.contains(reason)
            || SHAPE_REFUSALS.contains(reason)
            || TRACK_REFUSALS.contains(reason))
}

// --------------------------------------------------------------- test 1

#[test]
fn random_trees_come_out_of_sanitize_well_formed() {
    const SEED: u64 = 0x5EED_F00D_1234_5678;
    const NUM_TREES: usize = 200;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let (frame, bytes) = on_big_stack(move || {
            let frame = gen_frame(&mut Rng::new(seed), i);
            let bytes = encode(&frame);
            (frame, bytes)
        });

        let mut decoded = match decode::<Frame>(&bytes) {
            Ok(decoded) => decoded,
            Err(message) => {
                let root = frame.root.as_ref().expect("gen_frame always sets a root");
                assert!(
                    tree_depth(root) > MAX_DEPTH,
                    "{ctx}: decode refused a tree within MAX_DEPTH: {message}"
                );
                assert!(
                    message.contains("deeper than the host renders"),
                    "{ctx}: unexpected refusal: {message}"
                );
                continue;
            }
        };
        let duplicate_ids = decoded
            .root
            .as_ref()
            .is_some_and(has_duplicate_typed_siblings);
        let before = decoded.root.as_ref().map(field_texts).unwrap_or_default();
        match sanitized(&mut decoded) {
            Ok(styles) => {
                check_frame(&decoded, &styles, &ctx);
                let after = decoded.root.as_ref().map(field_texts).unwrap_or_default();
                let mut kept = before.iter();
                assert!(
                    after.iter().all(|text| kept.any(|had| had == text)),
                    "{ctx}: sanitize rewrote a field's text"
                );
            }
            Err(Refused::Duplicate(_)) => {
                assert!(
                    duplicate_ids,
                    "{ctx}: identity refusal must name an actual collision"
                );
            }
            Err(refused) => assert!(
                field_refusal(&refused),
                "{ctx}: unexpected refusal: {refused}"
            ),
        }
    }
}

// --------------------------------------------------------------- test 2

fn corrupt_length_prefix(rng: &mut Rng, bytes: &mut [u8]) {
    if bytes.len() < 8 {
        return;
    }
    let at = rng.next_range(bytes.len() - 7);
    let value = *rng.choose(&[u64::MAX, 1u64 << 40, 0u64]);
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// One random mutation applied to a copy of a sound frame's bytes: a bit
/// flip, a byte overwrite, a truncation, an insertion of random bytes, or a
/// length-prefix corruption (an 8-byte little-endian window overwritten
/// with a value a real `Vec`/`String` length prefix would never hold).
fn mutate_once(rng: &mut Rng, bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.push(rng.next_range(256) as u8);
        return;
    }
    match rng.next_range(5) {
        0 => {
            let i = rng.next_range(bytes.len());
            bytes[i] ^= 1 << rng.next_range(8);
        }
        1 => {
            let i = rng.next_range(bytes.len());
            bytes[i] = rng.next_range(256) as u8;
        }
        2 => {
            let cut = rng.next_range(bytes.len() + 1);
            bytes.truncate(cut);
        }
        3 => {
            let at = rng.next_range(bytes.len() + 1);
            let junk: Vec<u8> = (0..1 + rng.next_range(16))
                .map(|_| rng.next_range(256) as u8)
                .collect();
            bytes.splice(at..at, junk);
        }
        _ => corrupt_length_prefix(rng, bytes),
    }
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

#[test]
fn mutated_bytes_never_panic() {
    const SEED: u64 = 0xBADF_00D5_A5A5_5A5A;
    const NUM_FRAMES: usize = 50;
    const MUTATIONS_PER_FRAME: usize = 200;

    for i in 0..NUM_FRAMES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let bytes = on_big_stack(move || encode(&gen_frame_bounded(&mut Rng::new(seed))));
        let mut mutator = Rng::new(seed ^ 0xF00D);

        for m in 0..MUTATIONS_PER_FRAME {
            let mut mutated = bytes.clone();
            mutate_once(&mut mutator, &mut mutated);
            let ctx = format!("seed={seed:#x} frame={i} mutation={m}");

            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decode::<Frame>(&mutated)
            }));
            match outcome {
                Ok(Ok(mut decoded)) => {
                    if let Ok(styles) = sanitized(&mut decoded) {
                        check_frame(&decoded, &styles, &ctx);
                    }
                }
                Ok(Err(_)) => {}
                Err(payload) => panic!("{ctx}: decode panicked: {}", payload_message(&payload)),
            }
        }
    }
}

// --------------------------------------------------------------- test 2b

#[test]
fn a_patched_sanitized_tree_is_a_sanitized_tree() {
    const SEED: u64 = 0x9A7C_4E5D_0B1A_2F3E;
    const NUM_TREES: usize = 60;
    const PATCHES_PER_TREE: usize = 24;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        on_big_stack(move || {
            let mut rng = Rng::new(seed);
            let (depth, width) = (rng.skewed(MAX_DEPTH, 3), rng.skewed(64, 2));
            let mut frame = gen_frame_with(&mut rng, depth, width);
            let Ok(mut styles) = sanitized(&mut frame) else {
                return;
            };
            let mut root = frame
                .root
                .take()
                .expect("gen_frame_with always sets a root");
            let hostile = rng.next_range(3) == 0;
            let mut patches = Vec::new();
            let mut staged = root.clone();
            for _ in 0..rng.next_range(PATCHES_PER_TREE + 1) {
                let patch = gen_patch(&mut rng, &staged, hostile);
                // the styles its subtree names reach the table first, as
                // the patch frame's own entries do
                styles.extend(take_styles()).unwrap();
                // Paths are drawn against the tree as the patches so far
                // leave it, so a well-behaved sequence applies whole and
                // a hostile one is refused somewhere along it.
                let mut candidate = staged.clone();
                let applied = view_wire::apply(&mut candidate, vec![patch.clone()], &styles);
                if matches!(
                    applied.as_ref().map_err(Refused::reason),
                    Err(Refused::Duplicate(_))
                ) {
                    assert!(
                        has_duplicate_typed_siblings(&candidate),
                        "{ctx}: missing collision"
                    );
                    continue;
                }
                if applied.as_ref().is_err_and(field_refusal) {
                    continue;
                }
                staged = candidate;
                assert!(
                    hostile || applied.is_ok(),
                    "{ctx}: a well-behaved patch was refused: {applied:?}\n{patch:#?}"
                );
                patches.push(patch);
            }
            let patched = Frame {
                patches,
                ..Frame::default()
            };
            let decoded: Frame = match decode(&encode(&patched)) {
                Ok(decoded) => decoded,
                Err(message) => {
                    assert!(
                        message.contains("deeper than the host renders"),
                        "{ctx}: unexpected refusal: {message}"
                    );
                    return;
                }
            };
            let outcome = view_wire::apply(&mut root, decoded.patches, &styles);
            // Each patch was drawn against the tree `apply` had sanitized so
            // far; the batch sanitizes once, at the end, so a well-behaved
            // sequence can still collide in it. `apply` leaves the tree it
            // refused as the batch made it, collision included.
            if matches!(
                outcome.as_ref().map_err(Refused::reason),
                Err(Refused::Duplicate(_))
            ) {
                assert!(
                    has_duplicate_typed_siblings(&root),
                    "{ctx}: missing collision"
                );
                return;
            }
            assert!(
                hostile || outcome.is_ok() || outcome.as_ref().is_err_and(field_refusal),
                "{ctx}: a structurally valid sequence was refused: {outcome:?}"
            );
            match outcome {
                Ok(_) => {
                    let checked = Frame {
                        root: Some(root),
                        ..Frame::default()
                    };
                    check_frame(&checked, &styles, &ctx);
                }
                Err(refused) => {
                    let named = matches!(
                        refused,
                        Refused::Invalid(
                            "a path to no node"
                                | "an index past the list"
                                | "a list edit on no list"
                                | "props of another arity"
                        )
                    ) || field_refusal(&refused);
                    assert!(named, "{ctx}: unexpected refusal: {refused}");
                }
            }
        });
    }
}

// --------------------------------------------------------------- test 2c

#[test]
fn a_diff_applied_to_the_old_tree_is_the_new_tree_for_random_pairs() {
    const SEED: u64 = 0xD1FF_0000_A99B_1E5A;
    const NUM_PAIRS: usize = 150;
    const EDITS_PER_PAIR: usize = 8;

    for i in 0..NUM_PAIRS {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} pair={i}");
        on_big_stack(move || {
            let mut rng = Rng::new(seed);
            // a tree, and the table it names its styles in
            let tree = |rng: &mut Rng| {
                let (depth, width) = (rng.skewed(MAX_DEPTH / 2, 3), rng.skewed(48, 2));
                let mut frame = gen_frame_with(rng, depth, width);
                let styles = sanitized(&mut frame).ok()?;
                Some((frame.root.take()?, styles))
            };
            let Some((mut old, mut styles)) = tree(&mut rng) else {
                return;
            };
            let mut new = match rng.next_range(8) {
                0 => {
                    // an unrelated tree is all the result holds, so its
                    // table is the one the result is checked against
                    let Some((tree, table)) = tree(&mut rng) else {
                        return;
                    };
                    styles = table;
                    tree
                }
                _ => {
                    let mut edited = old.clone();
                    for _ in 0..1 + rng.next_range(EDITS_PER_PAIR) {
                        let patch = gen_patch(&mut rng, &edited, false);
                        styles.extend(take_styles()).unwrap();
                        let mut candidate = edited.clone();
                        match view_wire::apply(&mut candidate, vec![patch], &styles) {
                            Ok(_) => edited = candidate,
                            Err(refused) if field_refusal(&refused) => {}
                            Err(refused) if matches!(refused.reason(), Refused::Duplicate(_)) => {
                                assert!(
                                    has_duplicate_typed_siblings(&candidate),
                                    "{ctx}: missing collision"
                                );
                            }
                            Err(refused) => panic!("{ctx}: {refused}"),
                        }
                    }
                    edited
                }
            };
            let (old_before, new_before) = (old.clone(), new.clone());
            let patches = diff(&mut old, &mut new);
            assert_eq!(old, old_before, "{ctx}: diff moved the old tree");
            assert_eq!(new, new_before, "{ctx}: diff moved the new tree");
            let count = patches.len();
            let mut applied = old;
            match view_wire::apply(&mut applied, patches, &styles) {
                Ok(_) => assert_eq!(applied, new, "{ctx}: {count} patches"),
                Err(refused) => panic!("{ctx}: {count} patches refused: {refused}"),
            }
        });
    }
}

// --------------------------------------------------------------- test 3

/// A valid MessagePack array32 header claims u32::MAX children with no
/// payload. Decode must refuse without preallocating the claimed vector.
#[test]
fn a_length_prefix_bomb_is_refused_without_the_allocation() {
    let frame = |children| Frame {
        root: Some(Node::Container(view_wire::ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Default::default(),
            children,
        })),
        ..Default::default()
    };
    let empty = encode(&frame(vec![]));
    let one = encode(&frame(vec![Node::empty()]));
    let offset = empty.iter().zip(&one).take_while(|(a, b)| a == b).count();
    assert_eq!(empty[offset], 0x90, "empty fixarray marker");
    assert_eq!(one[offset], 0x91, "one-child fixarray marker");
    let mut bomb = empty[..offset].to_vec();
    bomb.push(0xdd); // array32, followed by its big-endian length
    bomb.extend_from_slice(&u32::MAX.to_be_bytes());
    let start = std::time::Instant::now();
    let error = decode::<Frame>(&bomb).unwrap_err();
    // the count is more than the bytes left could hold: refused at the header
    assert!(
        error.contains("the bytes end inside a value"),
        "must refuse the count the bytes cannot hold: {error}"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "the hostile size hint must cause only a bounded allocation"
    );
}

// --------------------------------------------------------------- test 4

/// `sanitize` is idempotent: running it again on its own output changes
/// nothing, which is what lets a host call it on every frame without
/// worrying whether the view already sent a clean one.
#[test]
fn sanitize_is_idempotent() {
    const SEED: u64 = 0x1DE4_1DE4_5EED_5EED;
    const NUM_TREES: usize = 150;

    for i in 0..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let mut once = on_big_stack(move || gen_frame(&mut Rng::new(seed), i));
        let Ok(styles) = sanitized(&mut once) else {
            continue;
        };
        // a second pass over what the first left: the tree as it stands,
        // with the table the host holds as the table it brings
        let mut twice = once.clone();
        twice.styles = (0..styles.len() as u32)
            .map(|id| Style::new(&styles[StyleId(id)]))
            .collect();
        let again = sanitized(&mut twice).unwrap();
        assert_eq!(styles, again, "{ctx}: sanitize is not idempotent");
        assert_eq!(once, twice, "{ctx}: sanitize is not idempotent");
    }
}

#[test]
fn resize_handle_round_trip_retains_routes_and_checks_its_child() {
    let mut frame = gen_frame_with(&mut Rng::new(17), 0, 0);
    frame.root = Some(Node::ResizeHandle {
        id: ElementIdWire::Name("divider".into()),
        on_press: Some(1),
        on_release: Some(2),
        on_drag: Some(u32::MAX),
        cursor: Some(mouse::Cursor::ResizingHorizontally),
        content: Box::new(Node::Text(TextNode {
            id: None,
            style: StyleId(1),
            content: String::new(),
            transform: None,
        })),
        style: PLAIN,
        interactivity: Default::default(),
    });
    frame.styles = common::table(&[gen_native_style(&mut Rng::new(99))]);
    let mut decoded: Frame = decode(&encode(&frame)).unwrap();
    let styles = sanitized(&mut decoded).unwrap();
    check_frame(&decoded, &styles, "resize child");
    let Node::ResizeHandle {
        on_press,
        on_release,
        on_drag,
        cursor,
        ..
    } = decoded.root.unwrap()
    else {
        panic!("resize handle retained");
    };
    assert_eq!(
        (on_press, on_release, on_drag),
        (Some(1), Some(2), Some(u32::MAX))
    );
    assert_eq!(cursor, Some(mouse::Cursor::ResizingHorizontally));
}

/// Every node kind that carries a typed id has it checked: a tree with a
/// host-local id where the walk reaches is refused, and a tree that passes
/// holds none (`check_frame` validates every surviving identity).
#[test]
fn a_host_local_id_on_any_node_kind_is_refused() {
    const SEED: u64 = 0xF0C5_1D1D_5EED_0001;
    const NUM_TREES: usize = 200;

    let mut refused = 0;
    for i in 1..NUM_TREES {
        let seed = SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let ctx = format!("seed={seed:#x} tree={i}");
        let mut frame = on_big_stack(move || gen_frame(&mut Rng::poisoning_ids(seed), i));
        match sanitized(&mut frame) {
            Ok(styles) => check_frame(&frame, &styles, &ctx),
            Err(Refused::Invalid("focus-handle element IDs are host-local")) => refused += 1,
            Err(_) => {}
        }
    }
    assert!(refused > 0, "no poisoned id reached the check");
}

/// A tooltip response's content is a tree of its own, decoded on the same
/// depth budget as the frame's tree: content nested past what the host
/// walks is refused, not decoded down the host's stack. Uncounted, 100
/// levels (9.5 KB of frame) aborted the app.
#[test]
fn tooltip_content_nested_past_what_the_host_walks_is_refused() {
    let decoded = |levels: usize| {
        on_big_stack(move || {
            let mut node = Node::empty();
            for _ in 0..levels {
                node = Node::Container(view_wire::ContainerNode {
                    id: None,
                    style: PLAIN,
                    interactivity: Default::default(),
                    children: vec![node],
                });
            }
            let bytes = encode(&Frame {
                tooltip_responses: vec![TooltipResponse {
                    request: 0,
                    character_index: None,
                    content: Some(Box::new(node)),
                }],
                ..Default::default()
            });
            decode::<Frame>(&bytes).map(drop)
        })
    };
    assert_eq!(decoded(MAX_DEPTH), Ok(()));
    let refused = decoded(MAX_DEPTH + 1).unwrap_err();
    assert!(
        refused.contains("deeper than the host renders"),
        "{refused}"
    );
}
