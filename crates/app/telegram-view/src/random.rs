//! One device-seeded generator for our requests and grammers' custom backend.
use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use std::cell::RefCell;

thread_local! {
    static RANDOM: RefCell<Option<ChaCha20Rng>> = const { RefCell::new(None) };
}

pub(crate) fn seed(bytes: [u8; 32]) {
    RANDOM.with(|random| *random.borrow_mut() = Some(ChaCha20Rng::from_seed(bytes)));
}

pub(crate) fn bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    fill(&mut bytes).expect("host.random seeds Telegram before any protocol work");
    bytes
}

fn fill(bytes: &mut [u8]) -> Result<(), getrandom::Error> {
    RANDOM.with(|random| {
        let mut random = random.borrow_mut();
        let random = random.as_mut().ok_or(getrandom::Error::UNSUPPORTED)?;
        random.fill_bytes(bytes);
        Ok(())
    })
}

#[cfg(target_arch = "wasm32")]
#[unsafe(export_name = "__getrandom_v03_custom")]
unsafe fn getrandom_0_4_v03_backend_without_os_or_javascript_imports(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // SAFETY: getrandom gives its backend a valid writable buffer for len bytes.
    unsafe { dest.write_bytes(0, len) };
    fill(unsafe { std::slice::from_raw_parts_mut(dest, len) })
}
