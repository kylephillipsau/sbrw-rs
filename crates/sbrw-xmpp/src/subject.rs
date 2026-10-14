//! The `<subject>` of a server push.
//!
//! The stock game checks that a push's subject is a hash of the recipient JID
//! followed by the body. SBRW's ModNet patch turns the check off, and the core
//! sent a constant instead (`OpenFireConnector.send`). The hash is NFS-World-
//! Offline's `calculateHashFromData.js`, kept for the stock client.

/// The subject the core put on every push.
pub const SBRW_SUBJECT: &str = "1337733113377331";

/// The hash the stock client expects, over `to` then `body` as UTF-16 code
/// units, printed as a signed 64-bit number.
pub fn hash(to: &str, body: &str) -> String {
    hash_code_units(to.encode_utf16().chain(body.encode_utf16())).to_string()
}

fn hash_code_units(units: impl IntoIterator<Item = u16>) -> i64 {
    const LOW: u64 = 0xFFFF_FFFF;
    let mut hash: u64 = LOW;
    let mut carry = true;
    for unit in units {
        // Only bits 0..64 of the product reach either half.
        let start = hash.wrapping_mul(33);
        let upper = start >> 32;
        let lower = start & LOW;
        let new_upper = (upper + u64::from(carry)) & LOW;
        let new_lower = (lower + u64::from(unit)) & LOW;
        // nfswo's carry comes from the upper half, not the lower.
        carry = upper > new_upper;
        hash = (new_upper << 32) | new_lower;
    }
    hash as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Vectors {
        vectors: Vec<(String, String)>,
    }

    #[test]
    fn matches_nfs_world_offline() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/xmpp_subject.json");
        let text =
            std::fs::read_to_string(path).expect("run reference/nfswo/subject_vectors.js to generate the vectors");
        let vectors: Vectors = serde_json::from_str(&text).unwrap();
        assert!(!vectors.vectors.is_empty());
        for (hex, expected) in vectors.vectors {
            let units: Vec<u16> = hex
                .as_bytes()
                .chunks(4)
                .map(|c| u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
                .collect();
            assert_eq!(hash_code_units(units).to_string(), expected, "{hex:.40}");
        }
    }

    #[test]
    fn hashes_the_recipient_then_the_body() {
        assert_eq!(hash("sbrw.100@127.0.0.1", ""), hash("", "sbrw.100@127.0.0.1"));
        assert_eq!(hash("", ""), "4294967295");
    }
}
