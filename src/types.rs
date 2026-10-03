pub type Byte = u8;
pub type Word = u16;
pub type Ram = [Byte; MEM_SIZE];
#[allow(clippy::upper_case_acronyms)]
pub type LCD = [Byte; (SCREEN_HEIGHT * SCREEN_WIDTH * 4) as usize];

// Timer and CPU constants
pub const TIMA: Word = 0xFF05;
pub const TMA: Word = 0xFF06;
pub const TMC: Word = 0xFF07;
pub const DIVIDER_REGISTER: Word = 0xFF04;

// Interrupt Constants
pub const IE: Word = 0xFFFF; // Interrupt enabled register
pub const IF: Word = 0xFF0F; // Interrupt request register

// Screen Constants
pub const SCREEN_HEIGHT: u32 = 144;
pub const SCREEN_WIDTH: u32 = 160;
pub const CURRENT_SCANLINE: Word = 0xFF44;
pub const LCD_STATUS: Word = 0xFF41;
pub const LCD_CONTROL: Word = 0xFF40;
pub const COINCIDENCE_FLAG: Word = 0xFF45;
pub const DMA_REG: Word = 0xFF46;
pub const SPRITE_RAM: Word = 0xFE00; // from 0xFE00 to OxFE9F
pub const MODE_2_BOUNDS: i32 = 456 - 80;
pub const MODE_3_BOUNDS: i32 = MODE_2_BOUNDS - 172;

// Input Constants
pub const INPUT_REGISTER: Word = 0xFF00;

#[derive(Debug)]
pub enum GameInput {
    Up,
    Left,
    Right,
    Down,
    Start,
    Select,
    A,
    B,
    Unknown,
}

#[derive(Copy, Clone, Default)]
pub enum KeyState {
    Pressed = 0,
    #[default]
    Released = 1,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    White,
    LightGrey,
    DarkGrey,
    Black,
}

/// RAM  Device Memory
pub const MEM_SIZE: usize = 0x10000;
pub const MAX_ROM_SIZE: usize = 0x80000; // 512 KiB

//Likely at some point will switch the RAM and ROM to be part of the Emulator struct

#[derive(PartialEq, Debug)]
pub enum RomBankingType {
    MBC1,
    MBC2,
    None,
}

// I thought I was smart and would implement the rom banks as enums but little did I know...
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CurrentRomBank {
    Bank(u8),
}

impl From<u8> for CurrentRomBank {
    fn from(val: u8) -> Self {
        CurrentRomBank::Bank(val)
    }
}

impl CurrentRomBank {
    pub fn value(self) -> u8 {
        match self {
            CurrentRomBank::Bank(val) => val,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurrentRamBank {
    Bank0,
    Bank1,
    Bank2,
    Bank3,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Error {
    ColorReadError,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_current_rom_bank_conversion() {
        let bank: CurrentRomBank = 5u8.into();
        assert_eq!(bank.value(), 5);
    }
}
