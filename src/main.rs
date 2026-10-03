// trueos-blueprint: features = ["ui4-scene"]

extern crate alloc;

use alloc::{format, string::String};
use rbgb::Emulator;
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, output_dimensions, rgba};
use trueos::{async_fs, clock, env, logl, vsys};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 144;
const FRAME_MS: u64 = 17;

fn main() {
    if let Err(error) = run() {
        logl::log(logl::level::ERROR, format_args!("rbgb: {error}"));
    }
}

fn run() -> Result<(), String> {
    let mut emulator = Emulator::new();
    if let Some(path) = env::args().nth(1) {
        let rom = async_fs::block_on(async_fs::read_file(path.as_bytes()))
            .map_err(|code| format!("ROM read failed: {path} (code {code})"))?;
        // The core stores at most 512 KiB and silently truncates larger ROMs.
        if !(0x150..=0x80000).contains(&rom.len()) {
            return Err(format!("unsupported ROM size: {} bytes", rom.len()));
        }
        emulator.load_rom_data(&rom);
    }

    let (output_width, output_height) = output_dimensions().unwrap_or((WIDTH, HEIGHT));
    let mut frame = Frame::open_streaming(
        output_width.saturating_sub(WIDTH) as i32 / 2,
        output_height.saturating_sub(HEIGHT) as i32 / 2,
        WIDTH,
        HEIGHT,
    )
    .map_err(|error| format!("UI4 frame open failed: {error:?}"))?;
    logl::log(
        logl::level::INFO,
        "rbgb: UI4 frame ready; optional argument: ROM path",
    );

    loop {
        let started = clock::monotonic_millis();
        emulator.update();

        let present = (|| {
            retry_busy(|| frame.begin(rgba(0, 0, 0, 255)))?;
            retry_busy(|| frame.write_opaque_rgba8(emulator.get_display_buffer()))?;
            retry_busy(|| frame.publish(Damage::full(WIDTH, HEIGHT)))
        })();
        match present {
            Ok(()) => {}
            // UI4 closes the frame on Escape by default.
            Err(Ui4Error::NotFound) => return Ok(()),
            Err(error) => return Err(format!("UI4 present failed: {error:?}")),
        }

        vsys::poll_once();
        let remaining = FRAME_MS.saturating_sub(clock::monotonic_millis() - started);
        if remaining > 0 {
            vsys::sleep_ms(remaining);
        }
    }
}

fn retry_busy(mut operation: impl FnMut() -> Result<(), Ui4Error>) -> Result<(), Ui4Error> {
    loop {
        match operation() {
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            result => return result,
        }
    }
}
