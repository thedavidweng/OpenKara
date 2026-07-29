use anyhow::{Context, Result};
use std::path::Path;

const PACKET_SIZE: usize = 24;

const CDG_COMMAND: u8 = 0x09;

#[derive(Clone, Debug)]
pub struct CdgPacket {
    pub command: u8,
    pub instruction: u8,
    pub data: [u8; 16],
}

impl CdgPacket {
    pub fn is_cdg(&self) -> bool {
        (self.command & 0x3F) == CDG_COMMAND
    }

    pub fn is_cdg_command(&self) -> bool {
        if !self.is_cdg() {
            return false;
        }
        matches!(
            self.instruction & 0x3F,
            1 | 2 | 6 | 20 | 24 | 28 | 30 | 31 | 38
        )
    }
}

/// Diagnostic emitted during parsing. Bounded: at most one trailing-byte
/// warning per parse call so logging cannot flood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdgParseDiagnostic {
    pub kind: CdgDiagnosticKind,
    pub trailing_bytes: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdgDiagnosticKind {
    TrailingBytes,
}

#[derive(Debug, Clone)]
pub struct CdgParseResult {
    pub packets: Vec<CdgPacket>,
    pub diagnostic: Option<CdgParseDiagnostic>,
}

impl CdgParseResult {
    pub fn has_cdg_commands(&self) -> bool {
        self.packets.iter().any(|p| p.is_cdg_command())
    }
}

/// Parse a `.cdg` file into a vector of packets.
///
/// Every 24 bytes in the file becomes one `CdgPacket`. Non-CDG packets are
/// included (they'll be skipped during rendering) to preserve timing — each
/// packet corresponds to 1/300th of a second.
pub fn parse_cdg_file(path: &Path) -> Result<Vec<CdgPacket>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read CDG file at {}", path.display()))?;

    Ok(parse_cdg_bytes(&bytes))
}

pub fn parse_cdg_bytes_with_diagnostics(bytes: &[u8]) -> CdgParseResult {
    let packet_count = bytes.len() / PACKET_SIZE;
    let trailing = bytes.len() % PACKET_SIZE;
    let mut packets = Vec::with_capacity(packet_count);

    for i in 0..packet_count {
        let offset = i * PACKET_SIZE;
        let mut data = [0u8; 16];
        data.copy_from_slice(&bytes[offset + 4..offset + 20]);

        packets.push(CdgPacket {
            command: bytes[offset],
            instruction: bytes[offset + 1],
            data,
        });
    }

    let diagnostic = if trailing > 0 {
        Some(CdgParseDiagnostic {
            kind: CdgDiagnosticKind::TrailingBytes,
            trailing_bytes: Some(trailing),
        })
    } else {
        None
    };

    CdgParseResult {
        packets,
        diagnostic,
    }
}

pub fn parse_cdg_bytes(bytes: &[u8]) -> Vec<CdgPacket> {
    parse_cdg_bytes_with_diagnostics(bytes).packets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.cdg");
        std::fs::write(&path, b"").unwrap();
        let packets = parse_cdg_file(&path).unwrap();
        assert!(packets.is_empty());
    }

    #[test]
    fn parse_single_packet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cdg");
        let mut raw = [0u8; 24];
        raw[0] = 0x09;
        raw[1] = 0x01;
        raw[4] = 0x05;
        std::fs::write(&path, raw).unwrap();

        let packets = parse_cdg_file(&path).unwrap();
        assert_eq!(packets.len(), 1);
        assert!(packets[0].is_cdg());
        assert_eq!(packets[0].instruction & 0x3F, 1);
        assert_eq!(packets[0].data[0] & 0x0F, 5);
    }

    #[test]
    fn non_cdg_packet_preserved_for_timing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cdg");
        let mut raw = [0u8; 48];
        raw[0] = 0x00;
        raw[24] = 0x09;
        raw[25] = 0x06;
        std::fs::write(&path, raw).unwrap();

        let packets = parse_cdg_file(&path).unwrap();
        assert_eq!(packets.len(), 2);
        assert!(!packets[0].is_cdg());
        assert!(packets[1].is_cdg());
    }

    #[test]
    fn trailing_bytes_ignored_but_diagnosed() {
        let raw = [0u8; 34];
        let result = parse_cdg_bytes_with_diagnostics(&raw);
        assert_eq!(result.packets.len(), 1);
        assert_eq!(
            result.diagnostic,
            Some(CdgParseDiagnostic {
                kind: CdgDiagnosticKind::TrailingBytes,
                trailing_bytes: Some(10),
            })
        );
    }

    #[test]
    fn no_trailing_bytes_no_diagnostic() {
        let raw = [0u8; 48];
        let result = parse_cdg_bytes_with_diagnostics(&raw);
        assert_eq!(result.packets.len(), 2);
        assert_eq!(result.diagnostic, None);
    }

    #[test]
    fn has_cdg_commands_detects_valid_instructions() {
        let mut raw = [0u8; 24];
        raw[0] = 0x09;
        raw[1] = 0x01;
        let result = parse_cdg_bytes_with_diagnostics(&raw);
        assert!(result.has_cdg_commands());
    }

    #[test]
    fn no_cdg_commands_when_only_non_cdg_packets() {
        let mut raw = [0u8; 48];
        raw[0] = 0x00;
        raw[24] = 0x09;
        raw[25] = 0x99;
        let result = parse_cdg_bytes_with_diagnostics(&raw);
        assert!(!result.has_cdg_commands());
    }
}
