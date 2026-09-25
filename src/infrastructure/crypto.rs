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
    let mut salted = Vec::with_capacity(salt.len() + 4);
    salted.extend_from_slice(salt);
    salted.extend_from_slice(&1u32.to_be_bytes());
    let mut mac = HmacSha256::new_from_slice(password).expect("hmac key");
    mac.update(&salted);
    let mut u = mac.finalize().into_bytes().to_vec();
    let mut out = u.clone();
    for _ in 1..iterations {
        let mut mac = HmacSha256::new_from_slice(password).expect("hmac key");
        mac.update(&u);
        u = mac.finalize().into_bytes().to_vec();
        for (o, x) in out.iter_mut().zip(u.iter()) {
            *o ^= *x;
        }
    }
    out
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
