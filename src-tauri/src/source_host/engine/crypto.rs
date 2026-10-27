use aes::cipher::{
    self,
    block_padding::{NoPadding, Pkcs7},
    BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit, StreamCipher,
};
use aes::{Aes128, Aes192, Aes256};
use base64::{
    alphabet,
    engine::{general_purpose::GeneralPurposeConfig, DecodePaddingMode, GeneralPurpose},
    Engine,
};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

pub(super) const MAX_RANDOM_BYTES: usize = 64 * 1024;
const MAX_RSA_BITS: u64 = 8192;

pub(super) type CryptoResult<T> = Result<T, &'static str>;

pub(super) fn digest(algorithm: &str, data: &[u8]) -> CryptoResult<Vec<u8>> {
    use sha2::Digest;
    Ok(match algorithm {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        _ => return Err("unsupported digest algorithm"),
    })
}

pub(super) fn hmac(algorithm: &str, key: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
    fn sign<M: Mac + KeyInit>(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut mac = <M as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    }
    Ok(match algorithm {
        "md5" => sign::<Hmac<md5::Md5>>(key, data),
        "sha1" => sign::<Hmac<sha1::Sha1>>(key, data),
        "sha256" => sign::<Hmac<sha2::Sha256>>(key, data),
        "sha384" => sign::<Hmac<sha2::Sha384>>(key, data),
        "sha512" => sign::<Hmac<sha2::Sha512>>(key, data),
        _ => return Err("unsupported HMAC algorithm"),
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Encrypt,
    Decrypt,
}

pub(super) struct AesOptions<'a> {
    pub(super) mode: &'a str,
    pub(super) key: &'a [u8],
    pub(super) iv: Option<&'a [u8]>,
    pub(super) padding: bool,
    pub(super) aad: &'a [u8],
}

/// AES in ECB, CBC, CTR (128-bit big-endian counter) or GCM (tag appended).
pub(super) fn aes(
    direction: Direction,
    options: &AesOptions<'_>,
    data: &[u8],
) -> CryptoResult<Vec<u8>> {
    macro_rules! by_key {
        ($run:ident) => {
            match options.key.len() {
                16 => $run::<Aes128>(direction, options, data),
                24 => $run::<Aes192>(direction, options, data),
                32 => $run::<Aes256>(direction, options, data),
                _ => Err("AES key must be 16, 24 or 32 bytes"),
            }
        };
    }
    match options.mode {
        "ecb" => by_key!(ecb_mode),
        "cbc" => by_key!(cbc_mode),
        "ctr" => by_key!(ctr_mode),
        "gcm" => gcm_mode(direction, options, data),
        _ => Err("unsupported AES mode"),
    }
}

fn block_mode<E: BlockModeEncrypt, D: BlockModeDecrypt>(
    direction: Direction,
    padding: bool,
    data: &[u8],
    encryptor: E,
    decryptor: D,
) -> CryptoResult<Vec<u8>> {
    if !padding && !data.len().is_multiple_of(16) {
        return Err("AES input without padding must be a multiple of 16 bytes");
    }
    let invalid = |_| "AES input or padding is invalid";
    match (direction, padding) {
        (Direction::Encrypt, true) => Ok(encryptor.encrypt_padded_vec::<Pkcs7>(data)),
        (Direction::Encrypt, false) => Ok(encryptor.encrypt_padded_vec::<NoPadding>(data)),
        (Direction::Decrypt, true) => decryptor.decrypt_padded_vec::<Pkcs7>(data).map_err(invalid),
        (Direction::Decrypt, false) => decryptor
            .decrypt_padded_vec::<NoPadding>(data)
            .map_err(invalid),
    }
}

fn ecb_mode<C>(direction: Direction, options: &AesOptions<'_>, data: &[u8]) -> CryptoResult<Vec<u8>>
where
    C: cipher::BlockCipherEncrypt + cipher::BlockCipherDecrypt + KeyInit,
{
    if options.iv.is_some() {
        return Err("AES-ECB takes no IV");
    }
    let bad_key = |_| "AES key is invalid";
    block_mode(
        direction,
        options.padding,
        data,
        ecb::Encryptor::<C>::new_from_slice(options.key).map_err(bad_key)?,
        ecb::Decryptor::<C>::new_from_slice(options.key).map_err(bad_key)?,
    )
}

fn iv_of<'a>(options: &AesOptions<'a>, length: usize) -> CryptoResult<&'a [u8]> {
    options
        .iv
        .filter(|iv| iv.len() == length)
        .ok_or("AES IV has the wrong length for this mode")
}

