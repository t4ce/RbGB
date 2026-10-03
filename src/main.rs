// trueos-blueprint: features = ["ui4-scene"]

extern crate alloc;

mod audio;
mod commands;
mod pacing;
mod view;

use alloc::{format, string::String};
use commands::{Command, LineInput};
use rbgb::{Emulator, GameInput, KeyState, MAX_ROM_SIZE};
use trueos::ui4_scene::{
    Damage, Error as Ui4Error, Frame, KeyboardState, SpriteCorner, SpriteQuad, output_dimensions,
    rgba,
};
use trueos::{async_fs, clock, env, logl, vshell, vsys};
use view::{HEIGHT, View, WIDTH};

const SPRITE_ID: u32 = 1;

fn main() {
    if let Err(error) = run() {
        logl::log(logl::level::ERROR, format_args!("rbgb: {error}"));
    }
}

fn run() -> Result<(), String> {
    let mut emulator = Emulator::new();
    // The launcher creates this app root; relative FS paths resolve inside it.
    let directory = env::var("TRUEOS_APP_FS_ROOT").unwrap_or_else(|_| String::from("apps/rbgb"));
    if let Some(name) = env::args().skip(1).find(|arg| !arg.starts_with("--")) {
        if let Err(error) = load_rom(&mut emulator, &directory, &name) {
            vshell::linef(format_args!("rbgb: {error}"));
        }
    }

    let (output_width, output_height) = output_dimensions().unwrap_or((WIDTH, HEIGHT));
    let initial_scale = (output_width / WIDTH)
        .min(output_height / HEIGHT)
        .clamp(1, 3);
    let width = WIDTH * initial_scale;
    let height = HEIGHT * initial_scale;
    let mut frame = Frame::open_streaming(
        output_width.saturating_sub(width) as i32 / 2,
        output_height.saturating_sub(height) as i32 / 2,
        width,
        height,
    )
    .map_err(|error| format!("UI4 frame open failed: {error:?}"))?;
    vshell::linef(format_args!(
        "rbgb: ROM folder {directory}; commands: list, load <filename>, help"
    ));
    print_controls();
    let mut shell = LineInput::new();
    let mut presenter = Presenter::new();
    let mut pending_resize = None;
    let mut audio = audio::AudioOutput::new();
    let mut pacer = pacing::Pacer::new(clock::monotonic_nanos());

    loop {
        let loaded = poll_commands(&mut shell, &mut emulator, &directory);
        if loaded {
            audio.reset();
            pacer.reset(clock::monotonic_nanos());
            presenter.dirty = true;
        }
        let ui_work = (|| {
            while let Some(event) = frame.take_resize_event()? {
                // A native pixel is the minimum; never crop or shrink the LCD.
                pending_resize = Some((event.width.max(WIDTH), event.height.max(HEIGHT)));
            }
            if let Some((width, height)) = pending_resize.filter(|_| presenter.can_resize()) {
                match frame.resize(width, height) {
                    Ok(()) => {
                        pending_resize = None;
                        presenter.dirty = true;
                    }
                    Err(Ui4Error::Busy) => {}
                    Err(error) => return Err(error),
                }
            }
            let keyboard = frame.keyboard_state()?;
            apply_keyboard(&mut emulator, keyboard.as_ref());
            // Held state drives the joypad; consume queued text transitions too.
            for _ in 0..64 {
                if frame.take_keyboard_event()?.is_none() {
                    break;
                }
            }
            Ok(())
        })();
        if let Err(error) = ui_work {
            return ui_result(error);
        }

        let running = !emulator.is_paused();
        audio.pump(&mut emulator);
        if running {
            // Catch up the native clock without requiring a GPU publication
            // for each frame. Pump between updates and retain all short writes.
            for _ in 0..4 {
                if !pacer.due(clock::monotonic_nanos()) {
                    break;
                }
                // Leave room for one LCD frame plus the largest DMA overshoot.
                // Backpressure pauses production rather than dropping notes.
                if emulator.audio_samples().len() / 2 > 3200 {
                    break;
                }
                emulator.update();
                pacer.advance();
                presenter.dirty = true;
                audio.pump(&mut emulator);
            }
        } else {
            pacer.reset(clock::monotonic_nanos());
            pacer.advance();
        }
        if let Err(error) = presenter.advance(&mut frame, emulator.get_display_buffer()) {
            return ui_result(error);
        }

        vsys::poll_once();
        let remaining = pacer.wait_ns(clock::monotonic_nanos());
        if remaining > 0 {
            let wait_ms = remaining.div_ceil(1_000_000);
            vsys::sleep_ms(if presenter.pending() {
                wait_ms.min(1)
            } else {
                wait_ms
            });
        } else {
            // Audio backpressure can temporarily leave the native clock due.
            vsys::sleep_ms(1);
        }
    }
}

