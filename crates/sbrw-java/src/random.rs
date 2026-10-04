//! `java.util.Random`: the 48-bit linear congruential generator from the JDK.
//!
//! Every random roll in the core (loot, card packs, lobby order) draws from one
//! of these, so seeded replays of reference recordings need the exact sequence.

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    /// `new Random(seed)`.
    pub fn new(seed: i64) -> Self {
        Self { seed: (seed as u64 ^ MULTIPLIER) & MASK }
    }

    /// `Random.next(bits)`: the top `bits` bits of the next 48-bit state.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND)) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// `nextInt()`.
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`. Panics if `bound <= 0`, where Java throws.
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive, got {bound}");
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            // Power of two: take the high bits.
            return ((i64::from(bound) * i64::from(r)) >> 31) as i32;
        }
        // Reject values from the incomplete final bucket. The check relies on
        // i32 overflow wrapping negative, exactly as in Java.
        let mut u = r;
        loop {
            r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
        }
    }

    /// `nextLong()`.
    pub fn next_long(&mut self) -> i64 {
        let high = i64::from(self.next(32)) << 32;
        high.wrapping_add(i64::from(self.next(32)))
    }

    /// `nextBoolean()`.
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    /// `nextFloat()`.
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    /// `nextDouble()`, which is also what `Math.random()` returns.
    pub fn next_double(&mut self) -> f64 {
        let high = i64::from(self.next(26)) << 27;
        (high + i64::from(self.next(27))) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `Collections.shuffle(list, random)`.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (2..=items.len()).rev() {
            let j = self.next_int_bounded(i as i32) as usize;
            items.swap(i - 1, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        seed: String,
        next_int: Vec<i32>,
        /// Each entry is `[bound, draw, draw, ...]` from one continuing generator.
        next_int_bounded: Vec<Vec<i32>>,
        next_long: Vec<String>,
        next_double_bits: Vec<String>,
        next_float_bits: Vec<String>,
        next_boolean: Vec<bool>,
        shuffle_0_to_9: Vec<i32>,
        after_shuffle_next_int: i32,
    }

    fn vectors() -> Vectors {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/java_random.json");
        let text = std::fs::read_to_string(path).expect("run `reference/record.sh vectors` to generate the vectors");
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn matches_the_jvm() {
        let vectors = vectors();
        assert!(!vectors.cases.is_empty());
        for case in vectors.cases {
            let seed: i64 = case.seed.parse().unwrap();
            let fresh = || JavaRandom::new(seed);

            let mut r = fresh();
            let ints: Vec<i32> = case.next_int.iter().map(|_| r.next_int()).collect();
            assert_eq!(ints, case.next_int, "nextInt, seed {seed}");

            let mut r = fresh();
            for draws in &case.next_int_bounded {
                let bound = draws[0];
                let got: Vec<i32> = draws[1..].iter().map(|_| r.next_int_bounded(bound)).collect();
                assert_eq!(got, draws[1..], "nextInt({bound}), seed {seed}");
            }

            let mut r = fresh();
            for expected in &case.next_long {
                assert_eq!(r.next_long().to_string(), *expected, "nextLong, seed {seed}");
            }

            let mut r = fresh();
            for expected in &case.next_double_bits {
                assert_eq!(format!("{:x}", r.next_double().to_bits()), *expected, "nextDouble, seed {seed}");
            }

            let mut r = fresh();
            for expected in &case.next_float_bits {
                assert_eq!(format!("{:x}", r.next_float().to_bits()), *expected, "nextFloat, seed {seed}");
            }

            let mut r = fresh();
            let bools: Vec<bool> = case.next_boolean.iter().map(|_| r.next_boolean()).collect();
            assert_eq!(bools, case.next_boolean, "nextBoolean, seed {seed}");

            let mut r = fresh();
            let mut list: Vec<i32> = (0..10).collect();
            r.shuffle(&mut list);
            assert_eq!(list, case.shuffle_0_to_9, "shuffle, seed {seed}");
            assert_eq!(r.next_int(), case.after_shuffle_next_int, "nextInt after shuffle, seed {seed}");
        }
    }

    #[test]
    #[should_panic(expected = "bound must be positive")]
    fn non_positive_bound_panics() {
        JavaRandom::new(1).next_int_bounded(0);
    }
}
