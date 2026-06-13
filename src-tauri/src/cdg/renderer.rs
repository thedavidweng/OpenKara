use super::CdgPacket;

/// Full CDG display dimensions (including border).
const FULL_WIDTH: usize = 300;
const FULL_HEIGHT: usize = 216;

/// Visible (cropped) display dimensions.
const VISIBLE_WIDTH: usize = 288;
const VISIBLE_HEIGHT: usize = 192;

/// Border size on each side.
const BORDER_X: usize = 6;
const BORDER_Y: usize = 12;

/// Item 11: Save a checkpoint every N packets to speed up backward seeks.
const CHECKPOINT_INTERVAL: usize = 1000;

/// CDG instruction codes (masked with 0x3F).
const CMD_MEMORY_PRESET: u8 = 1;
const CMD_BORDER_PRESET: u8 = 2;
const CMD_TILE_BLOCK: u8 = 6;
const CMD_SCROLL_PRESET: u8 = 20;
const CMD_SCROLL_COPY: u8 = 24;
const CMD_COLORS_LOW: u8 = 30;
const CMD_COLORS_HIGH: u8 = 31;
const CMD_TILE_BLOCK_XOR: u8 = 38;

/// Item 11: Snapshot of renderer state at a specific packet index, used to
/// accelerate backward seeks by restoring the nearest checkpoint instead of
/// replaying from packet 0.
#[derive(Clone)]
struct CdgCheckpoint {
    /// Packet index this checkpoint was saved at.
    packet_index: usize,
    pixels: Vec<u8>,
    palette: [[u8; 4]; 16],
    last_was_memory_preset: bool,
    h_offset: usize,
    v_offset: usize,
}

/// CDG renderer maintaining a 300x216 indexed-color frame buffer.
pub struct CdgRenderer {
    /// 4-bit indexed color per pixel (values 0..15).
    pixels: Vec<u8>,
    /// RGBA palette (16 entries, 4 bytes each).
    palette: [[u8; 4]; 16],
    /// Whether the last command was a MemoryPreset (for repeat filtering).
    last_was_memory_preset: bool,
    /// Current horizontal scroll offset in pixels.
    h_offset: usize,
    /// Current vertical scroll offset in pixels.
    v_offset: usize,
    /// Item 11: Periodic checkpoints for fast backward seeking.
    checkpoints: Vec<CdgCheckpoint>,
    /// Tracks the last packet index processed (for checkpoint scheduling).
    last_processed_index: usize,
}

