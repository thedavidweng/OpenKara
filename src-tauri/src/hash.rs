use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(hex_lower(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_lower_encodes_known_bytes() {
        assert_eq!(hex_lower([0x00u8, 0xff]), "00ff");
        assert_eq!(hex_lower([0x01, 0xab, 0xcd]), "01abcd");
    }

    #[test]
    fn hex_lower_empty_input() {
        assert_eq!(hex_lower([] as [u8; 0]), "");
    }

    #[test]
    fn hex_lower_single_byte_boundaries() {
        assert_eq!(hex_lower([0x00]), "00");
        assert_eq!(hex_lower([0x0f]), "0f");
        assert_eq!(hex_lower([0x10]), "10");
        assert_eq!(hex_lower([0xff]), "ff");
    }
}
