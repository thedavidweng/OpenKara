use crate::hash::hex_lower;
use aes::Aes128;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use num_bigint::BigUint;
use num_traits::Num;
use rand::RngExt;

const PRESET_KEY: &[u8; 16] = b"0CoJUm6Qyw8W8jud";
const IV: &[u8; 16] = b"0102030405060708";
const PUBLIC_EXPONENT: &str = "010001";
const MODULUS: &str = "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
const SECRET_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

type Aes128CbcEnc = cbc::Encryptor<Aes128>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeapiForm {
    pub params: String,
    pub enc_sec_key: String,
}

pub fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hex_lower(hasher.finalize())
}

pub fn weapi_encrypt(payload: &str) -> WeapiForm {
    let secret = random_secret();
    let first = aes_encrypt(payload.as_bytes(), PRESET_KEY);
    let params = aes_encrypt(first.as_bytes(), secret.as_bytes());
    let enc_sec_key = rsa_encrypt(&secret);
    WeapiForm {
        params,
        enc_sec_key,
    }
}

fn random_secret() -> String {
    let mut rng = rand::rng();
    (0..16)
        .map(|_| {
            let index = rng.random_range(0..SECRET_ALPHABET.len());
            SECRET_ALPHABET[index] as char
        })
        .collect()
}

fn aes_encrypt(data: &[u8], key: &[u8]) -> String {
    let mut buffer = vec![0_u8; data.len() + 16];
    buffer[..data.len()].copy_from_slice(data);
    let encrypted = Aes128CbcEnc::new(key.into(), IV.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, data.len())
        .expect("aes encrypt");
    BASE64.encode(encrypted)
}

fn rsa_encrypt(secret: &str) -> String {
    let reversed: String = secret.chars().rev().collect();
    let modulus = BigUint::from_str_radix(MODULUS, 16).expect("modulus");
    let exponent = BigUint::from_str_radix(PUBLIC_EXPONENT, 16).expect("exponent");
    let data = BigUint::from_bytes_be(reversed.as_bytes());
    let encrypted = data.modpow(&exponent, &modulus);
    format!("{encrypted:0>256x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapi_produces_non_empty_form() {
        let form = weapi_encrypt(r#"{"ids":["1"]}"#);
        assert!(!form.params.is_empty());
        assert_eq!(form.enc_sec_key.len(), 256);
        assert!(form.enc_sec_key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn password_is_md5_hex() {
        assert_eq!(md5_hex("password").len(), 32);
        assert_ne!(md5_hex("password"), "password");
    }
}
