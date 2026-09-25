use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// PBKDF2-SHA256 password hash, format: pbkdf2$<iterations>$<salt_hex>$<hash_hex>.
pub fn hash_password(password: &str) -> String {
    let salt = rand::random::<[u8; 16]>();
    let iterations: u32 = 60_000;
    let hash = pbkdf2_sha256(password.as_bytes(), &salt, iterations);
    format!("pbkdf2${iterations}${}${}", hex::encode(salt), hex::encode(hash))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2" {
        return false;
    }
    let iterations: u32 = match parts[1].parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let salt = match hex::decode(parts[2]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let expected = match hex::decode(parts[3]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let actual = pbkdf2_sha256(password.as_bytes(), &salt, iterations);
    constant_time_eq(&actual, &expected)
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Random lowercase hex token of `bytes` random bytes (2*bytes chars).
pub fn random_token(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn hmac_sha256_hex(secret: &str, data: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(data);
    hex::encode(mac.finalize().into_bytes())
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut out = [0u8; 32];
    // pbkdf2_hmac returns () for HMAC-based derivation; it cannot fail for valid lengths.
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut out);
    out.to_vec()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("correct-horse-battery-staple");
        assert!(hash.starts_with("pbkdf2$60000$"));
        assert!(verify_password("correct-horse-battery-staple", &hash));
        assert!(!verify_password("wrong-password", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn password_hash_uses_random_salt() {
        let a = hash_password("same-password");
        let b = hash_password("same-password");
        assert_ne!(a, b, "each hash must use a fresh random salt");
        assert!(verify_password("same-password", &a));
        assert!(verify_password("same-password", &b));
    }

    #[test]
    fn verify_password_rejects_malformed_hash() {
        assert!(!verify_password("pw", ""));
        assert!(!verify_password("pw", "plaintext"));
        assert!(!verify_password("pw", "md5$1$aa$bb"));
        assert!(!verify_password("pw", "pbkdf2$notanumber$aa$bb"));
    }

    #[test]
    fn sha256_hex_is_stable() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hmac_sha256_hex_matches_known_vector() {
        assert_eq!(
            hmac_sha256_hex("key", b"The quick brown fox jumps over the lazy dog"),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
}
