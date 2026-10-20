use super::*;

pub(super) fn sanitize_node(
    node: &mut Node,
    depth: usize,
    walk: &mut Walk,
    scopes: &mut identity::Scopes,
    row: Option<usize>,
) -> Result<(), Refused> {
    if depth >= MAX_DEPTH {
        let cut = std::mem::replace(node, Node::empty());
        walk.cut(|cuts| &mut cuts.depth, usize::from(cut != Node::empty()));
        return Ok(());
    }
    let entered = scopes.enter(identity::segment(node.identity().cloned(), row))?;
    let opened_sheet = matches!(node, Node::Sheet { .. });
    if opened_sheet {
        if walk.in_tooltip {
            return Err("a sheet inside a tooltip".into());
        }
        if walk.in_sheet {
            return Err("a sheet inside a sheet".into());
        }
        if std::mem::replace(&mut walk.sheet_seen, true) {
            return Err("a second sheet in one frame".into());
        }
        walk.in_sheet = true;
    }
    if let Node::Container(crate::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })
    | Node::UniformList {
        interactivity: Some(interactivity),
        ..
    }
    | Node::List {
        interactivity: Some(interactivity),
        ..
    }
    | Node::ResizeHandle {
        interactivity: Some(interactivity),
        ..
    }
    | Node::Image {
        interactivity: Some(interactivity),
        ..
    }
    | Node::Svg {
        interactivity: Some(interactivity),
        ..
    } = node
    {
        sanitize_interactivity(interactivity, walk)?;
        if interactivity.on_back.is_some() && depth > 0 {
            return Err("on_back below the root".into());
        }
        let aria = &mut interactivity.aria;
        if aria.active_descendant {
            aria.active_descendant = !std::mem::replace(&mut walk.active_descendant, true);
        }
    }
    if let Some(id) = entered.then(|| scopes.path().last()).flatten() {
        id.validate_host()?;
    }
    sanitize_fields(node, walk, scopes.path())?;
    let takes_focus = node
        .interactivity()
        .is_some_and(|i| (i.focusable || i.focus_handle.is_some()) && i.role.is_some());
    let claimed_outside =
        takes_focus.then(|| std::mem::replace(&mut walk.active_descendant, false));
    sanitize_children(node, depth, walk, scopes)?;
    if let Some(claimed) = claimed_outside {
        walk.active_descendant = claimed;
    }
    if opened_sheet {
        walk.in_sheet = false;
    }
    scopes.leave(entered);
    Ok(())
}

fn sanitize_fields(
    node: &mut Node,
    walk: &mut Walk,
    authored_path: &[ElementIdWire],
) -> Result<(), Refused> {
    match node {
        Node::Container(crate::ContainerNode { style, .. })
        | Node::ResizeHandle { style, .. }
        | Node::Sensor { style, .. }
        | Node::Text(crate::TextNode { style, .. }) => walk.style(*style)?,
        Node::Space | Node::Unknown { .. } | Node::Deferred { .. } => {}
        Node::UniformList {
            id,
            path,
            style,
            count,
            measure_index,
            indices,
            children,
            ..
        } => {
            walk.style(*style)?;
            uniform_list_path(id, path, authored_path)?;
            *measure_index = (*measure_index).min(count.saturating_sub(1));
            let rows: usize = children.iter().map(Node::count).sum();
            uniform_list_rows(*count, indices, children);
            let kept: usize = children.iter().map(Node::count).sum();
            walk.cut(|cuts| &mut cuts.nodes, rows - kept);
        }
        Node::List {
            path,
            item_count,
            overdraw,
            style,
            commands,
            range_start,
            children,
            ..
        } => {
            if path != authored_path {
                return Err("list authored path is invalid".into());
            }
            for id in path.iter() {
                id.validate_host()?;
            }
            *overdraw = bounded(*overdraw);
            walk.style(*style)?;
            list_commands(commands, *item_count);
            *range_start = (*range_start).min(*item_count);
            cut_children(children, *item_count - *range_start, walk);
        }
        Node::Overlay {
            style, children, ..
        } => {
            walk.style(*style)?;
            cut_children(children, 2, walk);
        }
        Node::Sheet { style, detents, .. } => {
            walk.style(*style)?;
            ascending_sheet_detents(detents)?;
        }
        Node::Canvas { style, commands } => {
            walk.style(*style)?;
            canvas::sanitize(commands);
        }
        Node::Web { style, .. }
        | Node::Camera { style, .. }
        | Node::Microphone { style, .. }
        | Node::Remote { style, .. }
        | Node::Artifact { style, .. } => walk.style(*style)?,
        Node::Anchored {
            fit,
            position,
            offset,
            ..
        } => {
            for point in [position, offset].into_iter().flatten() {
                for value in point {
                    *value = finite(*value);
                }
            }
            if let AnchoredFitMode::SnapToWindowWithMargin(edges) = fit {
                for edge in edges {
                    *edge = bounded(*edge);
                }
            }
        }
        Node::RichText {
            style,
            text,
            runs,
            font_family_overrides,
            clickable_ranges,
            ..
        } => {
            walk.style(*style)?;
            rich_text::sanitize(text, runs, font_family_overrides, clickable_ranges);
        }
        Node::Image {
            data,
            style,
            loading,
            fallback,
            state_children,
            ..
        } => {
            ImageData::sanitize(data);
            walk.style(*style)?;
            let expected = usize::from(*loading) + usize::from(*fallback);
            state_children.truncate(expected);
            if state_children.len() < expected {
                *loading = false;
                *fallback = false;
                state_children.clear();
            }
        }
        Node::Svg {
            transformation,
            style,
            ..
        } => {
            for value in &mut transformation.scale {
                *value = finite(*value);
            }
            for value in &mut transformation.translate {
                *value = finite(*value);
            }
            transformation.rotate = finite(transformation.rotate);
            walk.style(*style)?;
        }
        Node::Field {
            value,
            cursor,
            tokens,
            claims,
            style,
            ..
        } => {
            crate::validate_field(value, *cursor, tokens, claims)?;
            walk.style(*style)?;
        }
    }
    if let Some(track) = node.live_track() {
        track.check()?;
    }
    Ok(())
}

