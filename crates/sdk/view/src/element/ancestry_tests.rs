use super::*;
use crate::InteractiveElement;
use std::cell::RefCell;
use std::rc::Rc;

struct PathProbe {
    id: Option<ElementId>,
    paths: Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
}

impl PathProbe {
    fn identified(
        id: impl Into<ElementId>,
        paths: &Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
    ) -> Self {
        Self {
            id: Some(id.into()),
            paths: paths.clone(),
        }
    }

    fn anonymous(paths: &Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>) -> Self {
        Self {
            id: None,
            paths: paths.clone(),
        }
    }
}

impl IntoElement for PathProbe {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PathProbe {
    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        self.paths
            .borrow_mut()
            .push(lowering.current_path().to_vec());
        wire::Node::Text(crate::wire::TextNode {
            id: self
                .id
                .map(|id| wire::ElementIdWire::from_gpui(id).unwrap()),
            style: lowering.no_style(),
            content: "probe".into(),
            transform: None,
        })
    }
}

struct ProbeComponent {
    paths: Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
}

impl RenderOnce for ProbeComponent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().child(PathProbe::identified("leaf", &self.paths).into_any_element())
    }
}

impl IntoElement for ProbeComponent {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ProbeComponent {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.render_once(*self)
    }
}

fn lower(element: impl IntoElement) -> wire::Node {
    let mut app = App::for_driver();
    let mut window = app.window();
    Lowering::new(&mut window, &mut app).lower(element)
}

fn named(name: &'static str) -> wire::ElementIdWire {
    wire_id(ElementId::Name(name.into()))
}

#[test]
fn equal_local_ids_have_distinct_typed_parent_paths() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(
        div()
            .child(
                div()
                    .id(ElementId::Integer(1))
                    .child(PathProbe::identified("same", &paths)),
            )
            .child(
                div()
                    .id(ElementId::Name("1".into()))
                    .child(PathProbe::identified("same", &paths)),
            ),
    );
    assert_eq!(
        *paths.borrow(),
        vec![
            vec![wire_id(ElementId::Integer(1)), named("same")],
            vec![wire_id(ElementId::Name("1".into())), named("same")]
        ]
    );
}

#[test]
fn anonymous_any_and_render_once_wrappers_are_transparent() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(div().id("root").child(div().child(ProbeComponent {
        paths: paths.clone(),
    })));
    assert_eq!(*paths.borrow(), vec![vec![named("root"), named("leaf")]]);
}

#[test]
fn identified_child_scope_is_popped_before_its_sibling() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(
        div()
            .id("root")
            .child(div().id("branch").child(PathProbe::anonymous(&paths)))
            .child(PathProbe::anonymous(&paths)),
    );
    assert_eq!(
        *paths.borrow(),
        vec![vec![named("root"), named("branch")], vec![named("root")]]
    );
}

#[test]
fn uniform_list_opens_its_typed_registry_scope() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    let row_paths = paths.clone();
    lower(uniform_list("list", 1, move |_, _, _| {
        vec![PathProbe::identified("row", &row_paths)]
    }));
    assert_eq!(*paths.borrow(), vec![vec![named("list"), named("row")]]);
}

#[test]
fn equal_uniform_list_ids_use_distinct_typed_parent_registries() {
    let root = lower(
        div()
            .child(
                div()
                    .id(ElementId::Integer(1))
                    .child(uniform_list("same", 1, |_, _, _| vec![div()])),
            )
            .child(div().id(ElementId::Name("1".into())).child(uniform_list(
                "same",
                1,
                |_, _, _| vec![div()],
            ))),
    );
    fn collect(node: &wire::Node, lists: &mut Vec<(Vec<wire::ElementIdWire>, u32)>) {
        if let wire::Node::UniformList { path, route, .. } = node {
            lists.push((path.clone(), *route));
        }
        for child in node.children() {
            collect(child, lists);
        }
    }
    let mut lists = Vec::new();
    collect(&root, &mut lists);
    assert_eq!(
        lists
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![wire_id(ElementId::Integer(1)), named("same")],
            vec![wire_id(ElementId::Name("1".into())), named("same")],
        ]
    );
    assert_ne!(lists[0].1, lists[1].1);
}

#[test]
fn uniform_list_lowers_selected_measurement_and_scroll_request() {
    let scroll = UniformListScrollHandle::new();
    scroll.scroll_to_item_strict_with_offset(42, ScrollStrategy::Center, 2);
    let root = lower(
        uniform_list("list", 100, |range, _, _| {
            range.map(|_| div()).collect::<Vec<_>>()
        })
        .with_width_from_item(Some(7))
        .track_scroll(&scroll)
        .y_flipped(true),
    );
    let wire::Node::UniformList {
        indices,
        measure_index,
        y_flipped,
        scroll_request,
        revision,
        ..
    } = root
    else {
        panic!("expected uniform list");
    };
    let margin = crate::list::MARGIN_ROWS as u32;
    let window = 42 - margin..43 + margin;
    assert_eq!(
        indices,
        std::iter::once(7).chain(window).collect::<Vec<_>>()
    );
    assert_eq!(measure_index, 7);
    assert!(y_flipped);
    assert!(scroll.y_flipped());
    assert_eq!(
        scroll_request,
        Some(wire::list::UniformListScrollRequest {
            index: 42,
            strategy: wire::list::UniformListScrollStrategy::Center,
            offset: 2,
            strict: true,
        })
    );
    assert_eq!(revision, 1, "the first request");
}
