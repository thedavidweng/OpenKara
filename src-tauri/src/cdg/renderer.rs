use super::CdgPacket;

const FULL_WIDTH: usize = 300;
const FULL_HEIGHT: usize = 216;

pub const VISIBLE_WIDTH: usize = 288;
pub const VISIBLE_HEIGHT: usize = 192;

pub const CDG_RGBA_LEN: usize = VISIBLE_WIDTH * VISIBLE_HEIGHT * 4;

const BORDER_X: usize = 6;
const BORDER_Y: usize = 12;

/// CDG instruction codes (masked with 0x3F).
const CMD_MEMORY_PRESET: u8 = 1;
const CMD_BORDER_PRESET: u8 = 2;
const CMD_TILE_BLOCK: u8 = 6;
const CMD_SCROLL_PRESET: u8 = 20;
const CMD_SCROLL_COPY: u8 = 24;
const CMD_DEFINE_TRANSPARENT: u8 = 28;
const CMD_COLORS_LOW: u8 = 30;
const CMD_COLORS_HIGH: u8 = 31;
const CMD_TILE_BLOCK_XOR: u8 = 38;

const MAX_H_OFFSET: usize = 5;
const MAX_V_OFFSET: usize = 11;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdgRendererSnapshot {
    pixels: Vec<u8>,
    palette: [[u8; 4]; 16],
    last_was_memory_preset: bool,
    h_offset: usize,
    v_offset: usize,
    transparent_color: Option<u8>,
}

pub struct CdgRenderer {
    pixels: Vec<u8>,
    palette: [[u8; 4]; 16],
    last_was_memory_preset: bool,
    h_offset: usize,
    v_offset: usize,
    /// Transparent palette index from instruction 28 (Define Transparent Color).
    /// `None` means no transparency (all pixels opaque).
    transparent_color: Option<u8>,
}