fn ui_result(error: Ui4Error) -> Result<(), String> {
    // UI4 closes the frame on Escape by default.
    if error == Ui4Error::NotFound {
        Ok(())
    } else {
        Err(format!("UI4 error: {error:?}"))
    }
}

fn screen_quad(frame: &Frame) -> SpriteQuad {
    let view = View::fit(frame.width(), frame.height());
    let corner = |x, y, u, v| SpriteCorner {
        x: x as f32,
        y: y as f32,
        u,
        v,
    };
    SpriteQuad {
        sprite_id: SPRITE_ID,
        c0: corner(view.x, view.y, 0.0, 0.0),
        c1: corner(view.x + view.width, view.y, 1.0, 0.0),
        c2: corner(view.x + view.width, view.y + view.height, 1.0, 1.0),
        c3: corner(view.x, view.y + view.height, 0.0, 1.0),
        color_rgba: rgba(255, 255, 255, 255),
        source_over: false,
    }
}

#[derive(Clone, Copy)]
enum PresentStage {
    Upload,
    Begin,
    Draw,
    Publish,
}

struct Presenter {
    stage: PresentStage,
    dirty: bool,
}

impl Presenter {
    fn new() -> Self {
        Self {
            stage: PresentStage::Upload,
            dirty: true,
        }
    }

    fn can_resize(&self) -> bool {
        matches!(self.stage, PresentStage::Upload)
    }

    fn pending(&self) -> bool {
        !self.can_resize() || self.dirty
    }

    fn advance(&mut self, frame: &mut Frame, pixels: &[u8]) -> Result<(), Ui4Error> {
        // Busy retains the current lease/stage and returns to the audio loop.
        // A warm sprite is updated only after the preceding frame is published.
        for _ in 0..4 {
            let result = match self.stage {
                PresentStage::Upload => {
                    if !self.dirty {
                        return Ok(());
                    }
                    frame.upload_sprite_rgba8(SPRITE_ID, WIDTH, HEIGHT, pixels)
                }
                PresentStage::Begin => frame.begin_sprite_frame(rgba(0, 0, 0, 0)),
                PresentStage::Draw => frame.draw_sprite_quads(&[screen_quad(frame)]),
                PresentStage::Publish => {
                    frame.publish_compute(Damage::full(frame.width(), frame.height()))
                }
            };
            match result {
                Err(Ui4Error::Busy) => return Ok(()),
                Err(error) => return Err(error),
                Ok(()) => {}
            }
            self.stage = match self.stage {
                PresentStage::Upload => {
                    self.dirty = false;
                    PresentStage::Begin
                }
                PresentStage::Begin => PresentStage::Draw,
                PresentStage::Draw => PresentStage::Publish,
                PresentStage::Publish => {
                    self.stage = PresentStage::Upload;
                    return Ok(());
                }
            };
        }
        Ok(())
    }
}

fn apply_keyboard(emulator: &mut Emulator, keyboard: Option<&KeyboardState>) {
    // USB HID usages; retain the original SDL bindings and add common aliases.
    for (input, primary, alias) in [
        (GameInput::Up, 0x1A, 0x52),     // W / Up
        (GameInput::Down, 0x16, 0x51),   // S / Down
        (GameInput::Left, 0x04, 0x50),   // A / Left
        (GameInput::Right, 0x07, 0x4F),  // D / Right
        (GameInput::Select, 0x14, 0x2A), // Q / Backspace
        (GameInput::Start, 0x08, 0x28),  // E / Enter
        (GameInput::A, 0x1D, 0x1D),      // Z
        (GameInput::B, 0x1B, 0x1B),      // X
    ] {
        let down = keyboard.is_some_and(|state| state.is_down(primary) || state.is_down(alias));
        // No routed keyboard releases every button when the frame loses focus.
        emulator.game_input(
            input,
            if down {
                KeyState::Pressed
            } else {
                KeyState::Released
            },
        );
    }
}

