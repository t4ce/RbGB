//! Contains all code for interfacing io to the gameboy
use crate::types::{GameInput, KeyState};

use super::mem::Memory;

#[derive(Default)]
pub struct Joypad {
    // group 1 buttons
    a: KeyState,
    b: KeyState,
    start: KeyState,
    select: KeyState,

    // group 2 buttons
    right: KeyState,
    left: KeyState,
    up: KeyState,
    down: KeyState,
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            ..Default::default()
        }
    }

    /// Takes the input and affects the associated memory value with what it should be
    pub fn log_input(&mut self, mem: &mut Memory, input: GameInput, val: KeyState) {
        match input {
            // mode 0 buttons
            GameInput::A => self.a = val,
            GameInput::B => self.b = val,
            GameInput::Start => self.start = val,
            GameInput::Select => self.select = val,
            GameInput::Up => self.up = val,
            GameInput::Down => self.down = val,
            GameInput::Left => self.left = val,
            GameInput::Right => self.right = val,

            // unknown input
            _ => (),
        }

        self.write_input_to_mem(mem);
    }

    /// Main hardworking function that does the work to write the joypad state to RAM
    fn write_input_to_mem(&mut self, mem: &mut Memory) {
        let mut buttons = 0;
        buttons |= self.a as u8;
        buttons |= (self.b as u8) << 1;
        buttons |= (self.select as u8) << 2;
        buttons |= (self.start as u8) << 3;

        let mut directions = 0;
        directions |= self.right as u8;
        directions |= (self.left as u8) << 1;
        directions |= (self.up as u8) << 2;
        directions |= (self.down as u8) << 3;

        mem.update_joypad_state(buttons, directions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IF;
    use crate::{emulator::mem::Memory, types::INPUT_REGISTER};

    fn setup_joypad(mem: &mut Memory, select_bits: u8) -> Joypad {
        mem.write_byte_forced(INPUT_REGISTER, 0xC0 | select_bits | 0x0F);
        Joypad::new()
    }

    #[test]
    fn buttons_selection_updates_lower_nibble_and_interrupts() {
        let mut mem = Memory::new();
        let mut joypad = setup_joypad(&mut mem, 0x10); // select buttons
        joypad.log_input(&mut mem, GameInput::A, KeyState::Pressed);

        let value = mem.read_byte_forced(INPUT_REGISTER);
        assert_eq!(value, 0xDE);

        let if_val = mem.read_byte_forced(IF);
        assert_eq!(if_val & (1 << 4), 1 << 4);
    }

    #[test]
    fn directions_selection_updates_lower_nibble() {
        let mut mem = Memory::new();
        let mut joypad = setup_joypad(&mut mem, 0x20); // select directions
        joypad.log_input(&mut mem, GameInput::Up, KeyState::Pressed);

        let value = mem.read_byte_forced(INPUT_REGISTER);
        assert_eq!(value, 0xEB);
    }

    #[test]
    fn no_selection_keeps_low_nibble_high_and_no_interrupt() {
        let mut mem = Memory::new();
        let mut joypad = setup_joypad(&mut mem, 0x30); // no selection
        joypad.log_input(&mut mem, GameInput::A, KeyState::Pressed);

        let value = mem.read_byte_forced(INPUT_REGISTER);
        assert_eq!(value, 0xFF);

        let if_val = mem.read_byte_forced(IF);
        assert_eq!(if_val & (1 << 4), 0);
    }

    #[test]
    fn release_does_not_request_interrupt() {
        let mut mem = Memory::new();
        let mut joypad = setup_joypad(&mut mem, 0x10); // select buttons
        joypad.log_input(&mut mem, GameInput::A, KeyState::Pressed);
        mem.write_byte_forced(IF, 0x00);

        joypad.log_input(&mut mem, GameInput::A, KeyState::Released);
        let if_val = mem.read_byte_forced(IF);
        assert_eq!(if_val & (1 << 4), 0);
    }
}
