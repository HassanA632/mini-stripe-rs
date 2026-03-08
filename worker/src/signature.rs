use hmac::{Hmac, Mac};
use sha2::Sha256;

pub fn sign(secret: &str, payload: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload);
    let sig = mac.finalize().into_bytes();
    hex::encode(sig)
}