fn ascending_sheet_detents(detents: &[f32]) -> Result<(), &'static str> {
    const MOST: usize = 8;
    if detents.len() > MOST {
        return Err("a sheet has at most eight detents");
    }
    if detents
        .iter()
        .any(|d| !d.is_finite() || !(0. ..=1.).contains(d) || *d == 0.)
    {
        return Err("a sheet detent is a fraction of the body");
    }
    if detents.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("sheet detents ascend");
    }
    Ok(())
}

fn cut_children(children: &mut Vec<Node>, keep: usize, walk: &mut Walk) {
    let dropped: usize = children.iter().skip(keep).map(Node::count).sum();
    walk.cut(|cuts| &mut cuts.nodes, dropped);
    children.truncate(keep);
}

fn uniform_list_path(
    id: &ElementIdWire,
    path: &[ElementIdWire],
    authored_path: &[ElementIdWire],
) -> Result<(), &'static str> {
    if path.is_empty() || path.last() != Some(id) || path != authored_path {
        return Err("uniform-list authored path is invalid");
    }
    for ancestor in path {
        ancestor.validate_host()?;
    }
    Ok(())
}

fn uniform_list_rows(count: usize, indices: &mut Vec<u32>, children: &mut Vec<Node>) {
    let mut kept_indices = Vec::with_capacity(indices.len());
    let mut kept_children = Vec::with_capacity(children.len());
    for (index, child) in indices.drain(..).zip(children.drain(..)) {
        if (index as usize) < count {
            kept_indices.push(index);
            kept_children.push(child);
        }
    }
    *indices = kept_indices;
    *children = kept_children;
}

fn list_commands(commands: &mut [ListCommand], item_count: usize) {
    for command in commands {
        match command {
            ListCommand::Reset { .. } => {}
            ListCommand::Splice { start, end, .. } => *end = (*end).max(*start),
            ListCommand::Remeasure { start, end } => {
                *start = (*start).min(item_count);
                *end = (*end).clamp(*start, item_count);
            }
            ListCommand::ScrollTo(offset) => {
                offset.item_ix = offset.item_ix.min(item_count);
                offset.offset_in_item = bounded(offset.offset_in_item);
            }
            ListCommand::ScrollToRevealItem(index) => {
                *index = (*index).min(item_count.saturating_sub(1));
            }
            ListCommand::ScrollToEnd
            | ListCommand::SetFollowMode { .. }
            | ListCommand::PauseFollowingTail => {}
        }
    }
}

fn sanitize_children(
    node: &mut Node,
    depth: usize,
    walk: &mut Walk,
    scopes: &mut identity::Scopes,
) -> Result<(), Refused> {
    for child in 0..node.children().len() {
        let row = identity::row(node, child);
        walk.at.push(child as u32);
        sanitize_node(
            &mut node.children_mut()[child],
            depth + 1,
            walk,
            scopes,
            row,
        )?;
        walk.at.pop();
    }
    Ok(())
}
