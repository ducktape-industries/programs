use super::*;

fn gen_tree(rng: &mut Rng, depth: usize, width: usize) -> Node {
    let mut node = gen_leaf(rng);
    let width_level = if depth == 0 { 0 } else { rng.next_range(depth) };
    for level in 0..depth {
        if level == width_level && width > 0 {
            let mut children: Vec<Node> = (0..width).map(|_| gen_leaf(rng)).collect();
            children.push(node);
            node = gen_list(rng, children);
            continue;
        }
        node = match rng.next_range(6) {
            5 => Node::Sheet {
                id: gen_id(rng),
                label: rng.next_bool().then(|| gen_string(rng)),
                style: gen_style(rng),
                detents: gen_detents(rng),
                on_dismiss: rng.next_bool().then(|| rng.next_u64() as u32),
                child: Box::new(node),
            },
            4 => Node::ResizeHandle {
                id: gen_id(rng),
                on_press: rng.next_bool().then(|| rng.next_u64() as u32),
                on_release: rng.next_bool().then(|| rng.next_u64() as u32),
                on_drag: rng.next_bool().then(|| rng.next_u64() as u32),
                cursor: Some(mouse::Cursor::ResizingHorizontally),
                content: Box::new(node),
                style: PLAIN,
                interactivity: Default::default(),
            },
            3 => Node::Sensor {
                id: gen_id(rng),
                on_bounds: rng.next_bool().then(|| rng.next_u64() as u32),
                child: Box::new(node),
                style: gen_style(rng),
            },
            2 => Node::Deferred {
                priority: rng.next_range(64),
                content: Box::new(node),
            },
            0 => gen_container(rng, vec![node]),
            _ => gen_list(rng, vec![node]),
        };
    }
    node
}

fn gen_detents(rng: &mut Rng) -> Vec<f32> {
    match rng.next_range(20) {
        0 => Vec::new(),
        1 => vec![0.9, 0.5],
        2 => vec![1.5],
        3 => vec![f32::NAN],
        4 => (0..9).map(|i| i as f32 / 10.).collect(),
        _ => {
            let count = 1 + rng.next_range(3);
            (0..count)
                .map(|i| (i + 1) as f32 / (count + 1) as f32)
                .collect()
        }
    }
}

pub(super) fn gen_frame_with(rng: &mut Rng, depth: usize, width: usize) -> Frame {
    start_table();
    let root = gen_tree(rng, depth, width);
    let requests = (0..rng.next_range(4))
        .map(|_| Request {
            id: rng.next_u64(),
            kind: gen_string(rng),
            payload: (0..rng.next_range(16))
                .map(|_| rng.next_range(256) as u8)
                .collect(),
        })
        .collect();
    let cancels = (0..rng.next_range(4)).map(|_| rng.next_u64()).collect();
    Frame {
        tooltip_responses: Vec::new(),
        styles: take_styles(),
        root: Some(root),
        requests,
        cancels,
        unchanged: rng.next_bool(),
        busy: rng.next_bool(),
        patches: Vec::new(),
    }
}

pub(super) fn gen_path(rng: &mut Rng, root: &Node, hostile: bool) -> Vec<u32> {
    let mut path = Vec::new();
    let mut node = root;
    while !node.children().is_empty() && rng.next_range(4) != 0 {
        let index = rng.next_range(node.children().len());
        path.push(index as u32);
        node = &node.children()[index];
    }
    match rng.next_range(12) {
        0 if hostile => path.push(rng.next_range(4) as u32),
        1 if hostile => {
            path = (0..rng.next_range(4))
                .map(|_| rng.next_range(8) as u32)
                .collect()
        }
        _ => {}
    }
    path
}

pub(super) fn gen_patch_tree(rng: &mut Rng) -> Node {
    let (depth, width) = match rng.next_range(40) {
        0 => (MAX_DEPTH + 4, NODES / 4),
        _ => (rng.skewed(4, 2), rng.skewed(6, 2)),
    };
    gen_tree(rng, depth, width)
}

pub(super) fn is_list_node(node: &Node) -> bool {
    matches!(
        node,
        Node::Container(view_wire::ContainerNode { .. })
            | Node::Overlay { .. }
            | Node::Anchored { .. }
            | Node::Image { .. }
            | Node::UniformList { .. }
    )
}

pub(super) fn gen_patch(rng: &mut Rng, root: &Node, hostile: bool) -> Patch {
    let path = gen_path(rng, root, hostile);
    let mut node = Some(root);
    for index in &path {
        node = node.and_then(|node| node.children().get(*index as usize));
    }
    let is_list = node.is_some_and(is_list_node);
    let len = node.map_or(0, |node| node.children().len());
    let index = |rng: &mut Rng, bound: usize| match rng.next_range(8) {
        0 if hostile => rng.next_range(bound + 3) as u32,
        _ => rng.next_range(bound.max(1)) as u32,
    };
    let kind = match (is_list || hostile, rng.next_range(5)) {
        (false, kind) => kind % 2,
        (true, kind) if kind >= 3 && len == 0 && !hostile => 2,
        (true, kind) => kind,
    };
    match kind {
        0 => Patch::Replace {
            path,
            node: gen_patch_tree(rng),
        },
        1 => {
            let mut fresh = gen_patch_tree(rng);
            let same_arity = |fresh: &Node| match node {
                Some(at) => match (is_list_node(at), is_list_node(fresh)) {
                    (true, true) => true,
                    (false, false) => at.children().len() == fresh.children().len(),
                    _ => false,
                },
                None => false,
            };
            if !hostile {
                while !same_arity(&fresh) {
                    fresh = gen_patch_tree(rng);
                }
            }
            if hostile && rng.next_range(4) == 0 {
                return Patch::Props { path, node: fresh };
            }
            for child in fresh.children_mut() {
                *child = Node::empty();
            }
            if let Node::Container(view_wire::ContainerNode { children, .. }) = &mut fresh {
                children.clear();
            }
            Patch::Props { path, node: fresh }
        }
        2 => Patch::Insert {
            path,
            index: index(rng, len + 1),
            node: gen_patch_tree(rng),
        },
        3 => Patch::Remove {
            path,
            index: index(rng, len),
        },
        _ => Patch::Move {
            path,
            from: index(rng, len),
            to: index(rng, len),
        },
    }
}

pub(super) fn gen_frame(rng: &mut Rng, i: usize) -> Frame {
    let (depth, width) = if i == 0 {
        (MAX_DEPTH + 8, NODES + 300)
    } else {
        (rng.skewed(2 * MAX_DEPTH, 6), rng.skewed(NODES / 4, 6))
    };
    gen_frame_with(rng, depth, width)
}

pub(super) fn gen_frame_bounded(rng: &mut Rng) -> Frame {
    let depth = rng.skewed(MAX_DEPTH / 2, 3);
    let width = rng.skewed(NODES / 64, 6);
    gen_frame_with(rng, depth, width)
}

pub(super) fn on_big_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    let handle = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .expect("spawn a big-stack thread");
    match handle.join() {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
