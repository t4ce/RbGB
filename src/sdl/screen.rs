use std::{
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

use crate::{
    emulator::Emulator,
    types::{SCREEN_HEIGHT, SCREEN_WIDTH},
};
use sdl2::{
    event::Event,
    keyboard::Keycode,
    pixels::{Color, PixelFormatEnum},
    rect::Rect,
    render::Texture,
};

use super::io::handle_joystick_input;

// Window size multiplier so original 160x144 framebuffer is easier to see
const WINDOW_SCALE: u32 = 5;

pub struct SdlApp {
    _sdl_context: sdl2::Sdl,
    event_pump: sdl2::EventPump,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
}

impl SdlApp {
    pub fn new() -> Result<Self, String> {
        let sdl_context = sdl2::init()?;
        let video_subsystem = sdl_context.video()?;
        let window = video_subsystem
            .window(
                "Gameboy Emulator",
                SCREEN_WIDTH * WINDOW_SCALE,
                SCREEN_HEIGHT * WINDOW_SCALE,
            )
            .position_centered()
            .build()
            .map_err(|e| e.to_string())?;

        let canvas = window
            .into_canvas()
            .accelerated()
            .build()
            .map_err(|e| e.to_string())?;

        let event_pump = sdl_context.event_pump()?;

        Ok(Self {
            _sdl_context: sdl_context,
            event_pump,
            canvas,
        })
    }

    pub fn run(&mut self, emulator: &mut Emulator) -> Result<(), String> {
        // Create the streaming texture once per run for packed RGBA bytes.
        let texture_creator = self.canvas.texture_creator();
        let mut texture = texture_creator
            .create_texture_streaming(PixelFormatEnum::RGBA32, SCREEN_WIDTH, SCREEN_HEIGHT)
            .map_err(|e| e.to_string())?;

        'running: loop {
            let frame_start = Instant::now();

            // Process all queued SDL events before running a frame
            while let Some(event) = self.event_pump.poll_event() {
                if !Self::handle_event(event, emulator) {
                    break 'running;
                }
            }

            // Advance the emulator state, copy the LCD buffer into SDL, then render
            emulator.update();
            Self::blit_rgba_bytes_to_texture(emulator, &mut texture)?;

            self.draw(emulator.is_paused(), &texture)?;
            self.limit_frame_rate(frame_start);
        }

        Ok(())
    }

    // Returns false when the emulator should stop running (e.g. window closed)
    fn handle_event(event: Event, emulator: &mut Emulator) -> bool {
        match event {
            Event::Quit { .. } => false,
            Event::KeyDown {
                keycode: Some(Keycode::P),
                ..
            } => {
                emulator.toggle_pause();
                true
            }
            Event::KeyDown {
                keycode: Some(Keycode::L),
                ..
            } => {
                print!("Enter path to ROM: ");
                if io::stdout().flush().is_ok() {
                    let mut path = String::new();
                    if io::stdin().read_line(&mut path).is_err() {
                        println!("Failed to read ROM path from stdin");
                    } else {
                        let trimmed = path.trim();
                        if trimmed.is_empty() {
                            println!("No ROM path entered");
                        } else if let Err(e) = emulator.load_rom(trimmed) {
                            println!("Failed to load ROM: {e}");
                        } else {
                            println!("ROM loaded");
                        }
                    }
                }
                true
            }
            // Dump the lcd memory details
            Event::KeyDown {
                keycode: Some(Keycode::O),
                ..
            } => {
                emulator.dump_lcd_mem();
                true
            }

            // handle all remaining inputs as a game input
            event => {
                handle_joystick_input(event, emulator);
                true
            }
        }
    }

    fn draw(&mut self, paused: bool, texture: &sdl2::render::Texture) -> Result<(), String> {
        self.canvas.clear();
        self.canvas.copy(
            texture,
            None,
            Some(Rect::new(
                0,
                0,
                SCREEN_WIDTH * WINDOW_SCALE,
                SCREEN_HEIGHT * WINDOW_SCALE,
            )),
        )?;

        if paused {
            self.canvas.set_draw_color(Color::RGB(50, 50, 50));
            self.canvas.fill_rect(Rect::new(
                0,
                0,
                SCREEN_WIDTH * WINDOW_SCALE,
                SCREEN_HEIGHT * WINDOW_SCALE,
            ))?;
        }

        self.canvas.present();
        Ok(())
    }

    fn blit_rgba_bytes_to_texture(
        emulator: &Emulator,
        texture: &mut Texture,
    ) -> Result<(), String> {
        let data = emulator.get_display_buffer();
        let pitch = (SCREEN_WIDTH * 4) as usize; // 4 bytes per pixel
        let expected_len = pitch * SCREEN_HEIGHT as usize;

        if data.len() != expected_len {
            return Err(format!(
                "Expected {} bytes, but got {}",
                expected_len,
                data.len()
            ));
        }

        texture
            .update(None, data, pitch)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // Simple 60 FPS limiter so SDL doesn't run as fast as possible
    fn limit_frame_rate(&self, frame_start: Instant) {
        let frame_duration = frame_start.elapsed();
        if frame_duration < Duration::from_millis(16) {
            thread::sleep(Duration::from_millis(16) - frame_duration);
        }
    }
}
