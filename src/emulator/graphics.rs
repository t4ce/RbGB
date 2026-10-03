use crate::emulator::mem::*;
use crate::types::*;

/// Basic implementation and methods for the LCD Screen
pub struct Screen {
    scanline_counter: i32,

    // Packed, row-major RGBA8; every pixel has opaque alpha.
    pub buffer: LCD,
}

impl Screen {
    pub fn new() -> Self {
        let mut buffer = [0; (SCREEN_HEIGHT * SCREEN_WIDTH * 4) as usize];
        for pixel in buffer.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Screen {
            buffer,

            scanline_counter: 456,
        }
    }

    pub fn update_screen(&mut self, mem: &mut Memory, cycles: i32) {
        self.set_lcd_status(mem);

        if self.is_lcd_enabled(mem) {
            self.scanline_counter -= cycles;
        } else {
            // LCD is not enabled so do nothing
            return;
        }

        if self.scanline_counter <= 0 {
            // Time to move onto the next scanline
            let scanline = mem.read_byte(CURRENT_SCANLINE).wrapping_add(1);
            mem.write_byte_forced(CURRENT_SCANLINE, scanline);

            self.scanline_counter = 456;

            // we are now in the vertical blank period
            if scanline == 144 {
                mem.request_interrupt(0);
            }
            // If we are past 153, we reset to 0
            else if scanline > 153 {
                mem.write_byte_forced(CURRENT_SCANLINE, 0);
            }
            // Otherwise we draw the current line
            else if scanline < 144 {
                self.draw_scanline(mem);
            }
        }
    }

    fn draw_scanline(&mut self, mem: &Memory) {
        let control = mem.read_byte(LCD_CONTROL);

        if control & (1 << 7) != 0 {
            if control & 0x1 != 0 {
                self.render_tiles(mem, control);
            }

            if control & 0x2 != 0 {
                self.render_sprites(mem, control);
            }
        }
    }

    fn render_tiles(&mut self, mem: &Memory, control: Byte) {
        let mut unsigned = true;
        let tile_data = if control & (1 << 4) != 0 {
            0x8000
        } else {
            unsigned = false;
            0x8800
        };

        // where to draw the visual area and the window
        let scroll_y = mem.read_byte(0xFF42);
        let scroll_x = mem.read_byte(0xFF43);
        let window_y = mem.read_byte(0xFF4A);
        let window_x = mem.read_byte(0xFF4B).wrapping_sub(7);
        let current_line = mem.read_byte(CURRENT_SCANLINE);
        let using_window = (control & (1 << 5) != 0) && window_y <= current_line;

        // determine which tile map to use
        let background_memory: Word = if using_window {
            if control & (1 << 6) != 0 {
                0x9C00
            } else {
                0x9800
            }
        } else if control & (1 << 3) != 0 {
            0x9C00
        } else {
            0x9800
        };

        let y_pos = if using_window {
            current_line.wrapping_sub(window_y)
        } else {
            scroll_y.wrapping_add(current_line)
        };

        let tile_row: Word = ((y_pos / 8) as Word) * 32;

        for pixel in 0..SCREEN_WIDTH as Byte {
            let mut x_pos = pixel.wrapping_add(scroll_x);
            if using_window && pixel >= window_x {
                x_pos = pixel.wrapping_sub(window_x);
            }

            let tile_column = (x_pos / 8) as Word;
            let tile_num = mem.read_byte(background_memory + tile_row + tile_column);

            let mut tile_location: Word = tile_data;
            if unsigned {
                tile_location += tile_num as Word * 16;
            } else {
                let signed = tile_num as i8 as i16;
                tile_location += ((signed + 128) as Word) * 16;
            }

            let line = (y_pos % 8) * 2;
            let data1 = mem.read_byte(tile_location + line as Word);
            let data2 = mem.read_byte(tile_location + line as Word + 1);

            let color_bit = 7 - (x_pos % 8);
            let color_num = (((data2 >> color_bit) & 1) << 1) | ((data1 >> color_bit) & 1);

            let color: Color = mem.get_color(color_num, 0xFF47).unwrap_or(Color::Black);
            let (red, green, blue) = match color {
                Color::White => (255, 255, 255),
                Color::LightGrey => (0xCC, 0xCC, 0xCC),
                Color::DarkGrey => (0x77, 0x77, 0x77),
                Color::Black => (0, 0, 0),
            };

            if current_line as usize >= SCREEN_HEIGHT as usize
                || pixel as usize >= SCREEN_WIDTH as usize
            {
                continue;
            }

            let idx = (current_line as usize * SCREEN_WIDTH as usize + pixel as usize) * 4;
            self.buffer[idx..idx + 4].copy_from_slice(&[red, green, blue, 255]);
        }
    }

