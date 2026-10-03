use crate::emulator::mem::Memory;
use crate::types::*;

/// Scanline renderer. DMG and CGB share LCD timing and the RGBA output.
pub struct Screen {
    dots: u32,
    window_line: u8,
    lcd_enabled: bool,
    stat_line: bool,
    bg_colors: [u8; 160],
    bg_priority: [bool; 160],
    pub buffer: LCD,
}

impl Screen {
    pub fn new() -> Self {
        let mut buffer = [0; (SCREEN_HEIGHT * SCREEN_WIDTH * 4) as usize];
        for pixel in buffer.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Self {
            dots: 0,
            window_line: 0,
            lcd_enabled: false,
            stat_line: false,
            bg_colors: [0; 160],
            bg_priority: [false; 160],
            buffer,
        }
    }

    pub fn update_screen(&mut self, mem: &mut Memory, cycles: i32) {
        if !self.is_lcd_enabled(mem) {
            self.dots = 0;
            self.window_line = 0;
            self.lcd_enabled = false;
            mem.write_byte_forced(CURRENT_SCANLINE, 0);
            self.set_lcd_status(mem);
            return;
        }
        if !self.lcd_enabled {
            self.lcd_enabled = true;
            self.dots = 0;
            self.window_line = 0;
        }
        let mut remaining = cycles as u32;
        while remaining != 0 {
            let line = mem.read_byte(CURRENT_SCANLINE);
            let boundary = if line >= 144 {
                456
            } else if self.dots < 80 {
                80
            } else if self.dots < 252 {
                252
            } else {
                456
            };
            let advance = remaining.min(boundary - self.dots);
            self.dots += advance;
            remaining -= advance;
            if self.dots == 252 && line < 144 {
                self.draw_scanline(mem);
                mem.hblank_dma();
            }
            if self.dots == 456 {
                self.dots = 0;
                let next = if line == 153 { 0 } else { line + 1 };
                mem.write_byte_forced(CURRENT_SCANLINE, next);
                if next == 144 {
                    mem.request_interrupt(0);
                }
                if next == 0 {
                    self.window_line = 0;
                }
            }
            self.set_lcd_status(mem);
        }
    }

    fn draw_scanline(&mut self, mem: &Memory) {
        let control = mem.read_byte(LCD_CONTROL);
        self.render_tiles(mem, control);
        if control & 2 != 0 {
            self.render_sprites(mem, control);
        }
    }

