//! A reply's bytes are copied once on the way in: decoding a 50,000-byte
//! reply allocates the bytes it keeps, not a buffer to read them through
//! and then the bytes. Measured with a counting allocator.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use view_wire::{ElementIdWire, Event, decode, encode};

struct Counting;
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATED.fetch_add(size.saturating_sub(layout.size()), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

const BYTES: usize = 50_000;

#[test]
fn a_reply_is_copied_once_on_the_way_in() {
    let reply = Event::Response {
        id: 1,
        result: Ok((0..BYTES).map(|at| at as u8).collect()),
        done: true,
    };
    let wire = encode(&reply);
    let before = ALLOCATED.load(Ordering::Relaxed);
    let decoded = decode::<Event>(&wire).unwrap();
    let allocated = ALLOCATED.load(Ordering::Relaxed) - before;
    assert_eq!(decoded, reply);
    assert!(
        (BYTES..BYTES + 1024).contains(&allocated),
        "decoding {BYTES} bytes of reply allocated {allocated}"
    );

    let before = ALLOCATED.load(Ordering::Relaxed);
    let refused = decode::<Event>(&wire[..wire.len() / 2]).is_err();
    let allocated = ALLOCATED.load(Ordering::Relaxed) - before;
    assert!(refused);
    assert!(allocated < 1024, "a refused reply allocated {allocated}");

    let mut wire = vec![0x81, 0x03, 0xc6, 0x00, 0x40, 0x00, 0x00];
    wire.resize(wire.len() + (4 << 20), 9);
    let before = ALLOCATED.load(Ordering::Relaxed);
    let refused = decode::<ElementIdWire>(&wire).is_err();
    let allocated = ALLOCATED.load(Ordering::Relaxed) - before;
    assert!(refused);
    assert!(allocated < 1024, "a refused id allocated {allocated}");
}
