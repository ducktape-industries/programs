//! A view on a phone: its page stack, Back, a sheet, and the sweep that
//! holds it to the touch ladder (SPEC §8).
use serde::{Deserialize, Serialize};

use crate::design::{self, CAPSULE_CLEARANCE, Nav, Pages};
use crate::prelude::*;
use crate::testing::{self, TestAppContext};
use crate::{BackEvent, Div, Stateful, View, wire};

#[derive(Serialize, Deserialize)]
struct Phone {
    pages: Pages<String>,
    picking: bool,
    reader: String,
}
impl Default for Phone {
    fn default() -> Self {
        Self {
            pages: Pages::root("rooms".into()),
            picking: false,
            reader: "mina".into(),
        }
    }
}
impl View for Phone {
    const NAME: &'static str = "Phone";
    const CAPABILITIES: &'static [wire::methods::Capability] = &[wire::methods::Capability::Host];
}
impl Render for Phone {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let ladder = design::ladder(window.touch());
        let depth = self.pages.depth();
        let pop = |view: &mut Self, cx: &mut Context<Self>| {
            view.pages.pop();
            cx.notify();
        };
        let nav = match depth {
            0 => Nav::root(self.reader.clone()),
            depth => Nav::back(
                depth,
                cx.listener(move |view, _: &ClickEvent, _, cx| pop(view, cx)),
            ),
        };
        let page = design::page("rooms", self.pages.top().clone(), nav, &ladder, &theme)
            .caption(format!("{} deep", depth))
            .when(depth > 0, |page| {
                page.leading(div().id("avatar").size(ladder.avatar.side).rounded_full())
            })
            .trailing(design::button(
                "new",
                "New",
                &ladder,
                &theme,
                cx.listener(|view, _, _, cx| {
                    view.pages.push("new".into());
                    cx.notify();
                }),
            ))
            .child(design::button(
                "open",
                "Open",
                &ladder,
                &theme,
                cx.listener(|view, _, _, cx| {
                    view.pages.push("room".into());
                    cx.notify();
                }),
            ))
            .child(design::button(
                "pick",
                "Pick",
                &ladder,
                &theme,
                cx.listener(|view, _, _, cx| {
                    view.picking = true;
                    cx.notify();
                }),
            ))
            .children(self.picking.then(|| {
                design::sheet("picker", "Pick one", &ladder, &theme)
                    .on_dismiss(cx.listener(|view, _, _, cx| {
                        view.picking = false;
                        cx.notify();
                    }))
                    .child("options")
                    .into_any_element()
            }));
        div()
            .id("phone")
            .size_full()
            .when(depth > 0, |root| {
                root.on_back(cx.listener(move |view, _: &BackEvent, _, cx| pop(view, cx)))
            })
            .child(page)
    }
}

#[test]
fn a_phone_view_survives_the_sweep() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.back_handler(), None);
        cx.simulate_click("Open");
        assert!(cx.has_text("room"));
        assert!(cx.back_handler().is_some());
        cx.simulate_click("Pick");
        assert!(
            matches!(cx.node("picker"), wire::Node::Sheet { detents, label, .. }
                if detents.is_empty() && label.as_deref() == Some("Pick one"))
        );
        cx.simulate_dismiss("picker");
        assert!(cx.find("picker").is_none());
        cx.simulate_back();
        assert!(cx.has_text("rooms"));
        assert_eq!(cx.back_handler(), None);
    });
}

#[test]
fn a_back_to_no_route_is_ignored() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    cx.simulate_click("Open");
    let handler = cx.back_handler().unwrap();
    cx.tick(vec![wire::Event::Back {
        handler: handler + 1_000,
    }]);
    assert!(cx.has_text("room"));
    cx.tick(vec![wire::Event::Back { handler }]);
    assert!(cx.has_text("rooms"));
}

