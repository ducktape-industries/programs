//! Memos and cached views: a subtree kept while what it reads holds, its
//! routes still answering, and lowered again when that changes or when a
//! debug build finds a reuse that would draw something else.
use super::*;

#[derive(Default, Serialize, Deserialize)]
struct Rows {
    labels: Vec<String>,
    hovered: Option<usize>,
    pressed: Vec<usize>,
    loose: bool,
}
impl View for Rows {
    const NAME: &'static str = "Rows";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            labels: (0..20).map(|n| format!("row {n}")).collect(),
            ..Self::default()
        }
    }
}
impl Render for Rows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.labels.iter().enumerate().map(|(index, label)| {
            let hovered = self.hovered == Some(index);
            let read = match self.loose {
                true => String::new(),
                false => label.clone(),
            };
            let label = label.clone();
            let press = cx.listener(move |rows: &mut Self, _: &ClickEvent, _, cx| {
                rows.pressed.push(index);
                cx.notify();
            });
            let hover = cx.listener(move |rows: &mut Self, over: &bool, _, cx| {
                rows.hovered = over.then_some(index);
                cx.notify();
            });
            memo(format!("row-{index}"), (read, hovered), move |_, _| {
                div()
                    .id(format!("row-{index}"))
                    .role(Role::Button)
                    .focusable()
                    .on_click(press)
                    .on_hover(hover)
                    .when(hovered, |row| row.bg(hsla(0., 1., 0.5, 1.)))
                    .child(label)
            })
        });
        div().id("rows").children(rows)
    }
}

#[test]
fn a_hover_lowers_only_the_row_it_changed() {
    let mut cx = TestAppContext::new();
    cx.open::<Rows>();
    cx.memo_checks(false);
    let whole = cx.root().count();
    cx.simulate_hover("row-3", true);
    let tick = cx.reports()[0];
    assert!(tick.rendered);
    assert_eq!(
        tick.lowered, 3,
        "the column, row 3 and its text, of {whole}"
    );
    assert_eq!(cx.memos_lowered(), ["row-3"]);
    assert!(cx.style("row-3").background.is_some());
    assert!(cx.style("row-4").background.is_none());
    cx.simulate_hover("row-3", false);
    assert_eq!(cx.reports()[0].lowered, 3);
    assert!(cx.style("row-3").background.is_none());
}

#[test]
fn a_kept_row_keeps_its_routes_and_they_answer() {
    let mut cx = TestAppContext::new();
    let rows = cx.open::<Rows>();
    cx.memo_checks(false);
    let route = cx.interactivity("row-7").on_click;
    for at in [1, 2, 3] {
        cx.simulate_hover(&format!("row-{at}"), true);
    }
    assert_eq!(cx.interactivity("row-7").on_click, route);
    cx.simulate_click("row-7");
    rows.read(|rows| assert_eq!(rows.pressed, [7]));
}

#[test]
fn a_memo_whose_deps_leave_out_its_label_keeps_the_old_one() {
    let mut cx = TestAppContext::new();
    let rows = cx.open::<Rows>();
    cx.memo_checks(false);
    loosen_and_rename(&mut cx, &rows);
    assert!(cx.has_text("row 2") && !cx.has_text("renamed"));
}

#[test]
#[should_panic(expected = "draws something else")]
fn a_memo_whose_deps_leave_out_its_label_fails_a_debug_build() {
    let mut cx = TestAppContext::new();
    let rows = cx.open::<Rows>();
    loosen_and_rename(&mut cx, &rows);
}

fn loosen_and_rename(cx: &mut TestAppContext, rows: &Entity<Rows>) {
    cx.update(rows, |rows, _, cx| {
        rows.loose = true;
        cx.notify();
    });
    cx.run_until_parked();
    cx.update(rows, |rows, _, cx| {
        rows.labels[2] = "renamed".into();
        cx.notify();
    });
    cx.run_until_parked();
}

#[derive(Default, Serialize, Deserialize)]
struct Wrapped {
    wrapped: bool,
    count: usize,
}
impl View for Wrapped {
    const NAME: &'static str = "Wrapped";
}
impl Render for Wrapped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let row = memo("row", (), |_, _| {
            div().id("row").child(div().id("inner").child("kept"))
        });
        let row = match self.wrapped {
            true => div().child(row).into_any_element(),
            false => row.into_any_element(),
        };
        div().id("page").child(row).child(self.count.to_string())
    }
}

fn wrap_and_unwrap(checks: bool) {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Wrapped>();
    cx.memo_checks(checks);
    for wrapped in [true, false] {
        cx.update(&view, |view, _, cx| {
            view.wrapped = wrapped;
            view.count += 1;
            cx.notify();
        });
        cx.run_until_parked();
        let count = view.read(|view| view.count.to_string());
        assert!(
            cx.has_text("kept") && cx.has_text(&count),
            "{:?}",
            cx.texts()
        );
        assert!(cx.memos_lowered().is_empty(), "the row was kept");
    }
}

#[test]
fn a_kept_row_moved_under_a_wrapper_comes_from_the_last_tree() {
    wrap_and_unwrap(false);
    wrap_and_unwrap(true);
}

fn page(row: bool) -> wire::Node {
    let page = div().id("page").child(div().id("other").child("other"));
    let page = match row {
        true => page.child(div().id("row").child("kept")),
        false => page,
    };
    crate::testing::lower::<(), _>(move |_| page).0
}

fn kept_row() -> (wire::Node, Vec<crate::memo::Hit>) {
    let mut new = page(true);
    let shell = crate::memo::shell(&mut new.children_mut()[1]);
    new.children_mut()[1] = shell;
    let hit = crate::memo::Hit {
        path: vec![
            wire::ElementIdWire::Name("page".into()),
            wire::ElementIdWire::Name("row".into()),
        ],
        id: crate::memo::MemoId::Named(wire::ElementIdWire::Name("row".into())),
        check: None,
    };
    (new, vec![hit])
}