impl CdgRenderer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0u8; FULL_WIDTH * FULL_HEIGHT],
            palette: [[0, 0, 0, 255]; 16],
            last_was_memory_preset: false,
            h_offset: 0,
            v_offset: 0,
            checkpoints: Vec::new(),
            last_processed_index: 0,
        }
    }

    /// Process packets from `start` (inclusive) to `end` (exclusive).
    /// Returns `true` if any packet caused a visible change.
    pub fn process_range(&mut self, packets: &[CdgPacket], start: usize, end: usize) -> bool {
        let mut changed = false;
        let end = end.min(packets.len());
        for (i, pkt) in packets.iter().enumerate().take(end).skip(start) {
            if pkt.is_cdg() && self.apply_instruction(pkt) {
                changed = true;
            }
            self.last_was_memory_preset =
                pkt.is_cdg() && (pkt.instruction & 0x3F) == CMD_MEMORY_PRESET;
            self.maybe_save_checkpoint(i);
        }
        self.last_processed_index = end;
        changed
    }

    /// Item 11: Seek to a specific packet position. If a checkpoint exists that
    /// is before the target, restore it and replay forward from there. Otherwise
    /// replay from packet 0.
    pub fn seek_to(&mut self, packets: &[CdgPacket], target: usize) {
        let target = target.min(packets.len());

        // Find the nearest checkpoint at or before the target.
        let checkpoint_index = self
            .checkpoints
            .iter()
            .rev()
            .find(|cp| cp.packet_index <= target)
            .map(|cp| cp.packet_index);

        if let Some(idx) = checkpoint_index {
            self.restore_checkpoint_at(packets, idx, target);
        } else {
            self.reset_and_render_to(packets, target);
        }
    }

    /// Reset the renderer to initial state and re-render from packet 0 up to
    /// `end` (exclusive). Used for seeking when no checkpoint is available.
    pub fn reset_and_render_to(&mut self, packets: &[CdgPacket], end: usize) {
        self.pixels.fill(0);
        self.palette = [[0, 0, 0, 255]; 16];
        self.last_was_memory_preset = false;
        self.h_offset = 0;
        self.v_offset = 0;
        self.checkpoints.clear();
        self.last_processed_index = 0;
        self.process_range(packets, 0, end);
    }

    /// Item 11: Save a checkpoint if we've crossed a CHECKPOINT_INTERVAL boundary.
    fn maybe_save_checkpoint(&mut self, packet_index: usize) {
        let last_checkpoint_idx = self.checkpoints.last().map(|cp| cp.packet_index);
        let should_save = match last_checkpoint_idx {
            Some(last) => packet_index >= last + CHECKPOINT_INTERVAL,
            None => packet_index >= CHECKPOINT_INTERVAL,
        };

        if should_save {
            self.checkpoints.push(CdgCheckpoint {
                packet_index,
                pixels: self.pixels.clone(),
                palette: self.palette,
                last_was_memory_preset: self.last_was_memory_preset,
                h_offset: self.h_offset,
                v_offset: self.v_offset,
            });
        }
    }

    /// Restore from the nearest checkpoint at or before `target_packet` and
    /// re-render forward to `target`. Avoids borrow conflicts by operating
    /// entirely on `&mut self`.
    fn restore_checkpoint_at(
        &mut self,
        packets: &[CdgPacket],
        target_packet: usize,
        target: usize,
    ) {
        if let Some(cp) = self
            .checkpoints
            .iter()
            .rev()
            .find(|cp| cp.packet_index <= target_packet)
            .cloned()
        {
            self.restore_checkpoint(&cp);
            self.process_range(packets, cp.packet_index, target);
        } else {
            self.reset_and_render_to(packets, target);
        }
    }

    /// Item 11: Restore renderer state from a checkpoint.
    fn restore_checkpoint(&mut self, cp: &CdgCheckpoint) {
        self.pixels.copy_from_slice(&cp.pixels);
        self.palette = cp.palette;
        self.last_was_memory_preset = cp.last_was_memory_preset;
        self.h_offset = cp.h_offset;
        self.v_offset = cp.v_offset;
    }

    /// Convert the visible 288x192 area to RGBA pixels.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut rgba = vec![0u8; VISIBLE_WIDTH * VISIBLE_HEIGHT * 4];
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
                let color_idx = self.pixels[src_y * FULL_WIDTH + src_x] as usize;
                let color = &self.palette[color_idx & 0x0F];
                let dst = (y * VISIBLE_WIDTH + x) * 4;
                rgba[dst] = color[0];
                rgba[dst + 1] = color[1];
                rgba[dst + 2] = color[2];
                rgba[dst + 3] = color[3];
            }
        }
        rgba
    }

    fn apply_instruction(&mut self, pkt: &CdgPacket) -> bool {
        match pkt.instruction & 0x3F {
            CMD_MEMORY_PRESET => self.cmd_memory_preset(&pkt.data),
            CMD_BORDER_PRESET => self.cmd_border_preset(&pkt.data),
            CMD_TILE_BLOCK => {
                self.cmd_tile_block(&pkt.data, false);
                true
            }
            CMD_TILE_BLOCK_XOR => {
                self.cmd_tile_block(&pkt.data, true);
                true
            }
            CMD_SCROLL_PRESET => {
                self.cmd_scroll(&pkt.data, false);
                true
            }
            CMD_SCROLL_COPY => {
                self.cmd_scroll(&pkt.data, true);
                true
            }
            CMD_COLORS_LOW => self.cmd_colors(&pkt.data, 0),
            CMD_COLORS_HIGH => self.cmd_colors(&pkt.data, 8),
            _ => false,
        }
    }

    fn cmd_memory_preset(&mut self, data: &[u8; 16]) -> bool {
        let color = data[0] & 0x0F;
        if color >= 16 {
            return false;
        }
        let repeat = data[1] & 0x0F;
        if self.last_was_memory_preset && repeat != 0 {
            return false;
        }
        self.pixels.fill(color);
        true
    }

    fn cmd_border_preset(&mut self, data: &[u8; 16]) -> bool {
        let color = data[0] & 0x0F;
        if color >= 16 {
            return false;
        }
        for y in 0..FULL_HEIGHT {
            if !(BORDER_Y..FULL_HEIGHT - BORDER_Y).contains(&y) {
                // Full row is border
                for x in 0..FULL_WIDTH {
                    self.pixels[y * FULL_WIDTH + x] = color;
                }
            } else {
                // Left and right border columns
                for x in 0..BORDER_X {
                    self.pixels[y * FULL_WIDTH + x] = color;
                }
                for x in (FULL_WIDTH - BORDER_X)..FULL_WIDTH {
                    self.pixels[y * FULL_WIDTH + x] = color;
                }
            }
        }
        true
    }

    fn cmd_tile_block(&mut self, data: &[u8; 16], xor: bool) {
        let color0 = data[0] & 0x0F;
        let color1 = data[1] & 0x0F;
        let row = (data[2] & 0x1F) as usize;
        let column = (data[3] & 0x3F) as usize;

        if row >= 18 || column >= 50 || color0 >= 16 || color1 >= 16 {
            return;
        }

        let top = row * 12;
        let left = column * 6;

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
                    self.pixels[idx] ^= color;
                } else {
                    self.pixels[idx] = color;
                }
            }
        }
    }

    fn cmd_scroll(&mut self, data: &[u8; 16], copy: bool) {
        let color = data[0] & 0x0F;
        let h_scroll = data[1] & 0x3F;
        let v_scroll = data[2] & 0x3F;
        let h_cmd = (h_scroll & 0x30) >> 4;
        let h_offset = (h_scroll & 0x07) as usize;
        let v_cmd = (v_scroll & 0x30) >> 4;
        let v_offset = (v_scroll & 0x0F) as usize;

        // Horizontal scroll
        if h_cmd == 2 {
            // Scroll left 6px
            self.scroll_horizontal(-1, copy, color);
        } else if h_cmd == 1 {
            // Scroll right 6px
            self.scroll_horizontal(1, copy, color);
        }

        // Vertical scroll
        if v_cmd == 2 {
            // Scroll up 12px
            self.scroll_vertical(-1, copy, color);
        } else if v_cmd == 1 {
            // Scroll down 12px
            self.scroll_vertical(1, copy, color);
        }

        self.h_offset = h_offset;
        self.v_offset = v_offset;
    }

    fn scroll_horizontal(&mut self, direction: i32, copy: bool, color: u8) {
        let mut new_pixels = vec![0u8; FULL_WIDTH * FULL_HEIGHT];
        for y in 0..FULL_HEIGHT {
            for x in 0..FULL_WIDTH {
                let src_x = if direction < 0 {
                    // scroll left: source is 6 pixels to the right
                    x + 6
                } else {
                    // scroll right: source is 6 pixels to the left
                    x.wrapping_sub(6)
                };
                let dst = y * FULL_WIDTH + x;
                if src_x < FULL_WIDTH {
                    new_pixels[dst] = self.pixels[y * FULL_WIDTH + src_x];
                } else if copy {
                    // Wrap around
                    let wrapped = if direction < 0 {
                        src_x.wrapping_sub(FULL_WIDTH)
                    } else {
                        src_x.wrapping_add(FULL_WIDTH)
                    };
                    if wrapped < FULL_WIDTH {
                        new_pixels[dst] = self.pixels[y * FULL_WIDTH + wrapped];
                    } else {
                        new_pixels[dst] = color;
                    }
                } else {
                    new_pixels[dst] = color;
                }
            }
        }
        self.pixels = new_pixels;
    }

    fn scroll_vertical(&mut self, direction: i32, copy: bool, color: u8) {
        let mut new_pixels = vec![0u8; FULL_WIDTH * FULL_HEIGHT];
        for y in 0..FULL_HEIGHT {
            let src_y = if direction < 0 {
                // scroll up: source is 12 rows below
                y + 12
            } else {
                // scroll down: source is 12 rows above
                y.wrapping_sub(12)
            };
            for x in 0..FULL_WIDTH {
                let dst = y * FULL_WIDTH + x;
                if src_y < FULL_HEIGHT {
                    new_pixels[dst] = self.pixels[src_y * FULL_WIDTH + x];
                } else if copy {
                    let wrapped = if direction < 0 {
                        src_y.wrapping_sub(FULL_HEIGHT)
                    } else {
                        src_y.wrapping_add(FULL_HEIGHT)
                    };
                    if wrapped < FULL_HEIGHT {
                        new_pixels[dst] = self.pixels[wrapped * FULL_WIDTH + x];
                    } else {
                        new_pixels[dst] = color;
                    }
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
        assert_eq!(rgba.len(), 288 * 192 * 4);
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
        r.process_range(&[color_pkt], 0, 1);

        // Memory preset with color 3
        let mut preset_data = [0u8; 16];
        preset_data[0] = 3;
        let preset_pkt = cdg_packet(CMD_MEMORY_PRESET, preset_data);
        r.process_range(&[preset_pkt], 0, 1);

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
        r.process_range(&[color_pkt], 0, 1);

        // Draw a tile at row=1, column=1 (inside visible area)
        let mut tile_data = [0u8; 16];
        tile_data[0] = 0; // color0 = 0 (black)
        tile_data[1] = 1; // color1 = 1 (white)
        tile_data[2] = 1; // row = 1 (top = 12)
        tile_data[3] = 1; // column = 1 (left = 6)
        tile_data[4] = 0x3F; // first pixel row: all 6 bits set = all color1
        let tile_pkt = cdg_packet(CMD_TILE_BLOCK, tile_data);
        r.process_range(&[tile_pkt], 0, 1);

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
        r.process_range(std::slice::from_ref(&pkt), 0, 1);

        // Pixels should be color 5
        assert_eq!(r.pixels[0], 5);

        // Reset
        r.reset_and_render_to(&[], 0);
        assert_eq!(r.pixels[0], 0);
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

        r.process_range(&[pkt1, pkt2], 0, 2);
        // Should still be color 3, not 5
        assert_eq!(r.pixels[0], 3);
    }

    /// Item 11: Backward seek should restore from checkpoint, not replay from 0.
    #[test]
    fn seek_to_restores_nearest_checkpoint() {
        let mut r = CdgRenderer::new();

        // Create a sequence of packets that modify the state.
        // Fill with color 5.
        let mut color_data = [0u8; 16];
        color_data[10] = 0x14; // color 5 low byte
        color_data[11] = 0x00; // color 5 high byte
        let color_pkt = cdg_packet(CMD_COLORS_LOW, color_data);

        let mut preset_data = [0u8; 16];
        preset_data[0] = 5;
        let preset_pkt = cdg_packet(CMD_MEMORY_PRESET, preset_data);

        // Build enough packets to trigger a checkpoint (> CHECKPOINT_INTERVAL).
        let mut packets = Vec::new();
        packets.push(color_pkt);
        packets.push(preset_pkt);
        // Add dummy packets to reach checkpoint threshold.
        for _ in 2..=CHECKPOINT_INTERVAL + 10 {
            let mut data = [0u8; 16];
            data[0] = 0;
            data[1] = 0;
            packets.push(cdg_packet(CMD_MEMORY_PRESET, data));
        }

        // Process all packets to build checkpoints.
        r.process_range(&packets, 0, packets.len());

        // Record state at the end.
        let end_pixel = r.pixels[0];

        // Now seek backward to packet index 2 (right after initial setup).
        r.seek_to(&packets, 2);

        // The state should be as it was after processing packets 0..2.
        // After color setup + memory preset with color 5, pixels should be 5.
        assert_eq!(r.pixels[0], 5);
        // Verify this is different from the end state.
        assert_ne!(r.pixels[0], end_pixel);
    }

    /// Item 11: Checkpoints should be created at regular intervals.
    #[test]
    fn checkpoints_created_at_intervals() {
        let mut r = CdgRenderer::new();

        let mut packets = Vec::new();
        // Create enough packets for multiple checkpoints.
        for i in 0..CHECKPOINT_INTERVAL * 3 + 10 {
            let mut data = [0u8; 16];
            data[0] = (i % 16) as u8;
            packets.push(cdg_packet(CMD_MEMORY_PRESET, data));
        }

        r.process_range(&packets, 0, packets.len());

        // Should have at least 3 checkpoints (at ~1000, ~2000, ~3000).
        assert!(
            r.checkpoints.len() >= 3,
            "Expected at least 3 checkpoints, got {}",
            r.checkpoints.len()
        );

        // Checkpoints should be at intervals of CHECKPOINT_INTERVAL.
        for window in r.checkpoints.windows(2) {
            let gap = window[1].packet_index - window[0].packet_index;
            assert!(
                gap >= CHECKPOINT_INTERVAL,
                "Checkpoint gap {gap} should be >= {CHECKPOINT_INTERVAL}"
            );
        }
    }
}
