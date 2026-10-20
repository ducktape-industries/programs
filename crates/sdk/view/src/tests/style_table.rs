//! A view's styles cross once. The driver numbers each distinct style as it
//! lowers, a frame carries the entries the host does not hold yet, and a
//! tree sent whole starts the table over with the styles it names.
use super::*;
use gpui::{StyleRefinement, px};

fn row_style() -> StyleRefinement {
    StyleRefinement::default().flex().gap_2().p_4()
}

#[derive(Default, Serialize, Deserialize)]
struct Rows {
    rows: usize,
    marked: bool,
    rogue: bool,
}
impl View for Rows {
    const NAME: &'static str = "Rows";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            rows: 40,
            ..Self::default()
        }
    }
}
impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let row = |id: ElementId| {
            let mut row = div().id(id);
            *row.style() = row_style();
            row
        };
        div()
            .id("rows")
            .children((0..self.rows).map(|n| row(n.into()).child(format!("row {n}"))))
            .when(self.marked, |rows| {
                rows.child(row("marked".into()).font_weight(gpui::FontWeight::BOLD))
            })
            .when(self.rogue, |rows| rows.child(Rogue))
    }
}

struct Rogue;
impl Element for Rogue {
    fn lower(self: Box<Self>, _: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Text(wire::TextNode {
            id: None,
            style: wire::StyleId(9_999),
            content: "rogue".into(),
            transform: None,
        })
    }
}
impl IntoElement for Rogue {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[test]
fn forty_rows_of_one_style_send_it_once_and_a_new_row_brings_only_its_own() {
    let mut driver = Driver::<Rows>::new();
    let first = driver.tick_with(vec![], wire::Frame::clone);
    assert_eq!(first.root.as_ref().unwrap().count(), 81);
    assert_eq!(
        first.styles,
        [
            wire::Style::new(&StyleRefinement::default()),
            wire::Style::new(&row_style())
        ]
    );
    let rows = first.root.as_ref().unwrap().children();
    assert!(rows.iter().all(|row| row.style() == Some(wire::StyleId(1))));

    driver.entity().update(driver.app_mut(), |view, cx| {
        view.marked = true;
        cx.notify();
    });
    let patch = driver.tick_with(vec![], wire::Frame::clone);
    assert!(matches!(
        patch.patches.as_slice(),
        [wire::Patch::Insert { node, .. }] if node.style() == Some(wire::StyleId(2))
    ));
    assert_eq!(
        patch.styles,
        [wire::Style::new(
            &row_style().font_weight(gpui::FontWeight::BOLD)
        )]
    );

    driver.entity().update(driver.app_mut(), |view, cx| {
        view.rows = 41;
        cx.notify();
    });
    let patch = driver.tick_with(vec![], wire::Frame::clone);
    assert!(!patch.patches.is_empty() && patch.styles.is_empty());
}

#[test]
#[should_panic(
    expected = "the host refuses this frame: Text at [40]: a node names a style its table does not hold"
)]
fn the_test_host_refuses_a_style_its_table_does_not_hold() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Rows>();
    cx.update(&view, |view, _, cx| {
        view.rogue = true;
        cx.notify();
    });
    cx.run_until_parked();
}

#[derive(Default, Serialize, Deserialize)]
struct RogueFromTheStart;
impl View for RogueFromTheStart {
    const NAME: &'static str = "RogueFromTheStart";
}
impl Render for RogueFromTheStart {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("rogue").child(Rogue)
    }
}

#[test]
#[should_panic(
    expected = "the host refuses this frame: Text at [0]: a node names a style its table does not hold"
)]
fn the_test_host_refuses_a_whole_frame_naming_a_style_its_table_does_not_hold() {
    let mut cx = TestAppContext::new();
    cx.open::<RogueFromTheStart>();
    cx.run_until_parked();
}

#[derive(Default, Serialize, Deserialize)]
struct Tipped;
impl View for Tipped {
    const NAME: &'static str = "Tipped";
}
impl Render for Tipped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("tipped")
            .w(px(7.))
            .child("tipped")
            .tooltip(|_, cx| cx.new(|_| Tip).into())
    }
}
struct Tip;
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(9.)).child("tip")
    }
}

#[test]
fn a_tooltip_names_its_styles_in_the_table_of_the_frame_it_crosses_in() {
    let mut cx = TestAppContext::new();
    cx.open::<Tipped>();
    let width =
        |cx: &TestAppContext, node: &wire::Node| cx.styles()[node.style().unwrap()].size.width;
    let tip = |cx: &TestAppContext| {
        let [response] = cx.last_frame().tooltip_responses.as_slice() else {
            panic!("one tooltip answered")
        };
        *response.content.clone().unwrap()
    };

    cx.simulate_hover("tipped", true);
    assert!(cx.last_frame().root.is_none());
    assert_eq!(cx.last_frame().styles.len(), 1);
    assert_eq!(width(&cx, &tip(&cx)), Some(px(9.).into()));

    let request = cx.interactivity("tipped").tooltip.as_ref().unwrap().request;
    cx.tick(vec![
        Event::TooltipRequest {
            request,
            character_index: None,
        },
        Event::Resync,
    ]);
    assert!(cx.last_frame().root.is_some());
    assert_eq!(cx.last_frame().styles.len(), 3);
    assert_eq!(width(&cx, &tip(&cx)), Some(px(9.).into()));
    assert_eq!(width(&cx, cx.root()), Some(px(7.).into()));
}

#[derive(Default, Serialize, Deserialize)]
struct Tracked;
impl View for Tracked {
    const NAME: &'static str = "Tracked";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for Tracked {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(
                div()
                    .id("eyebrow")
                    .letter_spacing(px(0.66))
                    .child("EYEBROW"),
            )
            .child(div().id("title").letter_spacing(px(-0.22)).child("Rooms"))
    }
}

#[test]
fn a_texts_letter_spacing_reaches_the_host() {
    let mut cx = TestAppContext::new();
    cx.open::<Tracked>();
    let spacing = |key| cx.style(key).text.letter_spacing;
    assert_eq!(spacing("eyebrow"), Some(px(0.66)));
    assert_eq!(spacing("title"), Some(px(-0.22)));
}