#[test]
fn page_draws_back_by_the_ladder_and_clears_the_capsule_under_touch() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    assert!(cx.find("rooms-back").is_none(), "no Back at the root");
    cx.simulate_click("Open");
    assert_eq!(
        cx.interactivity("rooms-back").aria.label.as_deref(),
        Some("Back")
    );
    let row = cx.node("rooms-title").children();
    let keys: Vec<_> = row.iter().map(|node| node.key()).collect();
    assert_eq!(keys[..2], [Some("rooms-back"), Some("avatar")]);
    assert_eq!(keys.len(), 4);
    assert!(keys[3].is_some_and(|key| key.ends_with("new")), "{keys:?}");
    let words: Vec<_> = row[2].children().iter().map(|node| node.key()).collect();
    assert_eq!(words, [Some("rooms-heading"), Some("rooms-caption")]);
    assert!(cx.has_text("1 deep"));
    for key in ["rooms-heading", "rooms-caption"] {
        assert_eq!(
            cx.style(key).text.text_overflow,
            Some(gpui::TextOverflow::Truncate("…".into())),
            "{key}"
        );
    }
    let height = |cx: &TestAppContext| cx.style("rooms-title").min_size.height;
    assert_eq!(height(&cx), Some(design::POINTER.row.into()));
    assert_ne!(
        cx.style("rooms-title").padding.right,
        Some(CAPSULE_CLEARANCE.into())
    );
    let renders = cx.renders();
    cx.simulate_touch(true);
    assert_eq!(cx.renders(), renders + 1);
    assert_eq!(height(&cx), Some(design::TOUCH.header.into()));
    assert!(cx.find("rooms-back").is_some());
    assert_eq!(
        cx.style("rooms-title").padding.right,
        Some(CAPSULE_CLEARANCE.into())
    );
    cx.simulate_touch(true);
    assert_eq!(cx.renders(), renders + 1, "the same device is no render");
    cx.open::<Phone>();
    cx.simulate_click("Open");
    assert_eq!(height(&cx), Some(design::TOUCH.header.into()));
}

#[test]
fn under_touch_a_page_crosses_with_its_depth() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    let depth = |cx: &TestAppContext| cx.node("rooms").interactivity().and_then(|i| i.page);
    assert_eq!(depth(&cx), None, "a pointer's page");
    cx.simulate_click("Open");
    assert_eq!(depth(&cx), None, "a pointer's page");
    cx.simulate_back();
    cx.simulate_touch(true);
    assert_eq!(depth(&cx), Some(0));
    cx.simulate_click("Open");
    assert_eq!(depth(&cx), Some(1));
    cx.simulate_click("New");
    assert_eq!(depth(&cx), Some(2));
    cx.simulate_back();
    assert_eq!(depth(&cx), Some(1));
}

#[test]
fn under_a_pointer_a_root_page_is_drawn_as_any_page() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    let row = |cx: &TestAppContext| {
        let row = cx.style("rooms-title");
        let heading = cx.style("rooms-heading").text.clone();
        (row.border_widths.bottom, row.min_size.height, heading)
    };
    let root = row(&cx);
    cx.simulate_click("Open");
    assert!(cx.find("rooms-back").is_some(), "a page under the root");
    assert_eq!(row(&cx), root);
}

#[test]
fn under_touch_a_page_is_the_expo_header() {
    let mut cx = TestAppContext::new();
    cx.open::<Phone>();
    cx.simulate_touch(true);
    let text = |cx: &TestAppContext, key: &str| cx.style(key).text.clone();
    let root = text(&cx, "rooms-heading");
    assert_eq!(root.font_size, Some(px(22.).into()));
    assert_eq!(root.line_height, Some(px(27.).into()));
    assert_eq!(root.letter_spacing, Some(px(-0.22)));
    assert_eq!(root.font_weight, Some(gpui::FontWeight::MEDIUM));
    let hairline = |cx: &TestAppContext| {
        let row = cx.style("rooms-title");
        (row.border_widths.bottom, row.border_color)
    };
    let clear = crate::Hsla::transparent_black();
    assert_eq!(hairline(&cx), (Some(px(1.).into()), Some(clear)));
    cx.simulate_click("Open");
    let detail = text(&cx, "rooms-heading");
    assert_eq!(detail.font_size, Some(px(17.).into()));
    assert_eq!(detail.line_height, Some(px(21.).into()));
    assert_eq!(detail.letter_spacing, None);
    let theme = crate::Theme::light();
    assert_eq!(hairline(&cx), (Some(px(1.).into()), Some(theme.border)));
    assert_eq!(
        cx.style("rooms-title").min_size.height,
        Some(px(56.).into())
    );
    let back = cx.style("rooms-back");
    assert_eq!(
        (back.size.width, back.size.height),
        (Some(px(44.).into()), Some(px(44.).into()))
    );
    let arrow = &cx.node("rooms-back").children()[0];
    assert!(
        matches!(arrow, wire::Node::Svg { source: wire::SvgSource::Asset(path), .. }
            if path == "icons/arrow-left.svg"),
        "{arrow:?}"
    );
}

