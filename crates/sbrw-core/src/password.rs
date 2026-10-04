//! Stored passwords.
//!
//! The launcher sends the SHA-1 hex of the player's password, and upstream
//! stores and compares that string as is, so a leaked database is a leaked set
//! of logins. sbrw-rs stores an argon2id hash of it instead (a deliberate
//! divergence; replays verify rather than compare the column). Rows imported
//! from an upstream database still hold the bare SHA-1 and keep working.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};

/// Hash what the launcher sent for storage.
pub fn hash(sent: &str) -> String {
    // The salt comes from the OS, never from the game's shared java.util.Random,
    // whose sequence replays must reproduce.
    let hash: PasswordHash = Argon2::default().hash_password(sent.as_bytes()).expect("argon2 with default parameters");
    hash.to_string()
}

/// Upstream's `password.equals(userEntity.getPassword())`.
pub fn matches(sent: &str, stored: &str) -> bool {
    if stored.starts_with("$argon2") {
        PasswordHash::new(stored).is_ok_and(|h| Argon2::default().verify_password(sent.as_bytes(), &h).is_ok())
    } else {
        sent == stored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_legacy_rows_still_match() {
        let sha1 = "5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8";
        let stored = hash(sha1);
        assert!(stored.starts_with("$argon2id$"));
        assert_ne!(stored, hash(sha1), "salted");
        assert!(matches(sha1, &stored));
        assert!(!matches("0000000000000000000000000000000000000000", &stored));
        assert!(matches(sha1, sha1));
        assert!(!matches(sha1, "something else"));
    }
}
