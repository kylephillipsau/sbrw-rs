//! `java.util` behaviour that decides what the port does.

use crate::lang::string_hash_code;

/// The order a `java.util.HashMap<String, _>` iterates keys put in this order
/// (distinct keys, default capacity and load factor, no removals): by bucket,
/// then by insertion within a bucket. Returns indexes into `keys`.
///
/// Exact while no bucket collects 8 keys, where the JDK would resize or build a
/// tree instead of a list.
pub fn hash_map_order<S: AsRef<str>>(keys: &[S]) -> Vec<usize> {
    let hashes: Vec<i32> = keys.iter().map(|k| string_hash_code(k.as_ref())).collect();
    hash_order(&hashes, DEFAULT_TABLE)
}

/// A `new HashMap<>()`'s first table.
pub const DEFAULT_TABLE: usize = 16;

/// Guava's `ArrayListMultimap.create()`: a `HashMap` sized for 12 keys, so a
/// table of 32 (`fixtures/vectors/java_multimap_order.json`).
pub const MULTIMAP_TABLE: usize = 32;

/// The order a `HashMap` whose first table has `table` buckets iterates
/// distinct keys with these `hashCode()`s, put in this order and never
/// removed. Returns indexes into `hashes`. The table doubles each time the
/// size passes three quarters of it; splitting a bucket keeps the relative
/// order. Exact while no bucket collects 8 keys.
pub fn hash_order(hashes: &[i32], table: usize) -> Vec<usize> {
    let mut capacity = table;
    while hashes.len() > capacity / 4 * 3 {
        capacity *= 2;
    }
    let bucket = |h: i32| (h ^ ((h as u32) >> 16) as i32) as u32 as usize & (capacity - 1);
    let mut order: Vec<usize> = (0..hashes.len()).collect();
    order.sort_by_key(|&i| bucket(hashes[i]));
    order
}

/// `Long.hashCode(value)`.
pub fn long_hash_code(value: i64) -> i32 {
    (value ^ ((value as u64) >> 32) as i64) as i32
}

/// `DoubleStream.sum()` in JDK 11: Kahan summation in stream order, falling
/// back to the plain sum when the compensated one is NaN and the plain one
/// infinite.
pub fn double_stream_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut sum, mut compensation, mut simple) = (0.0f64, 0.0f64, 0.0f64);
    for value in values {
        let tmp = value - compensation;
        let velvel = sum + tmp;
        compensation = (velvel - sum) - tmp;
        sum = velvel;
        simple += value;
    }
    let tmp = sum + compensation;
    if tmp.is_nan() && simple.is_infinite() { simple } else { tmp }
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

    fn vectors(name: &str) -> serde_json::Value {
        let path = format!("{}/../../fixtures/vectors/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn distinct<T: PartialEq + Copy>(keys: &[T]) -> Vec<T> {
        let mut out = Vec::new();
        for &k in keys {
            if !out.contains(&k) {
                out.push(k);
            }
        }
        out
    }

    #[test]
    fn orders_a_multimap_as_guava() {
        let cases = vectors("java_multimap_order.json")["cases"].as_array().unwrap().clone();
        assert_eq!(cases.len(), 300);
        for case in cases {
            let keys: Vec<i32> = serde_json::from_value(case["keys"].clone()).unwrap();
            let expected: Vec<usize> = serde_json::from_value(case["order"].clone()).unwrap();
            let unique = distinct(&keys);
            let mut order = Vec::new();
            for i in hash_order(&unique, MULTIMAP_TABLE) {
                order.extend((0..keys.len()).filter(|&j| keys[j] == unique[i]));
            }
            assert_eq!(order, expected, "keys {keys:?}");
        }
    }

    #[test]
    fn sums_weights_as_item_reward_bo() {
        let cases = vectors("java_weight_sum.json")["cases"].as_array().unwrap().clone();
        assert_eq!(cases.len(), 400);
        for case in cases {
            let ids: Vec<i64> = serde_json::from_value(case["ids"].clone()).unwrap();
            let bits: Vec<String> = serde_json::from_value(case["weight_bits"].clone()).unwrap();
            let weights: Vec<f64> = bits.iter().map(|b| f64::from_bits(u64::from_str_radix(b, 16).unwrap())).collect();
            let expected_order: Vec<usize> = serde_json::from_value(case["order"].clone()).unwrap();
            let expected_sum = u64::from_str_radix(case["sum_bits"].as_str().unwrap(), 16).unwrap();
            // `put` keeps a key's place and takes its last value.
            let unique = distinct(&ids);
            let hashes: Vec<i32> = unique.iter().map(|&id| long_hash_code(id)).collect();
            let order: Vec<usize> = hash_order(&hashes, DEFAULT_TABLE)
                .into_iter()
                .map(|i| ids.iter().position(|&id| id == unique[i]).unwrap())
                .collect();
            assert_eq!(order, expected_order, "ids {ids:?}");
            let value = |id: i64| weights[ids.iter().rposition(|&i| i == id).unwrap()];
            let sum = double_stream_sum(order.iter().map(|&i| value(ids[i])));
            assert_eq!(sum.to_bits(), expected_sum, "ids {ids:?}");
        }
    }

    #[test]
    fn sums_like_double_stream() {
        assert_eq!(double_stream_sum([]), 0.0);
        assert_eq!(double_stream_sum([0.1, 0.2, 0.3]), 0.6);
        assert_eq!(0.1 + 0.2 + 0.3, 0.6000000000000001);
        assert_eq!(double_stream_sum([f64::INFINITY, 1.0]), f64::INFINITY);
    }
}

/// `Base64.getEncoder().encodeToString(bytes)`: the standard alphabet, padded.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod base64_tests {
    use super::*;

    #[test]
    fn encodes_like_java_util_base64() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(&[0xfb, 0xff, 0xbf]), "+/+/");
        // Upstream's cryptoticket: 10, 11, 12, 13 then zeros, 32 bytes.
        let mut ticket = [0u8; 32];
        ticket[..4].copy_from_slice(&[10, 11, 12, 13]);
        assert_eq!(base64_encode(&ticket), "CgsMDQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    }
}