#[test]
fn a_first_page_leads_with_the_readers_account() {
    let mut cx = TestAppContext::new();
    let phone = cx.open::<Phone>();
    assert!(cx.find("rooms-account").is_none(), "none under a pointer");
    cx.simulate_touch(true);
    let row = cx.node("rooms-title").children();
    assert_eq!(row[0].key(), Some("rooms-account"));
    let account = cx.interactivity("rooms-account");
    assert_eq!(account.role, Some(crate::Role::Button));
    assert_eq!(account.aria.label.as_deref(), Some("Account"));
    assert_eq!(account.active, None, "no dim on a press");
    let hit = cx.style("rooms-account");
    assert_eq!(
        (hit.size.width, hit.size.height),
        (Some(px(44.).into()), Some(px(44.).into()))
    );
    let avatar = cx.node("rooms-account").children()[0].style().unwrap();
    assert_eq!(cx.styles()[avatar].size.width, Some(px(32.).into()));
    assert!(cx.has_text("M"));
    cx.simulate_click("Account");
    assert_eq!(cx.host().requests::<HostShow>(), [Place::Account]);
    cx.update(&phone, |phone, _, cx| {
        phone.reader.clear();
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("•"));
    cx.simulate_click("Open");
    assert!(cx.find("rooms-account").is_none(), "none after Back");
}

#[derive(Default, Serialize, Deserialize)]
struct Cramped;
impl View for Cramped {
    const NAME: &'static str = "Cramped";
}
impl Render for Cramped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("short")
            .h(px(20.))
            .role(crate::Role::Button)
            .focusable()
            .on_click(|_, _, _| {})
            .child("Tap")
    }
}

#[test]
#[should_panic(expected = "declares no height of at least 44")]
fn a_short_press_target_does_not_fit_a_phone() {
    let mut cx = TestAppContext::new();
    cx.open::<Cramped>();
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

fn chip(ladder: Option<&design::Ladder>) -> Stateful<Div> {
    let chip = div().id("chip").h(px(30.)).min_w(px(40.)).child("👍 2");
    match ladder {
        Some(ladder) => chip.child(
            design::hit("react", px(40.), px(30.), px(0.), ladder)
                .role(crate::Role::Button)
                .aria_label("👍 2")
                .focusable()
                .on_click(|_, _, _| {}),
        ),
        None => chip
            .role(crate::Role::Button)
            .aria_label("👍 2")
            .focusable()
            .on_click(|_, _, _| {}),
    }
}

#[derive(Default, Serialize, Deserialize)]
struct HitChip;
impl View for HitChip {
    const NAME: &'static str = "HitChip";
}
impl Render for HitChip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let ladder = design::ladder(window.touch());
        div().id("root").child(chip(Some(&ladder)))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct BareChip;
impl View for BareChip {
    const NAME: &'static str = "BareChip";
}
impl Render for BareChip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("root").child(chip(None))
    }
}

#[test]
fn a_short_chip_fits_a_phone_through_a_hit() {
    let mut cx = TestAppContext::new();
    cx.open::<HitChip>();
    let inset = |cx: &TestAppContext| {
        let style = cx.style("react");
        [
            style.inset.top,
            style.inset.bottom,
            style.inset.left,
            style.inset.right,
        ]
    };
    assert_eq!(inset(&cx), [Some(px(0.).into()); 4]);
    testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.style("chip").size.height, Some(px(30.).into()));
        let [top, bottom, left, right] = inset(cx);
        assert_eq!((top, bottom), (Some(px(-7.).into()), Some(px(-7.).into())));
        assert_eq!((left, right), (Some(px(-2.).into()), Some(px(-2.).into())));
    });
}

#[test]
#[should_panic(expected = "declares no height of at least 44")]
fn a_short_chip_without_a_hit_does_not_fit_a_phone() {
    let mut cx = TestAppContext::new();
    cx.open::<BareChip>();
    cx.simulate_touch(true);
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

fn loose_chip(row: f32) -> Stateful<Div> {
    div().id("row").h(px(row)).child(
        div().id("chip").min_w(px(40.)).child("👍 2").child(
            design::hit("react", px(40.), px(30.), px(0.), &design::TOUCH)
                .role(crate::Role::Button)
                .aria_label("👍 2")
                .focusable()
                .on_click(|_, _, _| {}),
        ),
    )
}

#[derive(Default, Serialize, Deserialize)]
struct LooseChip<const ROW: u16>;
impl<const ROW: u16> View for LooseChip<ROW> {
    const NAME: &'static str = "LooseChip";
}
impl<const ROW: u16> Render for LooseChip<ROW> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        loose_chip(f32::from(ROW))
    }
}

#[test]
#[should_panic(expected = "a press box reaches past a parent that declares no height")]
fn a_hit_in_a_chip_of_no_declared_height_does_not_fit_in_a_tall_row() {
    let mut cx = TestAppContext::new();
    cx.open::<LooseChip<48>>();
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

#[test]
#[should_panic(expected = "a press box reaches past a parent that declares no height")]
fn a_hit_in_a_chip_of_no_declared_height_does_not_fit_in_a_short_row() {
    let mut cx = TestAppContext::new();
    cx.open::<LooseChip<30>>();
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

#[derive(Default, Serialize, Deserialize)]
struct NoWayBack;
impl View for NoWayBack {
    const NAME: &'static str = "NoWayBack";
}
impl Render for NoWayBack {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("root")
            .child(crate::sheet("picker", "Pick one").child("options"))
    }
}

#[test]
#[should_panic(expected = "a sheet is open and the root has no on_back")]
fn a_sheet_without_a_root_back_does_not_fit_a_phone() {
    let mut cx = TestAppContext::new();
    cx.open::<NoWayBack>();
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

#[derive(Default, Serialize, Deserialize)]
struct BackToNothing;
impl View for BackToNothing {
    const NAME: &'static str = "BackToNothing";
}
impl Render for BackToNothing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("root")
            .on_back(|_: &BackEvent, _, _| {})
            .child("home")
    }
}

