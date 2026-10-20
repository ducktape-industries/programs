//! `diff_taking` carries a replaced subtree by moving it: a diff that
//! replaces most of a tree allocates patches and paths, not a copy of the
//! tree. Measured with a counting allocator, so it holds whatever the
//! machine is doing.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use view_wire::{ContainerNode, Node, TextNode};

mod common;
use common::PLAIN;

struct Counting;
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn text(content: &str) -> Node {
    Node::Text(TextNode {
        id: None,
        style: PLAIN,
        content: content.into(),
        transform: None,
    })
}

fn column(children: Vec<Node>) -> Node {
    Node::Container(ContainerNode {
        id: None,
        style: PLAIN,
        interactivity: Default::default(),
        children,
    })
}

fn shell(page: Node) -> Node {
    column(vec![text("title"), page, text("footer")])
}

fn pages() -> (Node, Node) {
    const ROWS: usize = 500;
    let old = shell(column(vec![text("no rows yet")]));
    let new = shell(column(
        (0..ROWS)
            .map(|row| column(vec![text(&format!("row {row}")), text("…")]))
            .collect(),
    ));
    (old, new)
}

fn allocated_by(diff: impl FnOnce() -> Vec<view_wire::Patch>) -> (usize, usize) {
    let before = ALLOCATED.load(Ordering::Relaxed);
    let patches = diff();
    (ALLOCATED.load(Ordering::Relaxed) - before, patches.len())
}

#[test]
fn a_taking_diff_of_a_replaced_page_does_not_copy_the_rows() {
    let (mut old, mut new) = pages();
    let (copying, patches) = allocated_by(|| view_wire::diff(&mut old, &mut new));
    let (mut old, mut new) = pages();
    let (taking, same) = allocated_by(|| view_wire::diff_taking(&mut old, &mut new));
    assert_eq!((patches, same), (501, 501));
    let rows_children = 500 * 2 * std::mem::size_of::<Node>();
    assert!(
        copying >= taking + rows_children,
        "diff allocated {copying} bytes, diff_taking {taking}: a copy of the rows' \
         children alone is {rows_children}"
    );
}