#[test]
fn a_kept_row_takes_the_last_trees_node_and_sends_nothing() {
    let (mut new, hits) = kept_row();
    let patches = crate::memo::keeping(&mut page(true), &mut new, &hits, true);
    assert_eq!(patches, Some(Vec::new()));
    assert_eq!(new, page(true));
}

#[test]
#[should_panic(expected = "memo row at")]
fn a_kept_row_the_last_tree_lacks_fails_with_checks() {
    let (mut new, hits) = kept_row();
    crate::memo::keeping(&mut page(false), &mut new, &hits, true);
}

#[test]
fn a_kept_row_the_last_tree_lacks_draws_the_frame_whole() {
    let (mut new, hits) = kept_row();
    assert!(crate::memo::keeping(&mut page(false), &mut new, &hits, false).is_none());
}

#[test]
fn a_new_theme_lowers_every_memo_again() {
    let mut cx = TestAppContext::new();
    cx.open::<Rows>();
    cx.memo_checks(false);
    let whole = cx.root().count();
    cx.set_global(crate::Theme::dark());
    assert_eq!(cx.reports()[0].lowered, whole);
}

#[derive(Default, Serialize, Deserialize)]
struct Framed {
    count: usize,
    #[serde(skip)]
    picture: Option<Arc<Image>>,
}
impl View for Framed {
    const NAME: &'static str = "Framed";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        let picture = Image::from_bytes(ImageFormat::Png, vec![7; 64]);
        Self {
            count: 0,
            picture: Some(Arc::new(picture)),
        }
    }
}
impl Render for Framed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let picture = self.picture.clone().expect("built in new");
        let framed = memo("frame", (), move |_, _| {
            div().id("frame").child(img(picture))
        });
        div()
            .id("framed")
            .child(framed)
            .child(self.count.to_string())
    }
}

fn pictures_carried(frame: &wire::Frame) -> usize {
    let mut nodes: Vec<wire::Node> = frame.root.iter().cloned().collect();
    nodes.extend(frame.patches.iter().filter_map(|patch| match patch {
        wire::Patch::Replace { node, .. } | wire::Patch::Insert { node, .. } => Some(node.clone()),
        _ => None,
    }));
    let mut carried = 0;
    for mut node in nodes {
        node.for_each_mut(&mut |node| {
            if let wire::Node::Image { data: Some(_), .. } = node {
                carried += 1;
            }
        });
    }
    carried
}

#[test]
fn a_kept_picture_crosses_once_and_again_after_a_resync() {
    let mut cx = TestAppContext::new();
    let framed = cx.open::<Framed>();
    cx.memo_checks(false);
    assert!(cx.find("frame").is_some());
    cx.update(&framed, |framed, _, cx| {
        framed.count += 1;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(pictures_carried(cx.last_frame()), 0);
    assert!(cx.has_text("1"));
    cx.simulate_resync();
    assert_eq!(pictures_carried(cx.last_frame()), 1, "the host lost it");
}

struct Counter {
    name: &'static str,
    count: usize,
}
impl Render for Counter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(self.name)
            .child(format!("{} {}", self.name, self.count))
    }
}

struct Boxed {
    inner: Entity<Counter>,
}
impl Render for Boxed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("boxed")
            .child(AnyView::from(self.inner.clone()).cached())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Pair {
    #[serde(skip)]
    left: Option<Entity<Counter>>,
    #[serde(skip)]
    right: Option<Entity<Boxed>>,
    #[serde(skip)]
    inner: Option<Entity<Counter>>,
}
impl View for Pair {
    const NAME: &'static str = "Pair";
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let inner = cx.new(|_| Counter {
            name: "inner",
            count: 0,
        });
        self.left = Some(cx.new(|_| Counter {
            name: "left",
            count: 0,
        }));
        self.right = Some(cx.new(|_| Boxed {
            inner: inner.clone(),
        }));
        self.inner = Some(inner);
    }
}
impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let left = self.left.clone().expect("attached");
        let right = self.right.clone().expect("attached");
        div()
            .id("pair")
            .child(AnyView::from(left).cached())
            .child(AnyView::from(right).cached())
    }
}

fn bump(cx: &mut TestAppContext, counter: &Entity<Counter>) {
    cx.update(counter, |counter, _, cx| {
        counter.count += 1;
        cx.notify();
    });
    cx.run_until_parked();
}

#[test]
fn a_cached_view_lowers_again_only_when_it_or_one_inside_it_notified() {
    let mut cx = TestAppContext::new();
    let pair = cx.open::<Pair>();
    cx.memo_checks(false);
    let (left, inner) = pair.read(|pair| (pair.left.clone(), pair.inner.clone()));
    let (left, inner) = (left.unwrap(), inner.unwrap());
    bump(&mut cx, &left);
    assert!(cx.has_text("left 1"));
    assert_eq!(cx.reports()[0].lowered, 3);
    bump(&mut cx, &inner);
    assert!(cx.has_text("inner 1"), "{:?}", cx.texts());
    assert_eq!(cx.reports()[0].lowered, 4);
}

#[test]
fn cached_views_pass_the_debug_check() {
    let mut cx = TestAppContext::new();
    let pair = cx.open::<Pair>();
    let inner = pair.read(|pair| pair.inner.clone()).unwrap();
    bump(&mut cx, &inner);
    bump(&mut cx, &inner);
    assert!(cx.has_text("inner 2") && cx.has_text("left 0"));
}