    fn render_tiles(&mut self, mem: &Memory, control: u8) {
        let line = mem.read_byte(CURRENT_SCANLINE);
        let cgb = mem.cgb_mode();
        let wx = i16::from(mem.read_byte(0xFF4B)) - 7;
        let wy = mem.read_byte(0xFF4A);
        let window = control & 0x20 != 0 && line >= wy && wx < 160 && (cgb || control & 1 != 0);
        let mut window_drawn = false;
        for x in 0..160usize {
            let in_window = window && x as i16 >= wx;
            let (px, py, map) = if in_window {
                window_drawn = true;
                (
                    (x as i16 - wx) as u8,
                    self.window_line,
                    if control & 0x40 != 0 { 0x9C00 } else { 0x9800 },
                )
            } else {
                (
                    (x as u8).wrapping_add(mem.read_byte(0xFF43)),
                    line.wrapping_add(mem.read_byte(0xFF42)),
                    if control & 8 != 0 { 0x9C00 } else { 0x9800 },
                )
            };
            let map_addr = map + u16::from(py / 8) * 32 + u16::from(px / 8);
            let tile = mem.read_vram(map_addr, 0);
            let attrs = if cgb { mem.read_vram(map_addr, 1) } else { 0 };
            let row = if attrs & 0x40 != 0 {
                7 - (py & 7)
            } else {
                py & 7
            };
            let tile_addr = if control & 0x10 != 0 {
                0x8000 + u16::from(tile) * 16
            } else {
                (0x9000i32 + i32::from(tile as i8) * 16) as u16
            };
            let lo = mem.read_vram(tile_addr + u16::from(row) * 2, (attrs >> 3) & 1);
            let hi = mem.read_vram(tile_addr + u16::from(row) * 2 + 1, (attrs >> 3) & 1);
            let bit = if attrs & 0x20 != 0 {
                px & 7
            } else {
                7 - (px & 7)
            };
            let color = if !cgb && control & 1 == 0 {
                0
            } else {
                ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1)
            };
            self.bg_colors[x] = color;
            self.bg_priority[x] = attrs & 0x80 != 0;
            let rgba = if cgb {
                mem.cgb_color(false, attrs & 7, color)
            } else {
                Self::dmg_color(mem.read_byte(0xFF47), color)
            };
            let index = (line as usize * 160 + x) * 4;
            self.buffer[index..index + 4].copy_from_slice(&rgba);
        }
        if window_drawn {
            self.window_line = self.window_line.wrapping_add(1);
        }
    }

    fn render_sprites(&mut self, mem: &Memory, control: u8) {
        let line = i16::from(mem.read_byte(CURRENT_SCANLINE));
        let height = if control & 4 != 0 { 16 } else { 8 };
        let cgb = mem.cgb_mode();
        let mut sprites = [(0u8, 0u8); 10];
        let mut count = 0;
        for i in 0..40u16 {
            let y = i16::from(mem.read_byte(0xFE00 + i * 4)) - 16;
            if line >= y && line < y + height {
                sprites[count] = (mem.read_byte(0xFE01 + i * 4), i as u8);
                count += 1;
                if count == 10 {
                    break;
                }
            }
        }
        if !cgb {
            sprites[..count].sort_unstable();
        }
        let mut claimed = [false; 160];
        for &(_, sprite) in &sprites[..count] {
            let addr = 0xFE00 + u16::from(sprite) * 4;
            let y = i16::from(mem.read_byte(addr)) - 16;
            let x = i16::from(mem.read_byte(addr + 1)) - 8;
            let attrs = mem.read_byte(addr + 3);
            let mut row = line - y;
            if attrs & 0x40 != 0 {
                row = height - 1 - row;
            }
            let tile = mem.read_byte(addr + 2) & if height == 16 { 0xFE } else { 0xFF };
            let data = 0x8000 + u16::from(tile) * 16 + row as u16 * 2;
            let bank = if cgb { (attrs >> 3) & 1 } else { 0 };
            let lo = mem.read_vram(data, bank);
            let hi = mem.read_vram(data + 1, bank);
            for pixel in 0..8 {
                let pos = x + pixel;
                if !(0..160).contains(&pos) || claimed[pos as usize] {
                    continue;
                }
                let bit = if attrs & 0x20 != 0 { pixel } else { 7 - pixel };
                let color = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);
                if color == 0 {
                    continue;
                }
                claimed[pos as usize] = true;
                let bg_nonzero = self.bg_colors[pos as usize] != 0;
                let behind = attrs & 0x80 != 0 || (cgb && self.bg_priority[pos as usize]);
                if bg_nonzero && behind && (!cgb || control & 1 != 0) {
                    continue;
                }
                let rgba = if cgb {
                    mem.cgb_color(true, attrs & 7, color)
                } else {
                    Self::dmg_color(
                        mem.read_byte(if attrs & 0x10 != 0 { 0xFF49 } else { 0xFF48 }),
                        color,
                    )
                };
                let index = (line as usize * 160 + pos as usize) * 4;
                self.buffer[index..index + 4].copy_from_slice(&rgba);
            }
        }
    }

    fn dmg_color(palette: u8, color: u8) -> [u8; 4] {
        let value = [255, 0xCC, 0x77, 0][((palette >> (color * 2)) & 3) as usize];
        [value, value, value, 255]
    }

    fn set_lcd_status(&mut self, mem: &mut Memory) {
        let line = mem.read_byte(CURRENT_SCANLINE);
        let enabled = self.is_lcd_enabled(mem);
        let mode = if !enabled {
            0
        } else if line >= 144 {
            1
        } else if self.dots < 80 {
            2
        } else if self.dots < 252 {
            3
        } else {
            0
        };
        let old = mem.read_byte(LCD_STATUS);
        let coincidence = line == mem.read_byte(COINCIDENCE_FLAG);
        let status = (old & 0x78) | 0x80 | mode | if coincidence { 4 } else { 0 };
        mem.write_byte_forced(LCD_STATUS, status);
        let irq = enabled
            && ((mode == 0 && status & 8 != 0)
                || (mode == 1 && status & 0x10 != 0)
                || (mode == 2 && status & 0x20 != 0)
                || (coincidence && status & 0x40 != 0));
        if irq && !self.stat_line {
            mem.request_interrupt(1);
        }
        self.stat_line = irq;
    }

    fn is_lcd_enabled(&self, mem: &Memory) -> bool {
        mem.read_byte(LCD_CONTROL) & 0x80 != 0
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ntest::timeout;

    #[test]
    #[timeout(10)]
    fn test_is_lcd_enabled() {
        let mut mem = Memory::new();
        mem.write_byte(LCD_CONTROL, 0x80);
        let screen = Screen::new();
        assert!(screen.is_lcd_enabled(&mem));

        mem.write_byte(LCD_CONTROL, 0x00);
        assert!(!screen.is_lcd_enabled(&mem));
    }

    #[test]
    #[timeout(10)]
    fn test_render_tile_indexing() {
        let mut mem = Memory::new();
        mem.ram_startup();
        let mut screen = Screen::new();

        mem.write_byte_forced(CURRENT_SCANLINE, 1);
        mem.write_byte_forced(0xFF42, 0);
        mem.write_byte_forced(0xFF43, 0);
        mem.write_byte_forced(0xFF4A, 0);
        mem.write_byte_forced(0xFF4B, 7);
        mem.write_byte_forced(0xFF47, 0);
        mem.write_byte_forced(0x9800, 0);
        mem.write_byte_forced(0x8000, 0);
        mem.write_byte_forced(0x8001, 0);

        screen.render_tiles(&mem, 0x31);

        let correct = (SCREEN_WIDTH as usize) * 4;
        assert_eq!(&screen.buffer[correct..correct + 4], &[255, 255, 255, 255]);
        assert_eq!(screen.buffer[1], 0);
        assert!(screen.buffer.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }
}
