use std::sync::Arc;

use serde::{Deserialize, Serialize};
use view::prelude::*;
use view::testing::TestAppContext;
use view::{ElementId, Input, TextField, View, Window, wire};

#[derive(Default, Serialize, Deserialize)]
struct Form {
    value: TextField,
    submits: usize,
}

impl View for Form {
    const NAME: &'static str = "Form";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = ElementId::NamedChild(Arc::new(ElementId::Name("chat".into())), "search".into());
        div().child(
            Input::new(id, &self.value, "Search messages")
                .placeholder("Search")
                .w(px(240.))
                .on_submit(cx.listener(|view, _: &(), _, cx| {
                    view.submits += 1;
                    cx.notify();
                })),
        )
    }
}

type Lowered<'a> = (
    &'a wire::ElementIdWire,
    u32,
    u32,
    &'a gpui::StyleRefinement,
    &'a str,
    u64,
);

fn field(cx: &TestAppContext) -> &wire::Node {
    let wire::Node::Container(view::wire::ContainerNode { children, .. }) = cx.root() else {
        panic!("root container")
    };
    &children[0]
}

fn input(cx: &TestAppContext) -> Lowered<'_> {
    let wire::Node::Container(view::wire::ContainerNode { children, .. }) = cx.root() else {
        panic!("root container")
    };
    let wire::Node::Field {
        id,
        on_change: Some(on_change),
        on_submit: Some(on_submit),
        style,
        options,
        generation,
        ..
    } = &children[0]
    else {
        panic!("input child")
    };
    (
        id,
        *on_change,
        *on_submit,
        &cx.styles()[*style],
        &options.label,
        *generation,
    )
}

#[test]
fn input_lowers_typed_identity_style_and_frame_owned_callbacks() {
    let mut first = TestAppContext::new();
    let mut second = TestAppContext::new();
    let first_form = first.open::<Form>();
    let second_form = second.open::<Form>();
    let (id, first_input, first_submit, style, label, first_generation) = input(&first);
    let (second_id, second_input, second_submit, _, _, _) = input(&second);
    assert_eq!(label, "Search messages");
    assert_eq!(id, second_id);
    assert_eq!(first_input, second_input);
    assert_eq!(first_submit, second_submit);
    assert_eq!(
        id,
        &wire::ElementIdWire::NamedChild {
            base: wire::ElementIdAtom::Name("chat".into()),
            names: vec!["search".into()],
        }
    );
    assert_eq!(style, &gpui::StyleRefinement::default().w(px(240.)));

    first.simulate_input("Search", "hello");
    first.simulate_submit("Search");
    first_form.read(|form| {
        assert_eq!(form.value.text(), "hello");
        assert_eq!(form.submits, 1);
    });
    second_form.read(|form| {
        assert_eq!(form.value.text(), "");
        assert_eq!(form.submits, 0);
    });

    first.simulate_event(wire::Event::Text {
        handler: second_input,
        change: wire::TextChange {
            generation: first_generation,
            revision: 9,
            edit: Some(wire::Edit {
                range: wire::TextRange::caret(0),
                len: 5,
            }),
            text: "stale".into(),
            cursor: wire::TextRange::caret(5),
            preedit: None,
            tokens: Default::default(),
        },
    });
    first_form.read(|form| assert_eq!(form.value.text(), "stale"));
    let wire::Node::Field { revision, .. } = field(&first) else {
        panic!("the field")
    };
    assert_eq!(*revision, 9);
}

#[test]
fn a_bound_field_hears_what_is_typed() {
    let mut cx = TestAppContext::new();
    let form = cx.open::<Form>();
    let renders = cx.renders();
    cx.simulate_input("Search", "hi");
    form.read(|form| assert_eq!(form.value.text(), "hi"));
    assert_eq!(cx.renders(), renders + 1, "the keystroke drew once");
    let wire::Node::Field { value, .. } = field(&cx) else {
        panic!("the field")
    };
    assert_eq!(value, "hi", "and the frame shows it");
}

#[derive(Default, Serialize, Deserialize)]
struct Counted {
    name: TextField,
    heard: Vec<String>,
}

impl View for Counted {
    const NAME: &'static str = "Counted";
}

impl Render for Counted {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(
            Input::new("name", &self.name, "Name")
                .placeholder("Name")
                .on_change(
                    cx.listener(|view: &mut Counted, _: &wire::TextChange, _, cx| {
                        view.heard.push(view.name.text());
                        cx.notify();
                    }),
                ),
        )
    }
}

#[test]
fn on_change_runs_after_the_field_took_the_change() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Counted>();
    let renders = cx.renders();
    cx.simulate_input("Name", "a");
    cx.simulate_input("Name", "ab");
    view.read(|view| assert_eq!(view.heard, ["a", "ab"]));
    assert_eq!(cx.renders(), renders + 2, "one render a change, not two");
}