    fn render_sprites(&mut self, mem: &Memory, control: Byte) {
        // test if display is enabled
        if control & 0x2 != 0 {
            let mut use8x16 = false;
            if control & 0x4 != 0 {
                use8x16 = true;
            }

            for sprite in 0..40 {
                let index = sprite * 4;
                let y_pos = mem.read_byte(0xFE00 + index) as i32 - 16;
                let x_pos = mem.read_byte(0xFE00 + index + 1) as i32 - 8;
                let tile_location = mem.read_byte(0xFE00 + index + 2);
                let attributes = mem.read_byte(0xFE00 + index + 3);

                let y_flip = attributes & (1 << 6) != 0;
                let x_flip = attributes & (1 << 5) != 0;

                let scanline = mem.read_byte(CURRENT_SCANLINE) as i32;
                let mut y_size = 8;

                if use8x16 {
                    y_size = 16;
                }

                if (scanline >= y_pos) && (scanline < (y_pos + y_size)) {
                    let mut line = scanline - y_pos;

                    if y_flip {
                        line -= y_size;
                        line *= -1;
                    }

                    line *= 2;
                    let data1 =
                        mem.read_byte((0x8000 + (tile_location as Word * 16)) + line as Word);
                    let data2 =
                        mem.read_byte((0x8000 + (tile_location as Word * 16)) + line as Word + 1);

                    for tile_pixel in (0..=7).rev() {
                        let mut color_bit = tile_pixel;

                        if x_flip {
                            color_bit -= 7;
                            color_bit *= -1;
                        }
                        let color_num = (((data2 & (1 << color_bit)) >> color_bit) << 1)
                            | ((data1 & (1 << color_bit)) >> color_bit);

                        let addr = match attributes & (1 << 4) != 0 {
                            true => 0xFF49,
                            false => 0xFF48,
                        };

                        let color = mem.get_color(color_num, addr).unwrap_or(Color::Black);

                        if color == Color::White {
                            continue;
                        }

                        let red;
                        let blue;
                        let green;

                        match color {
                            Color::White => {
                                red = 255;
                                green = 255;
                                blue = 255;
                            }
                            Color::LightGrey => {
                                red = 0xCC;
                                green = 0xCC;
                                blue = 0xCC;
                            }
                            Color::DarkGrey => {
                                red = 0x77;
                                green = 0x77;
                                blue = 0x77;
                            }
                            Color::Black => {
                                red = 0;
                                green = 0;
                                blue = 0;
                            }
                        }

                        let x_pix = 7 - tile_pixel;
                        let pixel = x_pos + x_pix;

                        if pixel < 0
                            || pixel >= SCREEN_WIDTH as i32
                            || scanline < 0
                            || scanline >= SCREEN_HEIGHT as i32
                        {
                            continue;
                        }

                        if attributes & (1 << 7) != 0 {
                            continue; // TODO: Add packground handling
                        }

                        let idx = (scanline as usize * SCREEN_WIDTH as usize + pixel as usize) * 4;
                        self.buffer[idx..idx + 4].copy_from_slice(&[red, green, blue, 255]);
                    }
                }
            }
        }
    }

    fn set_lcd_status(&mut self, mem: &mut Memory) {
        let lcd_enabled = self.is_lcd_enabled(mem);

        let mut status = mem.read_byte(LCD_STATUS);

        // Behavior when lcd is disabled
        if !lcd_enabled {
            // sets the mode to 1 when the lcd is disabled and resets the scanline
            self.scanline_counter = 456;
            mem.write_byte_forced(CURRENT_SCANLINE, 0); // resets scanline

            status &= 0xFC;
            status |= 0x1;
            mem.write_byte(LCD_STATUS, status);
            return;
        }

        let current_line = mem.read_byte(CURRENT_SCANLINE);
        let current_mode = status & 0x3;

        let mode;
        let mut require_interrupt = false;

        // in vblank so mode is set to 1
        if current_line >= 144 {
            mode = 1;
            status |= 0x1;
            status &= !0x2;
            require_interrupt = status & (1 << 4) != 0;
        }
        // mode 2
        else if self.scanline_counter >= MODE_2_BOUNDS {
            mode = 2;
            status |= 0x2;
            status &= !0x1;
            require_interrupt = status & (1 << 5) != 0;
        }
        // mode 3
        else if self.scanline_counter >= MODE_3_BOUNDS {
            mode = 3;
            status |= 0x3;
        }
        // mode 0
        else {
            mode = 0;
            status &= !0x3;
            require_interrupt = status & (1 << 3) != 0;
        }

        // we have no entered a new mode
        if require_interrupt && (mode != current_mode) {
            mem.request_interrupt(1);
        }

        // check the conicidence flag
        if current_line == mem.read_byte(COINCIDENCE_FLAG) {
            status |= 0x4;
            if status & (1 << 6) != 0 {
                mem.request_interrupt(1);
            }
        } else {
            status &= !0x4;
        }

        // Write the new status
        mem.write_byte(LCD_STATUS, status);
    }

    fn is_lcd_enabled(&self, mem: &Memory) -> bool {
        // Check bit 7 of LCD Control register (0xFF40)
        mem.read_byte(LCD_CONTROL) & (1 << 7) != 0
    }

    // TODO: Add more methods for drawing, sprites, etc.
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
