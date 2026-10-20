//! view-wire's `decode` of one frame, the bytes a view hands its host each tick.
#![no_main]

use libfuzzer_sys::fuzz_target;
use view_wire::Frame;

fuzz_target!(|data: &[u8]| {
    let _ = view_wire::decode::<Frame>(data);
});
