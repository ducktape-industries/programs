use super::*;

pub(super) fn tree_depth(node: &Node) -> usize {
    match node {
        Node::Sensor { child: content, .. }
        | Node::Sheet { child: content, .. }
        | Node::ResizeHandle { content, .. }
        | Node::Deferred { content, .. } => 1 + tree_depth(content),
        Node::Container(view_wire::ContainerNode { children, .. })
        | Node::Anchored { children, .. }
        | Node::Image {
            state_children: children,
            ..
        }
        | Node::Overlay { children, .. } => 1 + children.iter().map(tree_depth).max().unwrap_or(0),
        _ => 0,
    }
}

pub(super) fn check_bounds(node: &Node, styles: &Styles, depth: usize, ctx: &str) {
    assert!(
        depth <= MAX_DEPTH,
        "{ctx}: a node sits at depth {depth}, over MAX_DEPTH"
    );
    if let Some(id) = node.identity() {
        id.validate_host()
            .expect("sanitized typed identity is portable and bounded");
    }
    let mut sibling_ids = HashSet::new();
    for child in node.children() {
        if let Some(id) = child.identity() {
            assert!(
                sibling_ids.insert(id),
                "{ctx}: typed sibling identity aliases state"
            );
        }
    }
    match node {
        Node::Container(view_wire::ContainerNode {
            style,
            interactivity,
            children,
            ..
        }) => {
            check_native_style(&styles[*style]);
            assert!(
                depth == 0 || interactivity.as_ref().is_none_or(|i| i.on_back.is_none()),
                "{ctx}: on_back survived below the root"
            );
            for conditional in interactivity
                .iter()
                .flat_map(|interactivity| {
                    [
                        interactivity.hover,
                        interactivity.active,
                        interactivity.group_hover.as_ref().map(|group| group.style),
                        interactivity.group_active.as_ref().map(|group| group.style),
                    ]
                })
                .flatten()
            {
                check_native_style(&styles[conditional]);
            }
            for child in children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::UniformList { children, .. } => {
            for child in children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::List {
            path,
            item_count,
            overdraw,
            range_start,
            children,
            ..
        } => {
            assert!(!path.is_empty());
            assert!(overdraw.is_finite() && *overdraw >= 0.0);
            assert!(*range_start <= *item_count);
            assert!(children.len() <= item_count.saturating_sub(*range_start));
            for child in children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::Sensor { style, child, .. } => {
            check_native_style(&styles[*style]);
            check_bounds(child, styles, depth + 1, ctx);
        }
        Node::Web { style, .. }
        | Node::Camera { style, .. }
        | Node::Microphone { style, .. }
        | Node::Remote { style, .. }
        | Node::Artifact { style, .. } => {
            check_native_style(&styles[*style]);
            if let Some(track) = node.live_track() {
                assert_eq!(track.check(), Ok(()), "{ctx}: a track out of its rule kept");
            }
        }
        Node::ResizeHandle { content, .. } => {
            check_bounds(content, styles, depth + 1, ctx);
        }
        Node::RichText {
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            ..
        } => {
            let valid = |range: &std::ops::Range<usize>| {
                range.start <= range.end
                    && range.end <= text.len()
                    && text.is_char_boundary(range.start)
                    && text.is_char_boundary(range.end)
            };
            match runs {
                view_wire::RichTextRuns::Highlights(highlights) => {
                    assert!(highlights.iter().all(|(range, _)| valid(range)));
                }
                view_wire::RichTextRuns::Runs(runs) => {
                    assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), text.len());
                }
            }
            assert!(font_family_overrides.iter().all(|(range, _)| valid(range)));
            assert!(clickable_ranges.iter().all(valid));
        }
        Node::Text(view_wire::TextNode { style, .. }) => check_native_style(&styles[*style]),
        Node::Image {
            data,
            style,
            state_children,
            ..
        } => {
            if let Some(data) = data {
                assert!(data.valid_rgba(), "{ctx}: invalid RGBA");
            }
            check_native_style(&styles[*style]);
            for child in state_children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::Svg {
            transformation,
            style,
            interactivity,
            ..
        } => {
            check_native_style(&styles[*style]);
            if let Some(hover) = interactivity.as_ref().and_then(|i| i.hover) {
                check_native_style(&styles[hover]);
            }
            for value in transformation
                .scale
                .into_iter()
                .chain(transformation.translate)
                .chain([transformation.rotate])
            {
                assert!(value.is_finite());
            }
        }
        Node::Anchored {
            position,
            offset,
            children,
            ..
        } => {
            for value in position.iter().chain(offset).flatten() {
                assert!(value.is_finite());
            }
            for child in children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::Deferred { content, .. } => check_bounds(content, styles, depth + 1, ctx),
        Node::Field {
            id,
            value,
            cursor,
            tokens,
            claims,
            ..
        } => {
            assert!(id.validate_host().is_ok(), "{ctx}: invalid field identity");
            assert_eq!(
                view_wire::validate_field(value, *cursor, tokens, claims),
                Ok(()),
                "{ctx}: sanitize kept an invalid field"
            );
        }
        Node::Space | Node::Unknown { .. } => {}
        Node::Overlay { children, .. } => {
            assert!(children.len() <= 2, "{ctx}: overlay child count");
            for child in children {
                check_bounds(child, styles, depth + 1, ctx);
            }
        }
        Node::Sheet {
            style,
            detents,
            child,
            ..
        } => {
            check_native_style(&styles[*style]);
            assert!(
                detents.len() <= 8
                    && detents.iter().all(|d| (0. ..=1.).contains(d))
                    && detents.windows(2).all(|pair| pair[0] < pair[1]),
                "{ctx}: sanitize kept unsound sheet detents {detents:?}"
            );
            check_bounds(child, styles, depth + 1, ctx);
        }
        Node::Canvas { style, .. } => check_native_style(&styles[*style]),
    }
}

pub(super) fn field_texts(root: &Node) -> Vec<String> {
    let mut pending = vec![root];
    let mut texts = Vec::new();
    while let Some(node) = pending.pop() {
        if let Node::Field { value, .. } = node {
            texts.push(value.clone());
        }
        pending.extend(node.children());
    }
    texts
}

pub(super) fn check_frame(frame: &Frame, styles: &Styles, ctx: &str) {
    if let Some(root) = &frame.root {
        assert!(
            !has_duplicate_typed_siblings(root),
            "{ctx}: authored identity scope aliases state"
        );
    }

    if let Some(root) = &frame.root {
        check_bounds(root, styles, 0, ctx);
    }
}

pub(super) fn has_duplicate_typed_siblings(node: &Node) -> bool {
    fn walk(node: &Node, row: Option<usize>, scope: &mut HashSet<ElementIdWire>) -> bool {
        let row_of = |at: usize| match node {
            Node::List { range_start, .. } => Some(range_start + at),
            Node::UniformList { indices, .. } => indices.get(at).map(|index| *index as usize),
            _ => None,
        };
        let filed = node
            .identity()
            .cloned()
            .or_else(|| row.map(|index| ElementIdWire::Integer(index as u64)));
        let mut own = HashSet::new();
        let scope = match filed {
            Some(id) => {
                if !scope.insert(id) {
                    return true;
                }
                &mut own
            }
            None => scope,
        };
        node.children()
            .iter()
            .enumerate()
            .any(|(at, child)| walk(child, row_of(at), scope))
    }
    walk(node, None, &mut HashSet::new())
}
