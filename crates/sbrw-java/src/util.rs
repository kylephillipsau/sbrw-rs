//! `java.util` behaviour that decides what the port does.

use crate::lang::string_hash_code;

/// The order a `java.util.HashMap<String, _>` iterates keys put in this order
/// (distinct keys, default capacity and load factor, no removals): by bucket,
/// then by insertion within a bucket. Returns indexes into `keys`.
///
/// Exact while no bucket collects 8 keys, where the JDK would resize or build a
/// tree instead of a list.
pub fn hash_map_order<S: AsRef<str>>(keys: &[S]) -> Vec<usize> {
    // The table starts at 16 buckets and doubles each time the size passes
    // three quarters of it. Splitting a bucket keeps the relative order.
    let mut capacity = 16usize;
    while keys.len() > capacity / 4 * 3 {
        capacity *= 2;
    }
    let bucket = |key: &str| {
        let h = string_hash_code(key);
        (h ^ ((h as u32) >> 16) as i32) as u32 as usize & (capacity - 1)
    };
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| bucket(keys[i].as_ref()));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iterate<'a>(keys: &[&'a str]) -> Vec<&'a str> {
        hash_map_order(keys).into_iter().map(|i| keys[i]).collect()
    }

    #[test]
    fn matches_the_jdk() {
        // keySet() orders printed by JDK 11's jshell.
        assert_eq!(iterate(&["LOGIN"]), ["LOGIN"]);
        assert_eq!(iterate(&["EVENT", "PROGRESSION"]), ["EVENT", "PROGRESSION"]);
        assert_eq!(iterate(&["PROGRESSION", "EVENT"]), ["PROGRESSION", "EVENT"]);
        assert_eq!(
            iterate(&["COMMERCE", "EVENT", "PROGRESSION", "LOGIN"]),
            ["COMMERCE", "LOGIN", "EVENT", "PROGRESSION"]
        );
        let letters = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n"];
        assert_eq!(iterate(&letters), letters);
        assert_eq!(iterate(&["Aa", "BB", "C#", "é", "😀"]), ["Aa", "BB", "C#", "😀", "é"]);
    }
}