fn cbc_mode<C>(direction: Direction, options: &AesOptions<'_>, data: &[u8]) -> CryptoResult<Vec<u8>>
where
    C: cipher::BlockCipherEncrypt + cipher::BlockCipherDecrypt + KeyInit,
{
    let iv = iv_of(options, 16)?;
    let bad_key = |_| "AES key is invalid";
    block_mode(
        direction,
        options.padding,
        data,
        cbc::Encryptor::<C>::new_from_slices(options.key, iv).map_err(bad_key)?,
        cbc::Decryptor::<C>::new_from_slices(options.key, iv).map_err(bad_key)?,
    )
}

fn ctr_mode<C>(
    _direction: Direction,
    options: &AesOptions<'_>,
    data: &[u8],
) -> CryptoResult<Vec<u8>>
where
    C: cipher::BlockCipherEncrypt
        + cipher::BlockSizeUser<BlockSize = cipher::consts::U16>
        + KeyInit,
{
    let iv = iv_of(options, 16)?;
    let mut cipher =
        ctr::Ctr128BE::<C>::new_from_slices(options.key, iv).map_err(|_| "bad key or IV")?;
    let mut output = data.to_vec();
    cipher
        .try_apply_keystream(&mut output)
        .map_err(|_| "AES-CTR input is too long")?;
    Ok(output)
}

fn gcm_mode(direction: Direction, options: &AesOptions<'_>, data: &[u8]) -> CryptoResult<Vec<u8>> {
    let algorithm = match options.key.len() {
        16 => &aead::AES_128_GCM,
        32 => &aead::AES_256_GCM,
        _ => return Err("AES-GCM key must be 16 or 32 bytes"),
    };
    let key = LessSafeKey::new(UnboundKey::new(algorithm, options.key).map_err(|_| "bad key")?);
    let nonce = Nonce::try_assume_unique_for_key(iv_of(options, 12)?).map_err(|_| "bad IV")?;
    let mut buffer = data.to_vec();
    match direction {
        Direction::Encrypt => {
            key.seal_in_place_append_tag(nonce, Aad::from(options.aad), &mut buffer)
                .map_err(|_| "AES-GCM encryption failed")?;
            Ok(buffer)
        }
        Direction::Decrypt => {
            let plain = key
                .open_in_place(nonce, Aad::from(options.aad), &mut buffer)
                .map_err(|_| "AES-GCM authentication failed")?;
            Ok(plain.to_vec())
        }
    }
}

/// Raw RSA public-key operation `data^e mod n` with no padding. The result
/// is left-padded to the modulus length.
pub(super) fn rsa_public(modulus: &[u8], exponent: &[u8], data: &[u8]) -> CryptoResult<Vec<u8>> {
    let modulus = BigUint::from_bytes_be(modulus);
    let exponent = BigUint::from_bytes_be(exponent);
    let bits = modulus.bits();
    if !(512..=MAX_RSA_BITS).contains(&bits) || exponent < BigUint::from(3u8) {
        return Err("RSA key is unsupported");
    }
    let message = BigUint::from_bytes_be(data);
    if message >= modulus {
        return Err("RSA input must be smaller than the modulus");
    }
    let length = bits.div_ceil(8) as usize;
    let result = message.modpow(&exponent, &modulus).to_bytes_be();
    let mut output = vec![0u8; length - result.len()];
    output.extend_from_slice(&result);
    Ok(output)
}

/// Reads the modulus and exponent of an RSA public key in PEM form, either
/// SubjectPublicKeyInfo (`PUBLIC KEY`) or PKCS #1 (`RSA PUBLIC KEY`).
pub(super) fn rsa_public_key_pem(pem: &str) -> CryptoResult<(Vec<u8>, Vec<u8>)> {
    let invalid = "RSA public key PEM is invalid";
    let (label, body) = ["PUBLIC KEY", "RSA PUBLIC KEY"]
        .iter()
        .find_map(|label| {
            let begin = format!("-----BEGIN {label}-----");
            let end = format!("-----END {label}-----");
            let start = pem.find(&begin)? + begin.len();
            let stop = pem[start..].find(&end)? + start;
            Some((*label, &pem[start..stop]))
        })
        .ok_or(invalid)?;
    let compact: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(compact)
        .map_err(|_| invalid)?;
    let mut reader = Der(&der);
    let mut key = reader.take(0x30).map_err(|_| invalid)?;
    if label == "PUBLIC KEY" {
        let mut algorithm = key.take(0x30).map_err(|_| invalid)?;
        const RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
        if algorithm.take(0x06).map_err(|_| invalid)?.0 != RSA_ENCRYPTION {
            return Err(invalid);
        }
        let bits = key.take(0x03).map_err(|_| invalid)?.0;
        let (unused, inner) = bits.split_first().ok_or(invalid)?;
        if *unused != 0 {
            return Err(invalid);
        }
        key = Der(inner).take(0x30).map_err(|_| invalid)?;
    }
    let modulus = key.take(0x02).map_err(|_| invalid)?.0.to_vec();
    let exponent = key.take(0x02).map_err(|_| invalid)?.0.to_vec();
    Ok((modulus, exponent))
}

/// A minimal DER reader for the definite-length TLVs of an RSA public key.
struct Der<'a>(&'a [u8]);
impl<'a> Der<'a> {
    fn take(&mut self, tag: u8) -> Result<Der<'a>, ()> {
        let (&found, rest) = self.0.split_first().ok_or(())?;
        let (&first, mut rest) = rest.split_first().ok_or(())?;
        if found != tag {
            return Err(());
        }
        let length = if first < 0x80 {
            usize::from(first)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 || count > 4 || rest.len() < count {
                return Err(());
            }
            let (bytes, tail) = rest.split_at(count);
            rest = tail;
            bytes
                .iter()
                .fold(0usize, |length, byte| (length << 8) | usize::from(*byte))
        };
        if rest.len() < length {
            return Err(());
        }
        let (value, tail) = rest.split_at(length);
        self.0 = tail;
        Ok(Der(value))
    }
}

