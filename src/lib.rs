#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
//! Game Boy emulator core.
//!
//! This crate provides the core emulation loop and subsystems for a classic
//! Game Boy. The primary entry point is [`Emulator`], a struct that owns the
//! CPU, memory, graphics, and input devices and drives them per frame.
//!
//! High-level flow:
//! - Create an [`Emulator`] (starts paused).
//! - Load a ROM with [`Emulator::load_rom`], which resets memory and CPU state.
//! - Call [`Emulator::update`] once per frame to advance CPU, timers, and video.
//! - Provide input through [`Emulator::game_input`], and read pixels from
//!   [`Emulator::get_display_buffer`].
//!
//! The emulation state is shared across CPU, graphics, and joypad through a
//! single memory model owned by the emulator core.
//!
//! # Example
//! ```no_run
//! use rbgb::Emulator;
//!
//! fn main() -> Result<(), String> {
//!     let mut emu = Emulator::new();
//!     emu.load_rom("path/to/game.gb")?;
//!     loop {
//!         emu.update();
//!         let _pixels = emu.get_display_buffer();
//!         // Render pixels and handle input here.
//!     }
//! }
//! ```

/// The main emulator core
pub mod emulator;
mod types;

pub use emulator::*;
extern crate alloc;

pub use types::{GameInput, KeyState, MAX_ROM_SIZE};