#[test]
#[should_panic(expected = "the root has on_back with no sheet open and no Back control drawn")]
fn a_root_back_with_nowhere_to_go_does_not_fit_a_phone() {
    let mut cx = TestAppContext::new();
    cx.open::<BackToNothing>();
    testing::assert_fits(cx.root(), cx.styles(), cx.dense(), 390., &design::TOUCH);
}

fn fits_in_a_tall_row(press: impl IntoElement) {
    let (tree, styles, dense) = testing::lower(|_: &mut Context<()>| {
        div()
            .id("row")
            .h(px(48.))
            .child(div().id("chip").child(press))
    });
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

fn press(element: Stateful<Div>) -> Stateful<Div> {
    element
        .role(crate::Role::Button)
        .aria_label("React")
        .on_click(|_, _, _| {})
}

fn line() -> Stateful<Div> {
    press(div().id("line").h(px(20.)))
}

#[test]
fn the_sweep_lets_a_press_marked_dense_keep_its_own_height() {
    let (tree, styles, dense) =
        testing::lower(|_: &mut Context<()>| div().id("diff").child(design::dense(line())));
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

#[test]
#[should_panic(expected = "declares no height of at least 44")]
fn a_press_inside_a_dense_row_is_still_held_to_44() {
    let (tree, styles, dense) = testing::lower(|_: &mut Context<()>| {
        div()
            .id("diff")
            .child(design::dense(line()).child(press(div().id("chip").h(px(20.)))))
    });
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

#[test]
#[should_panic(expected = "declares no height of at least 44")]
fn the_sweep_refuses_a_20_tall_press_not_marked_dense() {
    let (tree, styles, dense) =
        testing::lower(|_: &mut Context<()>| div().id("diff").child(line()));
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

#[test]
fn the_sweep_trusts_a_hit_told_its_parent_is_a_finger_tall() {
    fits_in_a_tall_row(press(design::hit(
        "react",
        px(44.),
        px(44.),
        px(0.),
        &design::TOUCH,
    )));
}

#[test]
fn the_sweep_trusts_a_box_whose_insets_add_up_to_zero() {
    fits_in_a_tall_row(press(
        div().id("react").absolute().top(px(-12.)).bottom(px(12.)),
    ));
}

#[test]
fn the_sweep_trusts_a_box_pinned_by_one_inset() {
    fits_in_a_tall_row(press(div().id("react").absolute().top(px(-7.))));
}

#[test]
fn the_sweep_trusts_a_press_with_no_height_of_its_own() {
    fits_in_a_tall_row(press(div().id("react")));
}

#[test]
#[should_panic(expected = "declare the parent's height")]
fn the_sweep_refuses_a_hit_whose_parent_fills_its_row_by_layout() {
    let (tree, styles, dense) = testing::lower(|_: &mut Context<()>| {
        div()
            .id("row")
            .h(px(48.))
            .child(div().id("chip").h_full().child(press(design::hit(
                "react",
                px(44.),
                px(30.),
                px(0.),
                &design::TOUCH,
            ))))
    });
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

fn bordered_chip(edge: Pixels) -> Stateful<Div> {
    div().id("row").h(px(48.)).child(
        div()
            .id("chip")
            .relative()
            .h(px(32.))
            .min_w(px(44.))
            .border_1()
            .child("👍 2")
            .child(press(design::hit(
                "react",
                px(44.),
                px(32.),
                edge,
                &design::TOUCH,
            ))),
    )
}

#[test]
fn a_bordered_chip_presses_at_44_through_a_hit_told_its_edge() {
    let (tree, styles, dense) = testing::lower(|_: &mut Context<()>| bordered_chip(px(1.)));
    let hit = &tree.children()[0].children()[1];
    let inset = &styles[hit.style().expect("a styled hit")].inset;
    assert_eq!(
        (inset.top, inset.bottom),
        (Some(px(-7.).into()), Some(px(-7.).into()))
    );
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}

#[test]
#[should_panic(expected = "comes to 42")]
fn a_bordered_chip_whose_hit_is_not_told_its_edge_does_not_fit() {
    let (tree, styles, dense) = testing::lower(|_: &mut Context<()>| bordered_chip(px(0.)));
    testing::assert_fits(&tree, &styles, &dense, 390., &design::TOUCH);
}