impl CdgRenderer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0u8; FULL_WIDTH * FULL_HEIGHT],
            palette: [[0, 0, 0, 255]; 16],
            last_was_memory_preset: false,
            h_offset: 0,
            v_offset: 0,
            transparent_color: None,
        }
    }

    pub fn reset(&mut self) {
        self.pixels.fill(0);
        self.palette = [[0, 0, 0, 255]; 16];
        self.last_was_memory_preset = false;
        self.h_offset = 0;
        self.v_offset = 0;
        self.transparent_color = None;
    }

    /// Process a single packet. Returns `true` only when decoder-visible
    /// state changed. Invalid tiles, unsupported commands, duplicate palette
    /// values, and no-op scroll packets must not report a change.
    pub fn process_packet(&mut self, packet: &CdgPacket) -> bool {
        let changed = if packet.is_cdg() {
            self.apply_instruction(packet)
        } else {
            false
        };
        // Track Memory Preset repeat state only for actual CDG Memory Preset
        // packets so non-CDG packets do not break the adjacent-repeat filter.
        self.last_was_memory_preset =
            packet.is_cdg() && (packet.instruction & 0x3F) == CMD_MEMORY_PRESET;
        changed
    }

    pub fn process_range(&mut self, packets: &[CdgPacket], start: usize, end: usize) -> bool {
        let mut changed = false;
        let end = end.min(packets.len());
        for pkt in packets.iter().take(end).skip(start) {
            if self.process_packet(pkt) {
                changed = true;
            }
        }
        changed
    }

    pub fn snapshot(&self) -> CdgRendererSnapshot {
        CdgRendererSnapshot {
            pixels: self.pixels.clone(),
            palette: self.palette,
            last_was_memory_preset: self.last_was_memory_preset,
            h_offset: self.h_offset,
            v_offset: self.v_offset,
            transparent_color: self.transparent_color,
        }
    }

    pub fn restore(&mut self, snapshot: &CdgRendererSnapshot) {
        self.pixels.copy_from_slice(&snapshot.pixels);
        self.palette = snapshot.palette;
        self.last_was_memory_preset = snapshot.last_was_memory_preset;
        self.h_offset = snapshot.h_offset;
        self.v_offset = snapshot.v_offset;
        self.transparent_color = snapshot.transparent_color;
    }

    /// Convert the visible 288x192 area to RGBA pixels.
    ///
    /// Pixels using the transparent palette index get alpha 0; all other
    /// palette entries get alpha 255. Never reads outside the 300x216
    /// buffer for any packet bytes.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut rgba = vec![0u8; CDG_RGBA_LEN];
        for y in 0..VISIBLE_HEIGHT {
            let src_y = y + BORDER_Y + self.v_offset;
            if src_y >= FULL_HEIGHT {
                continue;
            }
            for x in 0..VISIBLE_WIDTH {
                let src_x = x + BORDER_X + self.h_offset;
                if src_x >= FULL_WIDTH {
                    continue;
                }
                let color_idx = self.pixels[src_y * FULL_WIDTH + src_x] & 0x0F;
                let color = &self.palette[color_idx as usize];
                let dst = (y * VISIBLE_WIDTH + x) * 4;
                rgba[dst] = color[0];
                rgba[dst + 1] = color[1];
                rgba[dst + 2] = color[2];
                rgba[dst + 3] = if self.transparent_color == Some(color_idx) {
                    0
                } else {
                    255
                };
            }
        }
        rgba
    }

    fn apply_instruction(&mut self, pkt: &CdgPacket) -> bool {
        match pkt.instruction & 0x3F {
            CMD_MEMORY_PRESET => self.cmd_memory_preset(&pkt.data),
            CMD_BORDER_PRESET => self.cmd_border_preset(&pkt.data),
            CMD_TILE_BLOCK => self.cmd_tile_block(&pkt.data, false),
            CMD_TILE_BLOCK_XOR => self.cmd_tile_block(&pkt.data, true),
            CMD_SCROLL_PRESET => self.cmd_scroll(&pkt.data, false),
            CMD_SCROLL_COPY => self.cmd_scroll(&pkt.data, true),
            CMD_DEFINE_TRANSPARENT => self.cmd_define_transparent(&pkt.data),
            CMD_COLORS_LOW => self.cmd_colors(&pkt.data, 0),
            CMD_COLORS_HIGH => self.cmd_colors(&pkt.data, 8),
            _ => false,
        }
    }

    fn cmd_memory_preset(&mut self, data: &[u8; 16]) -> bool {
        let color = data[0] & 0x0F;
        let repeat = data[1] & 0x0F;
        if self.last_was_memory_preset && repeat != 0 {
            return false;
        }
        let mut changed = false;
        for px in self.pixels.iter_mut() {
            if *px != color {
                *px = color;
                changed = true;
            }
        }
        changed
    }

    fn cmd_border_preset(&mut self, data: &[u8; 16]) -> bool {
        let color = data[0] & 0x0F;
        let mut changed = false;
        for y in 0..FULL_HEIGHT {
            if !(BORDER_Y..FULL_HEIGHT - BORDER_Y).contains(&y) {
                for x in 0..FULL_WIDTH {
                    let idx = y * FULL_WIDTH + x;
                    if self.pixels[idx] != color {
                        self.pixels[idx] = color;
                        changed = true;
                    }
                }
            } else {
                for x in 0..BORDER_X {
                    let idx = y * FULL_WIDTH + x;
                    if self.pixels[idx] != color {
                        self.pixels[idx] = color;
                        changed = true;
                    }
                }
                for x in (FULL_WIDTH - BORDER_X)..FULL_WIDTH {
                    let idx = y * FULL_WIDTH + x;
                    if self.pixels[idx] != color {
                        self.pixels[idx] = color;
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// Normal tile writes return true only if at least one color index changed;
    /// XOR returns true only if at least one resulting index changed.
    fn cmd_tile_block(&mut self, data: &[u8; 16], xor: bool) -> bool {
        let color0 = data[0] & 0x0F;
        let color1 = data[1] & 0x0F;
        let row = (data[2] & 0x1F) as usize;
        let column = (data[3] & 0x3F) as usize;

        if row >= 18 || column >= 50 {
            return false;
        }

        let top = row * 12;
        let left = column * 6;
        let mut changed = false;

        for y in 0..12 {
            let row_data = data[4 + y];
            let py = top + y;
            if py >= FULL_HEIGHT {
                continue;
            }
            for x in 0..6 {
                let px = left + x;
                if px >= FULL_WIDTH {
                    continue;
                }
                let mask = 0x20 >> x;
                let color = if row_data & mask != 0 { color1 } else { color0 };
                let idx = py * FULL_WIDTH + px;
                if xor {
                    let new_val = self.pixels[idx] ^ color;
                    if new_val != self.pixels[idx] {
                        self.pixels[idx] = new_val;
                        changed = true;
                    }
                } else if self.pixels[idx] != color {
                    self.pixels[idx] = color;
                    changed = true;
                }
            }
        }
        changed
    }

    fn cmd_scroll(&mut self, data: &[u8; 16], copy: bool) -> bool {
        let color = data[0] & 0x0F;
        let h_scroll = data[1] & 0x3F;
        let v_scroll = data[2] & 0x3F;
        let h_cmd = (h_scroll & 0x30) >> 4;
        let h_offset_raw = (h_scroll & 0x07) as usize;
        let v_cmd = (v_scroll & 0x30) >> 4;
        let v_offset_raw = (v_scroll & 0x0F) as usize;

        // Clamp fine offsets to FFmpeg/VLC/PyKaraoke bounds.
        let new_h_offset = h_offset_raw.min(MAX_H_OFFSET);
        let new_v_offset = v_offset_raw.min(MAX_V_OFFSET);

        let mut changed = false;

        if h_cmd == 2 {
            self.scroll_horizontal(-1, copy, color);
            changed = true;
        } else if h_cmd == 1 {
            self.scroll_horizontal(1, copy, color);
            changed = true;
        }

        if v_cmd == 2 {
            self.scroll_vertical(-1, copy, color);
            changed = true;
        } else if v_cmd == 1 {
            self.scroll_vertical(1, copy, color);
            changed = true;
        }

        if new_h_offset != self.h_offset {
            self.h_offset = new_h_offset;
            changed = true;
        }
        if new_v_offset != self.v_offset {
            self.v_offset = new_v_offset;
            changed = true;
        }

        changed
    }

    /// Scroll the framebuffer horizontally by 6 pixels. `direction` is -1
    /// (left) or +1 (right). In copy mode, vacated columns wrap around from
    /// the opposite edge; in preset mode, they are filled with `color`.
    fn scroll_horizontal(&mut self, direction: i32, copy: bool, color: u8) {
        let mut new_pixels = vec![0u8; FULL_WIDTH * FULL_HEIGHT];
        for y in 0..FULL_HEIGHT {
            for x in 0..FULL_WIDTH {
                let dst = y * FULL_WIDTH + x;
                let src_x = if direction < 0 {
                    if x + 6 < FULL_WIDTH {
                        Some(x + 6)
                    } else if copy {
                        Some(x + 6 - FULL_WIDTH)
                    } else {
                        None
                    }
                } else {
                    if x >= 6 {
                        Some(x - 6)
                    } else if copy {
                        Some(x + FULL_WIDTH - 6)
                    } else {
                        None
                    }
                };
                if let Some(sx) = src_x {
                    new_pixels[dst] = self.pixels[y * FULL_WIDTH + sx];
                } else {
                    new_pixels[dst] = color;
                }
            }
        }
        self.pixels = new_pixels;
    }

    /// Scroll the framebuffer vertically by 12 pixels. `direction` is -1
    /// (up) or +1 (down). In copy mode, vacated rows wrap around from the
    /// opposite edge; in preset mode, they are filled with `color`.
    fn scroll_vertical(&mut self, direction: i32, copy: bool, color: u8) {
        let mut new_pixels = vec![0u8; FULL_WIDTH * FULL_HEIGHT];
        for y in 0..FULL_HEIGHT {
            let src_y = if direction < 0 {
                if y + 12 < FULL_HEIGHT {
                    Some(y + 12)
                } else if copy {
                    Some(y + 12 - FULL_HEIGHT)
                } else {
                    None
                }
            } else {
                if y >= 12 {
                    Some(y - 12)
                } else if copy {
                    Some(y + FULL_HEIGHT - 12)
                } else {
                    None
                }
            };
            for x in 0..FULL_WIDTH {
                let dst = y * FULL_WIDTH + x;
                if let Some(sy) = src_y {
                    new_pixels[dst] = self.pixels[sy * FULL_WIDTH + x];
                } else {
                    new_pixels[dst] = color;
                }
            }
        }
        self.pixels = new_pixels;
    }

    fn cmd_colors(&mut self, data: &[u8; 16], offset: usize) -> bool {
        let mut changed = false;
        for i in 0..8 {
            let idx = i * 2;
            let low = data[idx];
            let high = data[idx + 1];

            let red = (low >> 2) & 0x0F;
            let green = ((low & 0x03) << 2) | ((high >> 4) & 0x03);
            let blue = high & 0x0F;

            let color = [red * 17, green * 17, blue * 17, 255];
            let color_idx = offset + i;
            if color_idx < 16 && self.palette[color_idx] != color {
                self.palette[color_idx] = color;
                changed = true;
            }
        }
        changed
    }

    /// Instruction 28: Define Transparent Color.
    /// Uses `data[0] & 0x0F` as the transparent palette index. In RGBA
    /// output, pixels using that index have alpha 0; all other palette
    /// entries have alpha 255. Palette loads change RGB values without
    /// losing the active transparency rule. Reset clears transparency.
    fn cmd_define_transparent(&mut self, data: &[u8; 16]) -> bool {
        let new_transparent = Some(data[0] & 0x0F);
        if new_transparent != self.transparent_color {
            self.transparent_color = new_transparent;
            true
        } else {
            false
        }
    }
}

impl Default for CdgRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdg::CdgPacket;

    fn cdg_packet(instruction: u8, data: [u8; 16]) -> CdgPacket {
        CdgPacket {
            command: 0x09,
            instruction,
            data,
        }
    }

    #[test]
    fn new_renderer_is_black() {
        let r = CdgRenderer::new();
        let rgba = r.to_rgba();
        assert_eq!(rgba.len(), CDG_RGBA_LEN);
        // All pixels should be black (palette[0] = [0,0,0,255])
        for chunk in rgba.chunks(4) {
            assert_eq!(chunk, &[0, 0, 0, 255]);
        }
    }

    #[test]
    fn memory_preset_fills_screen() {
        let mut r = CdgRenderer::new();
        // Set palette color 3 to red
        let mut color_data = [0u8; 16];
        // Color 3: red=15, green=0, blue=0 → low=0x3C, high=0x00
        color_data[6] = 0x3C; // color index 3 low byte
        color_data[7] = 0x00; // color index 3 high byte
        let color_pkt = cdg_packet(CMD_COLORS_LOW, color_data);
        r.process_packet(&color_pkt);

        // Memory preset with color 3
        let mut preset_data = [0u8; 16];
        preset_data[0] = 3;
        let preset_pkt = cdg_packet(CMD_MEMORY_PRESET, preset_data);
        r.process_packet(&preset_pkt);

        let rgba = r.to_rgba();
        // Check a pixel in the visible area — should be red
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn tile_block_writes_pixels() {
        let mut r = CdgRenderer::new();
        // Set palette color 1 to white
        let mut color_data = [0u8; 16];
        color_data[2] = 0x3F; // color 1: R=15 G=15
        color_data[3] = 0x3F; // color 1: B=15
        let color_pkt = cdg_packet(CMD_COLORS_LOW, color_data);
        r.process_packet(&color_pkt);

        // Draw a tile at row=1, column=1 (inside visible area)
        let mut tile_data = [0u8; 16];
        tile_data[0] = 0; // color0 = 0 (black)
        tile_data[1] = 1; // color1 = 1 (white)
        tile_data[2] = 1; // row = 1 (top = 12)
        tile_data[3] = 1; // column = 1 (left = 6)
        tile_data[4] = 0x3F; // first pixel row: all 6 bits set = all color1
        let tile_pkt = cdg_packet(CMD_TILE_BLOCK, tile_data);
        r.process_packet(&tile_pkt);

        let rgba = r.to_rgba();
        // Row 1, col 1 maps to visible area (0,0) since border is row=1/col=1
        // The first visible pixel should be white
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn reset_clears_state() {
        let mut r = CdgRenderer::new();
        let mut preset_data = [0u8; 16];
        preset_data[0] = 5;
        let pkt = cdg_packet(CMD_MEMORY_PRESET, preset_data);
        r.process_packet(&pkt);

        // Pixels should be color 5
        assert_eq!(r.pixels[0], 5);

        // Reset
        r.reset();
        assert_eq!(r.pixels[0], 0);
        assert_eq!(r.h_offset, 0);
        assert_eq!(r.v_offset, 0);
        assert_eq!(r.transparent_color, None);
    }

    #[test]
    fn repeat_memory_preset_filtered() {
        let mut r = CdgRenderer::new();
        let mut data1 = [0u8; 16];
        data1[0] = 3; // color 3
        data1[1] = 0; // repeat = 0 (first)
        let pkt1 = cdg_packet(CMD_MEMORY_PRESET, data1);

        let mut data2 = [0u8; 16];
        data2[0] = 5; // color 5
        data2[1] = 1; // repeat = 1 (should be filtered)
        let pkt2 = cdg_packet(CMD_MEMORY_PRESET, data2);

        r.process_packet(&pkt1);
        r.process_packet(&pkt2);
        // Should still be color 3, not 5
        assert_eq!(r.pixels[0], 3);
    }

    #[test]
    fn snapshot_and_restore_roundtrip() {
        let mut r = CdgRenderer::new();
        let mut color_data = [0u8; 16];
        color_data[2] = 0x3F;
        color_data[3] = 0x3F;
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, color_data));
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 1;
            d
        }));

        let snap = r.snapshot();
        let rgba_before = r.to_rgba();

        r.reset();
        assert_ne!(r.to_rgba(), rgba_before);

        r.restore(&snap);
        assert_eq!(r.to_rgba(), rgba_before);
    }

    #[test]
    fn define_transparent_color_sets_alpha_zero() {
        let mut r = CdgRenderer::new();
        // Fill with color 0 (default black, alpha 255)
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, [0u8; 16]));

        // Define color 0 as transparent
        r.process_packet(&cdg_packet(CMD_DEFINE_TRANSPARENT, {
            let mut d = [0u8; 16];
            d[0] = 0;
            d
        }));

        let rgba = r.to_rgba();
        // All visible pixels use color 0 → alpha should be 0
        assert_eq!(rgba[3], 0);
        assert_eq!(rgba[7], 0);
    }

    #[test]
    fn transparency_survives_palette_reload() {
        let mut r = CdgRenderer::new();
        // Define color 5 as transparent
        r.process_packet(&cdg_packet(CMD_DEFINE_TRANSPARENT, {
            let mut d = [0u8; 16];
            d[0] = 5;
            d
        }));

        // Reload palette (colors low) — transparency should remain
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, [0u8; 16]));

        // Fill with color 5
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 5;
            d
        }));

        let rgba = r.to_rgba();
        assert_eq!(
            rgba[3], 0,
            "color 5 should still be transparent after palette reload"
        );
    }

    #[test]
    fn reset_clears_transparency() {
        let mut r = CdgRenderer::new();
        r.process_packet(&cdg_packet(CMD_DEFINE_TRANSPARENT, {
            let mut d = [0u8; 16];
            d[0] = 3;
            d
        }));
        assert_eq!(r.transparent_color, Some(3));

        r.reset();
        assert_eq!(r.transparent_color, None);
    }

    #[test]
    fn fine_offset_clamps_to_5_horizontal() {
        let mut r = CdgRenderer::new();
        // h_offset_raw = 7 (above max 5), no scroll command
        let mut data = [0u8; 16];
        data[1] = 0x07; // h_scroll = 0x07 → h_cmd=0, h_offset=7
        r.process_packet(&cdg_packet(CMD_SCROLL_PRESET, data));
        assert_eq!(r.h_offset, 5, "h_offset should clamp to 5");
    }

    #[test]
    fn fine_offset_clamps_to_11_vertical() {
        let mut r = CdgRenderer::new();
        // v_offset_raw = 15 (above max 11), no scroll command
        let mut data = [0u8; 16];
        data[2] = 0x0F; // v_scroll = 0x0F → v_cmd=0, v_offset=15
        r.process_packet(&cdg_packet(CMD_SCROLL_PRESET, data));
        assert_eq!(r.v_offset, 11, "v_offset should clamp to 11");
    }

    #[test]
    fn invalid_tile_coordinates_no_change() {
        let mut r = CdgRenderer::new();
        // row=20 (>= 18, invalid)
        let mut data = [0u8; 16];
        data[2] = 20;
        data[3] = 0;
        let changed = r.process_packet(&cdg_packet(CMD_TILE_BLOCK, data));
        assert!(
            !changed,
            "invalid tile coordinates should not report change"
        );
    }

    #[test]
    fn xor_tile_returns_true_only_on_index_change() {
        let mut r = CdgRenderer::new();
        // Fill with color 0
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, [0u8; 16]));

        // XOR with color 0 on color 0 → no change
        let mut data = [0u8; 16];
        data[0] = 0; // color0 = 0
        data[1] = 0; // color1 = 0
        data[2] = 1; // row
        data[3] = 1; // col
        data[4] = 0x3F;
        let changed = r.process_packet(&cdg_packet(CMD_TILE_BLOCK_XOR, data));
        assert!(!changed, "XOR with 0 on 0 should not report change");

        // XOR with color 1 on color 0 → change
        data[1] = 1; // color1 = 1
        let changed = r.process_packet(&cdg_packet(CMD_TILE_BLOCK_XOR, data));
        assert!(changed, "XOR with 1 on 0 should report change");
    }

    #[test]
    fn scroll_left_copy_wraps() {
        let mut r = CdgRenderer::new();
        // Set color 1 to white, fill with color 1
        let mut color_data = [0u8; 16];
        color_data[2] = 0x3F;
        color_data[3] = 0x3F;
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, color_data));
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 1;
            d
        }));

        // Scroll left with copy — should wrap right edge to left
        let mut scroll_data = [0u8; 16];
        scroll_data[1] = 0x20; // h_cmd=2 (left), h_offset=0
        r.process_packet(&cdg_packet(CMD_SCROLL_COPY, scroll_data));

        // After scroll left by 6 with copy, the rightmost 6 columns wrap to left.
        // All pixels are color 1, so no visible change, but the operation must
        // not panic or read out of bounds.
        let rgba = r.to_rgba();
        assert_eq!(rgba.len(), CDG_RGBA_LEN);
    }

    #[test]
    fn scroll_right_copy_wraps() {
        let mut r = CdgRenderer::new();
        let mut scroll_data = [0u8; 16];
        scroll_data[1] = 0x10; // h_cmd=1 (right), h_offset=0
                               // Should not panic with wrapping_sub
        r.process_packet(&cdg_packet(CMD_SCROLL_COPY, scroll_data));
        let rgba = r.to_rgba();
        assert_eq!(rgba.len(), CDG_RGBA_LEN);
    }

    #[test]
    fn scroll_down_copy_wraps() {
        let mut r = CdgRenderer::new();
        let mut scroll_data = [0u8; 16];
        scroll_data[2] = 0x10; // v_cmd=1 (down), v_offset=0
        r.process_packet(&cdg_packet(CMD_SCROLL_COPY, scroll_data));
        let rgba = r.to_rgba();
        assert_eq!(rgba.len(), CDG_RGBA_LEN);
    }

    #[test]
    fn non_cdg_packet_no_change() {
        let mut r = CdgRenderer::new();
        let pkt = CdgPacket {
            command: 0x00, // not CDG
            instruction: 0x01,
            data: [0u8; 16],
        };
        let changed = r.process_packet(&pkt);
        assert!(!changed);
    }

    #[test]
    fn unsupported_instruction_no_change() {
        let mut r = CdgRenderer::new();
        // Instruction 99 is not a valid CDG instruction
        let changed = r.process_packet(&cdg_packet(99, [0u8; 16]));
        assert!(!changed);
    }

    #[test]
    fn to_rgba_never_reads_out_of_bounds() {
        let mut r = CdgRenderer::new();
        // Set max offsets
        r.h_offset = MAX_H_OFFSET;
        r.v_offset = MAX_V_OFFSET;
        let rgba = r.to_rgba();
        assert_eq!(rgba.len(), CDG_RGBA_LEN);
    }

    // Golden fixture tests: deterministic CDG packet sequences verified
    // against known-good RGBA reference values. They serve as regression
    // guards: if any decoder logic changes, the golden values will fail and
    // force a conscious review.

    #[test]
    fn golden_memory_preset_red_visible_area() {
        let mut r = CdgRenderer::new();
        // Set palette color 5 to pure red (R=15, G=0, B=0).
        let mut color_data = [0u8; 16];
        // Color 5 is at index 5 in the low palette (indices 0-7).
        // low byte: (red << 2) | (green >> 2) = (15 << 2) | 0 = 0x3C
        // high byte: ((green & 0x03) << 4) | blue = 0 | 0 = 0x00
        color_data[10] = 0x3C; // index 5 low
        color_data[11] = 0x00; // index 5 high
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, color_data));

        // Memory preset with color 5.
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 5;
            d
        }));

        let rgba = r.to_rgba();
        // Every visible pixel should be pure red, fully opaque.
        for chunk in rgba.chunks(4) {
            assert_eq!(chunk, &[255, 0, 0, 255], "golden: red preset pixel");
        }
    }

    #[test]
    fn golden_tile_block_white_on_black() {
        let mut r = CdgRenderer::new();
        // Set palette color 1 to white (R=15, G=15, B=15).
        let mut color_data = [0u8; 16];
        // Color 1: low = (15<<2)|(15>>2) = 0x3F, high = ((15&3)<<4)|15 = 0xFF
        color_data[2] = 0x3F;
        color_data[3] = 0xFF;
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, color_data));

        // Tile at row=0, column=0 (top-left, starts in border).
        // All 6 pixels in row 0 are color1 (white).
        let mut tile_data = [0u8; 16];
        tile_data[0] = 0; // color0 = black
        tile_data[1] = 1; // color1 = white
        tile_data[2] = 0; // row = 0
        tile_data[3] = 0; // column = 0
        tile_data[4] = 0x3F; // row 0: all 6 bits set
        r.process_packet(&cdg_packet(CMD_TILE_BLOCK, tile_data));

        let rgba = r.to_rgba();
        // The tile starts at (0,0) in the full buffer. The visible area
        // starts at (BORDER_X, BORDER_Y) = (6, 12). The tile's first row
        // (y=0..11) is in the top border, not visible. The tile's column
        // (x=0..5) is in the left border, not visible. So the visible area
        // should still be all black.
        for chunk in rgba.chunks(4) {
            assert_eq!(chunk, &[0, 0, 0, 255], "golden: tile in border not visible");
        }
    }

    #[test]
    fn golden_tile_block_visible_white_pixel() {
        let mut r = CdgRenderer::new();
        // Set palette color 1 to white.
        let mut color_data = [0u8; 16];
        color_data[2] = 0x3F;
        color_data[3] = 0xFF;
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, color_data));

        // Tile at row=1, column=1 — this maps to (top=12, left=6), which is
        // the first visible pixel (0,0) in the cropped output.
        let mut tile_data = [0u8; 16];
        tile_data[0] = 0; // color0 = black
        tile_data[1] = 1; // color1 = white
        tile_data[2] = 1; // row = 1
        tile_data[3] = 1; // column = 1
        tile_data[4] = 0x3F; // row 0 of tile: all white
        r.process_packet(&cdg_packet(CMD_TILE_BLOCK, tile_data));

        let rgba = r.to_rgba();
        // Visible pixel (0,0) should be white.
        assert_eq!(
            &rgba[0..4],
            &[255, 255, 255, 255],
            "golden: visible white pixel"
        );
        // Visible pixel (0,5) should be white (6th pixel in the tile row).
        assert_eq!(
            &rgba[20..24],
            &[255, 255, 255, 255],
            "golden: 6th pixel white"
        );
        // Visible pixel (0,6) should be black (outside the tile).
        assert_eq!(
            &rgba[24..28],
            &[0, 0, 0, 255],
            "golden: pixel after tile is black"
        );
    }

    #[test]
    fn golden_transparent_color_alpha_zero() {
        let mut r = CdgRenderer::new();
        // Fill with color 0 (black).
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, [0u8; 16]));
        // Define color 0 as transparent.
        r.process_packet(&cdg_packet(CMD_DEFINE_TRANSPARENT, {
            let mut d = [0u8; 16];
            d[0] = 0;
            d
        }));

        let rgba = r.to_rgba();
        // All visible pixels should be black with alpha 0.
        for chunk in rgba.chunks(4) {
            assert_eq!(chunk, &[0, 0, 0, 0], "golden: transparent black pixel");
        }
    }

    #[test]
    fn golden_deterministic_replay_same_output() {
        // Build a packet sequence that exercises multiple instructions.
        let packets = vec![
            // Set color 1 to white.
            cdg_packet(CMD_COLORS_LOW, {
                let mut d = [0u8; 16];
                d[2] = 0x3F;
                d[3] = 0xFF;
                d
            }),
            // Memory preset to color 0 (black).
            cdg_packet(CMD_MEMORY_PRESET, [0u8; 16]),
            // Tile at (1,1) with white pixels.
            cdg_packet(CMD_TILE_BLOCK, {
                let mut d = [0u8; 16];
                d[0] = 0;
                d[1] = 1;
                d[2] = 1;
                d[3] = 1;
                d[4] = 0x3F;
                d
            }),
        ];

        // Decode twice and verify identical output.
        let mut r1 = CdgRenderer::new();
        r1.process_range(&packets, 0, packets.len());
        let rgba1 = r1.to_rgba();

        let mut r2 = CdgRenderer::new();
        r2.process_range(&packets, 0, packets.len());
        let rgba2 = r2.to_rgba();

        assert_eq!(
            rgba1, rgba2,
            "golden: deterministic replay must produce same output"
        );
    }

    #[test]
    fn golden_snapshot_restore_preserves_output() {
        let mut r = CdgRenderer::new();
        // Set color 1 to white and fill.
        r.process_packet(&cdg_packet(CMD_COLORS_LOW, {
            let mut d = [0u8; 16];
            d[2] = 0x3F;
            d[3] = 0xFF;
            d
        }));
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 1;
            d
        }));

        let snap = r.snapshot();
        let rgba_before = r.to_rgba();

        r.reset();
        r.process_packet(&cdg_packet(CMD_MEMORY_PRESET, {
            let mut d = [0u8; 16];
            d[0] = 5;
            d
        }));
        assert_ne!(r.to_rgba(), rgba_before);

        r.restore(&snap);
        assert_eq!(
            r.to_rgba(),
            rgba_before,
            "golden: restore must reproduce same output"
        );
    }
}