pub(super) fn random_bytes(length: usize) -> CryptoResult<Vec<u8>> {
    if length > MAX_RANDOM_BYTES {
        return Err("random byte request is too large");
    }
    let mut bytes = vec![0u8; length];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "secure random bytes are unavailable")?;
    Ok(bytes)
}

pub(super) fn base64_encode(data: &[u8], url_safe: bool, padding: bool) -> String {
    let alphabet = if url_safe {
        &alphabet::URL_SAFE
    } else {
        &alphabet::STANDARD
    };
    GeneralPurpose::new(
        alphabet,
        GeneralPurposeConfig::new().with_encode_padding(padding),
    )
    .encode(data)
}

/// Accepts the standard and URL-safe alphabets, with or without padding.
pub(super) fn base64_decode(text: &str) -> CryptoResult<Vec<u8>> {
    let alphabet = if text.contains(['-', '_']) {
        &alphabet::URL_SAFE
    } else {
        &alphabet::STANDARD
    };
    GeneralPurpose::new(
        alphabet,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    )
    .decode(text)
    .map_err(|_| "base64 text is invalid")
}

pub(super) fn hex_encode(data: &[u8]) -> String {
    crate::source_host::digest_hex(data)
}

pub(super) fn hex_decode(text: &str) -> CryptoResult<Vec<u8>> {
    let invalid = "hex text is invalid";
    if !text.len().is_multiple_of(2) {
        return Err(invalid);
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(invalid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        hex_decode(text).unwrap()
    }

    #[test]
    fn digests_and_hmacs_match_published_vectors() {
        assert_eq!(
            hex_encode(&digest("md5", b"abc").unwrap()),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex_encode(&digest("sha1", b"abc").unwrap()),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex_encode(&digest("sha256", b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(digest("sha3", b"abc").is_err());
        // RFC 4231 test case 2.
        assert_eq!(
            hex_encode(&hmac("sha256", b"Jefe", b"what do ya want for nothing?").unwrap()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // RFC 2202 test case 2.
        assert_eq!(
            hex_encode(&hmac("md5", b"Jefe", b"what do ya want for nothing?").unwrap()),
            "750c783e6ab0b503eaa86e310a5db738"
        );
    }

    fn options<'a>(
        mode: &'a str,
        key: &'a [u8],
        iv: Option<&'a [u8]>,
        padding: bool,
    ) -> AesOptions<'a> {
        AesOptions {
            mode,
            key,
            iv,
            padding,
            aad: &[],
        }
    }

    #[test]
    fn aes_modes_match_nist_vectors_and_round_trip() {
        // NIST SP 800-38A F.1.1, F.2.1 and F.5.1, first block.
        let key = hex("2b7e151628aed2a6abf7158809cf4f3c");
        let plain = hex("6bc1bee22e409f96e93d7e117393172a");
        let ecb = aes(
            Direction::Encrypt,
            &options("ecb", &key, None, false),
            &plain,
        )
        .unwrap();
        assert_eq!(hex_encode(&ecb), "3ad77bb40d7a3660a89ecaf32466ef97");
        let iv = hex("000102030405060708090a0b0c0d0e0f");
        let cbc = aes(
            Direction::Encrypt,
            &options("cbc", &key, Some(&iv), false),
            &plain,
        )
        .unwrap();
        assert_eq!(hex_encode(&cbc), "7649abac8119b246cee98e9b12e9197d");
        let counter = hex("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let ctr = aes(
            Direction::Encrypt,
            &options("ctr", &key, Some(&counter), false),
            &plain,
        )
        .unwrap();
        assert_eq!(hex_encode(&ctr), "874d6191b620e3261bef6864990db6ce");

        for mode in ["ecb", "cbc", "ctr", "gcm"] {
            let iv = match mode {
                "ecb" => None,
                "gcm" => Some(&counter[..12]),
                _ => Some(&counter[..]),
            };
            let options = AesOptions {
                aad: b"header",
                ..options(mode, &key, iv, true)
            };
            let sealed = aes(Direction::Encrypt, &options, b"hello, sandbox").unwrap();
            assert_eq!(
                aes(Direction::Decrypt, &options, &sealed).unwrap(),
                b"hello, sandbox",
                "{mode}"
            );
        }
        let gcm = options("gcm", &key, Some(&counter[..12]), true);
        let mut sealed = aes(Direction::Encrypt, &gcm, b"message").unwrap();
        sealed[0] ^= 1;
        assert!(aes(Direction::Decrypt, &gcm, &sealed).is_err());
        assert!(aes(Direction::Encrypt, &options("cbc", &key, None, true), b"x").is_err());
        assert!(aes(Direction::Encrypt, &options("ecb", &key, None, false), b"x").is_err());
        assert!(aes(Direction::Encrypt, &options("ofb", &key, None, true), b"x").is_err());
    }

    #[test]
    fn rsa_without_padding_matches_textbook_exponentiation() {
        let modulus = hex(concat!(
            "e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b72594",
            "21a8ff2ad3b8b6b2d04a2f0d9d7ba7e2c0d4d0f5c1e9c1b5c8f8a5a8c8a7d4c3"
        ));
        let exponent = hex("010001");
        let message = hex("02");
        let output = rsa_public(&modulus, &exponent, &message).unwrap();
        assert_eq!(output.len(), 64);
        let expected = BigUint::from(2u8)
            .modpow(&BigUint::from(65537u32), &BigUint::from_bytes_be(&modulus))
            .to_bytes_be();
        assert_eq!(output[64 - expected.len()..], expected[..]);
        assert!(rsa_public(&modulus, &exponent, &modulus).is_err());
        assert!(rsa_public(&hex("ff"), &exponent, &message).is_err());
    }

    #[test]
    fn rsa_public_keys_parse_from_both_pem_forms() {
        let pkcs1 =
            "-----BEGIN RSA PUBLIC KEY-----\nMAgCAwEAAQIBAw==\n-----END RSA PUBLIC KEY-----";
        assert_eq!(rsa_public_key_pem(pkcs1).unwrap(), (vec![1, 0, 1], vec![3]));
        let spki = base64::engine::general_purpose::STANDARD.encode(
            [
                &[0x30, 0x1a, 0x30, 0x0d, 0x06, 0x09][..],
                &[
                    0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
                ],
                &[
                    0x03, 0x09, 0x00, 0x30, 0x06, 0x02, 0x01, 0x07, 0x02, 0x01, 0x03,
                ],
            ]
            .concat(),
        );
        let spki = format!("-----BEGIN PUBLIC KEY-----\n{spki}\n-----END PUBLIC KEY-----");
        assert_eq!(rsa_public_key_pem(&spki).unwrap(), (vec![7], vec![3]));
        assert!(
            rsa_public_key_pem("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----")
                .is_err()
        );
    }

    #[test]
    fn encodings_round_trip_and_reject_garbage() {
        let bytes = [0xfbu8, 0xff, 0x00, 0x10];
        assert_eq!(base64_encode(&bytes, false, true), "+/8AEA==");
        assert_eq!(base64_encode(&bytes, true, false), "-_8AEA");
        assert_eq!(base64_decode("+/8AEA==").unwrap(), bytes);
        assert_eq!(base64_decode("-_8AEA").unwrap(), bytes);
        assert!(base64_decode("not base64!").is_err());
        assert_eq!(hex_encode(&bytes), "fbff0010");
        assert_eq!(hex_decode("FBFF0010").unwrap(), bytes);
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
        assert_eq!(random_bytes(32).unwrap().len(), 32);
        assert_ne!(random_bytes(32).unwrap(), random_bytes(32).unwrap());
        assert!(random_bytes(MAX_RANDOM_BYTES + 1).is_err());
    }
}
