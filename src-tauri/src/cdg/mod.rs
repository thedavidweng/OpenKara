pub mod parser;
pub mod renderer;

pub use parser::{
    parse_cdg_bytes, parse_cdg_bytes_with_diagnostics, parse_cdg_file, CdgDiagnosticKind,
    CdgPacket, CdgParseDiagnostic, CdgParseResult,
};
pub use renderer::{CdgRenderer, CdgRendererSnapshot, CDG_RGBA_LEN, VISIBLE_HEIGHT, VISIBLE_WIDTH};
