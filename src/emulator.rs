use crate::types::{GameInput, KeyState};

mod cgb;
mod cpu;
mod graphics;
mod joypad;
mod mem;
mod sound;

/// High-level Game Boy emulator coordinator.
///
/// Owns the CPU, memory, graphics, and input subsystems and drives the
/// per-frame execution loop. The emulator starts paused and must be unpaused
/// (or a ROM loaded) before it will execute instructions.
///
/// This type exposes the core API used by the frontend to load ROMs, advance
/// frames, provide input, and read the display buffer.
pub struct Emulator {
    screen: graphics::Screen,
    cpu: cpu::CPU,
    joypad: joypad::Joypad,
    paused: bool,
    frame_dot_debt: u32,
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Emulator {
    /// Master-clock dots per LCD frame, independent of CPU speed.
    const FRAME_DOTS: u32 = 456 * 154;

    /// Create a new emulator instance with initialized subsystems.
    ///
    /// Memory is initialized to its startup state and the emulator begins
    /// paused until a ROM is loaded or `toggle_pause` is called.
    ///
    /// Returns a ready-to-use `Emulator` in the paused state.
    pub fn new() -> Self {
        let mut cpu = cpu::CPU::new();
        cpu.memory_mut().ram_startup();
        Emulator {
            screen: graphics::Screen::new(),
            cpu,
            joypad: joypad::Joypad::new(),
            paused: true,
            frame_dot_debt: 0,
        }
    }

    /// Execute one frame of emulation if not paused.
    ///
    /// Runs CPU instructions, updates timers and graphics, and handles
    /// interrupts until the frame's cycle budget is consumed.
    ///
    /// Returns `()` and has no effect if the emulator is paused.
    pub fn update(&mut self) {
        if self.paused {
            return;
        }
        let mut dots = self.frame_dot_debt;
        while dots < Self::FRAME_DOTS {
            let double_speed = self.cpu.memory_mut().double_speed();
            let dma_cycles = self.cpu.memory_mut().take_dma_cycles();
            let cycles = if dma_cycles != 0 {
                dma_cycles
            } else {
                self.cpu.execute_next_opcode(false) as u32
            };
            let elapsed_dots = if double_speed { cycles / 2 } else { cycles };
            dots += elapsed_dots;
            self.cpu.update_timers(cycles as i32);
            self.screen
                .update_screen(self.cpu.memory_mut(), elapsed_dots as i32);
            self.cpu.memory_mut().sound.step(elapsed_dots);
        }
        self.frame_dot_debt = dots - Self::FRAME_DOTS;
    }

    /// Toggle the paused state.
    ///
    /// When paused, `update` returns immediately without advancing emulation.
    ///
    /// Takes `&mut self` and returns `()`.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Check whether emulation is currently paused.
    ///
    /// Returns `true` if the emulator will skip work in `update`.
    ///
    /// Takes `&self` and returns a `bool` indicating pause state.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Load ROM data and reset CPU/memory state.
    pub fn load_rom_data(&mut self, data: &[u8]) {
        let mem = self.cpu.memory_mut();
        mem.load_rom_data(data);
        mem.ram_startup();
        self.cpu.reset();
        self.screen = graphics::Screen::new();
        self.joypad = joypad::Joypad::new();
        self.frame_dot_debt = 0;
        self.paused = false;
    }

    /// Whether the loaded cartridge runs in native Game Boy Color mode.
    pub fn is_cgb(&mut self) -> bool {
        self.cpu.memory_mut().cgb_mode()
    }

    /// Pending interleaved left/right signed 16-bit PCM at 48 kHz.
    pub fn audio_samples(&mut self) -> &[i16] {
        self.cpu.memory_mut().sound.samples()
    }

    /// Retire PCM frames accepted by the audio output, retaining short writes.
    pub fn consume_audio_frames(&mut self, frames: usize) {
        self.cpu.memory_mut().sound.consume_frames(frames);
    }

    #[cfg(feature = "std")]
    /// Load a ROM from disk and reset the CPU and memory state.
    ///
    /// The ROM contents are copied into memory, memory is reinitialized, and
    /// the CPU is reset. On success, the emulator is unpaused.
    ///
    /// Parameters:
    /// - `path`: filesystem path to the ROM file.
    ///
    /// Returns `Ok(())` on success or a string I/O error on failure.
    pub fn load_rom(&mut self, path: &str) -> Result<(), std::string::String> {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        self.load_rom_data(&data);
        Ok(())
    }

    /// Borrow the current display buffer for rendering.
    ///
    /// The buffer is a tightly packed 160 x 144 RGBA8 image in row-major order
    /// (92,160 bytes). Each pixel contains red, green, blue, then alpha (255).
    ///
    /// Returns a borrowed `&[u8]` slice tied to the emulator's lifetime.
    pub fn get_display_buffer(&self) -> &[u8] {
        &self.screen.buffer
    }

    /// Prints our all relevant memory locations into the stdout
    pub fn dump_lcd_mem(&self) {
        #[cfg(feature = "std")]
        {
            let mem = self.cpu.memory();
            println!("IDK: {:X}", mem.read_byte_forced(0xFF26));
            println!(
                "LCD Control: {:X}",
                mem.read_byte_forced(crate::types::LCD_CONTROL)
            );
            println!("Scroll Y: {:X}", mem.read_byte_forced(0xFF42));
            println!("Scroll X: {:X}", mem.read_byte_forced(0xFF43));
            println!("BG Palette: {:X}", mem.read_byte_forced(0xFF47));
            println!("OBJ palette: {:X}", mem.read_byte_forced(0xFF48));
            println!(
                "Current Scanline: {:X}",
                mem.read_byte_forced(crate::types::CURRENT_SCANLINE)
            );
            println!(
                "Joystick Register: 0x{:X}",
                mem.read_byte_forced(crate::types::INPUT_REGISTER)
            );
        }
    }

    /// Handle input for the emulator's joypad.
    ///
    /// Updates the joypad state and forwards the input to memory-mapped input
    /// registers.
    ///
    /// Parameters:
    /// - `input`: which Game Boy control was pressed or released.
    /// - `val`: the key state for that control.
    ///
    /// Returns `()`.
    pub fn game_input(&mut self, input: GameInput, val: KeyState) {
        self.joypad.log_input(self.cpu.memory_mut(), input, val)
    }
}