fn print_controls() {
    vshell::line(
        "Click the game frame for controls: WASD/arrows, Z=A, X=B, E/Enter=Start, Q/Backspace=Select",
    );
}

fn list_roms(directory: &str) -> Result<(), String> {
    let listing = async_fs::block_on(async_fs::list_dir(directory.as_bytes()))
        .map_err(|code| format!("cannot list {directory} (code {code})"))?;
    let mut names = listing
        .entries
        .into_iter()
        .filter(|entry| entry.kind == async_fs::NodeKind::File && commands::rom_name(&entry.name))
        .map(|entry| entry.name)
        .collect::<alloc::vec::Vec<_>>();
    names.sort();
    if names.is_empty() {
        vshell::linef(format_args!("No .gb or .gbc files in {directory}"));
    }
    for name in names {
        vshell::line(&name);
    }
    if listing.truncated {
        vshell::line("Directory listing truncated; load a known filename directly.");
    }
    Ok(())
}

fn load_rom(emulator: &mut Emulator, directory: &str, name: &str) -> Result<(), String> {
    if !commands::rom_name(name) {
        return Err(String::from(
            "use load <filename.gb|filename.gbc> from the ROM folder",
        ));
    }
    let path = format!("{}/{name}", directory.trim_end_matches('/'));
    let metadata = async_fs::block_on(async_fs::metadata(path.as_bytes()))
        .map_err(|code| format!("cannot open {name} (code {code})"))?;
    if metadata.kind != async_fs::NodeKind::File
        || !(0x150..=MAX_ROM_SIZE as u64).contains(&metadata.len)
    {
        return Err(format!(
            "unsupported ROM: {name} ({} bytes; core supports up to {} MiB)",
            metadata.len,
            MAX_ROM_SIZE / (1024 * 1024)
        ));
    }
    let rom = async_fs::block_on(async_fs::read_file(path.as_bytes()))
        .map_err(|code| format!("ROM read failed: {name} (code {code})"))?;
    if !(0x150..=MAX_ROM_SIZE).contains(&rom.len()) {
        return Err(format!("unsupported ROM size: {} bytes", rom.len()));
    }
    emulator.load_rom_data(&rom);
    let mode = if emulator.is_cgb() { "CGB" } else { "DMG" };
    vshell::linef(format_args!("Loaded {name} ({} bytes; {mode})", rom.len()));
    Ok(())
}

fn poll_commands(shell: &mut LineInput, emulator: &mut Emulator, directory: &str) -> bool {
    let mut loaded = false;
    // Bound console work so a continuous input stream cannot starve frames.
    for _ in 0..256 {
        let Some(byte) = vshell::attached_read_byte() else {
            break;
        };
        // The VMX minishell owns line editing, echo, and its prompt.
        let line = shell.push(byte);
        let Some(line) = line else { continue };
        if line.as_ref().is_ok_and(|line| line.trim().is_empty()) {
            continue;
        }
        let result = match line {
            Ok(line) => match commands::parse(&line) {
                Command::List => list_roms(directory),
                Command::Load(name) => load_rom(emulator, directory, name).map(|()| loaded = true),
                Command::Help => {
                    vshell::linef(format_args!(
                        "Commands: list; load <filename.gb|filename.gbc> from {directory}"
                    ));
                    print_controls();
                    Ok(())
                }
                Command::Empty => Ok(()),
                Command::Invalid => Err(String::from("commands: list, load <filename>, help")),
            },
            Err(error) => Err(String::from(error)),
        };
        if let Err(error) = result {
            vshell::linef(format_args!("rbgb: {error}"));
        }
    }
    loaded
}
