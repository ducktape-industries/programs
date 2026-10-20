/// A tiny deterministic PRNG so the property tests need no new dependency.
/// splitmix64: https://prng.di.unimi.it/splitmix64.c
///
/// The flag makes [`gen_id`] draw a host-local id now and then.
pub(super) struct Rng(u64, bool);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed, false)
    }

    pub(super) fn poisoning_ids(seed: u64) -> Self {
        Self(seed, true)
    }

    pub(super) fn poisons_ids(&self) -> bool {
        self.1
    }

    pub(super) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(super) fn next_range(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() as usize) % bound
    }

    pub(super) fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    pub(super) fn next_unit(&mut self) -> f64 {
        ((self.next_u64() >> 40) as f64) / ((1u64 << 24) as f64)
    }

    pub(super) fn skewed(&mut self, max: usize, exponent: i32) -> usize {
        if max == 0 {
            return 0;
        }
        let biased = self.next_unit().powi(exponent);
        ((biased * max as f64) as usize).min(max)
    }

    pub(super) fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_range(items.len())]
    }
}
