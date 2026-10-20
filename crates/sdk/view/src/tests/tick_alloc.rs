//! The production tick moves a frame's subtrees out of the kept tree and
//! back instead of cloning them. Counted per thread, so the tests the
//! binary runs beside this one do not add to the measurement.
use super::*;
use std::alloc::{GlobalAlloc, Layout, System};

struct Counting;
thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATED.try_with(|count| count.set(count.get() + layout.size()));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocated_by<R>(work: impl FnOnce() -> R) -> (usize, R) {
    let before = ALLOCATED.with(Cell::get);
    let result = work();
    (ALLOCATED.with(Cell::get) - before, result)
}

fn text(content: &str) -> wire::Node {
    wire::Node::Text(wire::TextNode {
        id: None,
        style: wire::StyleId(0),
        content: content.into(),
        transform: None,
    })
}

fn keyed(key: &str, content: &str) -> wire::Node {
    let mut node = text(content);
    let wire::Node::Text(wire::TextNode { id, .. }) = &mut node else {
        unreachable!()
    };
    *id = Some(wire::ElementIdWire::Name(key.into()));
    node
}

fn column(children: Vec<wire::Node>) -> wire::Node {
    wire::Node::Container(wire::ContainerNode {
        id: None,
        style: wire::StyleId(0),
        interactivity: Default::default(),
        children,
    })
}

#[test]
fn put_back_completes_the_tree_a_taking_diff_hollowed() {
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
    let mut hollow = new.clone();
    let mut patches = wire::diff_taking(&mut old.clone(), &mut hollow);
    assert_ne!(hollow, new, "{patches:#?}");
    patches.reverse();
    crate::driver::put_back(&mut hollow, patches);
    assert_eq!(hollow, new);
}

#[derive(Default, Serialize, Deserialize)]
struct Pages {
    rows: bool,
    keyed_chrome: bool,
}
impl View for Pages {
    const NAME: &'static str = "Pages";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            rows: false,
            keyed_chrome: true,
        }
    }
}
impl Render for Pages {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let page = match self.rows {
            false => div().id("overview").child("nothing here"),
            true => div().id("rows").children((0..500).map(|row| {
                div()
                    .id(format!("row/{row}"))
                    .child(format!("row {row}"))
                    .child("…")
            })),
        };
        match self.keyed_chrome {
            true => div()
                .id("shell")
                .child(div().id("title").child("title"))
                .child(page)
                .child(div().id("footer").child("footer")),
            false => div().id("shell").child("title").child(page).child("footer"),
        }
    }
}

fn notify(driver: &mut Driver<Pages>, change: impl FnOnce(&mut Pages)) {
    driver.entity().update(driver.app_mut(), |view, cx| {
        change(view);
        cx.notify();
    });
}

fn a_switch_tick_does_not_copy_the_rows(keyed_chrome: bool) {
    let mut driver = Driver::<Pages>::new();
    notify(&mut driver, |view| view.keyed_chrome = keyed_chrome);
    driver.tick_with(vec![], |_| ());
    let (switch, ()) = allocated_by(|| {
        notify(&mut driver, |view| view.rows = true);
        driver.tick_with(vec![], |_| ())
    });
    let (copy, rows) = allocated_by(|| driver.last_root.clone());
    let (render, ()) = allocated_by(|| {
        notify(&mut driver, |_| {});
        driver.tick_with(vec![], |_| ())
    });
    assert_eq!(driver.last_root, rows, "the kept tree is the rendered page");
    assert!(
        switch < render + copy / 2,
        "keyed_chrome={keyed_chrome}: the switch tick allocated {switch} bytes, an unchanged \
         re-render {render}, a copy of the kept tree {copy}"
    );
}

#[test]
fn a_patch_frame_switch_does_not_copy_the_rows() {
    a_switch_tick_does_not_copy_the_rows(true);
}

#[test]
fn a_whole_frame_switch_does_not_copy_the_rows() {
    a_switch_tick_does_not_copy_the_rows(false);
}
