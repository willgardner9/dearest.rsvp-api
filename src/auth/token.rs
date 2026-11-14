use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{TryRng, rngs::SysRng};
use sha2::{Digest, Sha256};

/// Make a token: 32 random bytes from the OS, as base64url without padding.
/// Return the plain token and its SHA-256 hash. The database stores only the hash.
pub fn new_token() -> (String, Vec<u8>) {
    let mut bytes = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut bytes)
        .expect("the OS random source works");
    let plain = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&plain);
    (plain, hash)
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_43_chars_base64url() {
        let (plain, hash) = new_token();
        assert_eq!(plain.len(), 43);
        assert!(
            plain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn hash_matches() {
        let (plain, hash) = new_token();
        assert_eq!(hash_token(&plain), hash);
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(new_token().0, new_token().0);
    }
}
