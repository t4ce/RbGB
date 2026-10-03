//! CGB memory and palette state. CPU instructions are shared with DMG.

use alloc::{vec, vec::Vec};

pub(super) struct Cgb {
    pub vram: Vec<u8>,
    pub wram: Vec<u8>,
    pub vram_bank: u8,
    pub wram_bank: u8,
    pub key1: u8,
    pub bg_index: u8,
    pub obj_index: u8,
    pub bg_palette: [u8; 64],
    pub obj_palette: [u8; 64],
    pub dma_source: u16,
    pub dma_dest: u16,
    pub dma_blocks: u8,
    pub dma_active: bool,
}

impl Cgb {
    pub fn new() -> Self {
        Self {
            vram: vec![0; 0x2000],
            wram: vec![0; 0x7000],
            vram_bank: 0,
            wram_bank: 1,
            key1: 0,
            bg_index: 0,
            obj_index: 0,
            bg_palette: [0xFF; 64],
            obj_palette: [0xFF; 64],
            dma_source: 0,
            dma_dest: 0,
            dma_blocks: 0,
            dma_active: false,
        }
    }

    pub fn read_register(&self, addr: u16) -> Option<u8> {
        Some(match addr {
            0xFF4D => self.key1 | 0x7E,
            0xFF4F => self.vram_bank | 0xFE,
            0xFF51..=0xFF54 => 0xFF, // DMA address registers are write-only.
            0xFF55 => {
                if self.dma_blocks == 0 {
                    0xFF
                } else {
                    (self.dma_blocks - 1) | if self.dma_active { 0 } else { 0x80 }
                }
            }
            0xFF68 => self.bg_index | 0x40,
            0xFF69 => self.bg_palette[(self.bg_index & 0x3F) as usize],
            0xFF6A => self.obj_index | 0x40,
            0xFF6B => self.obj_palette[(self.obj_index & 0x3F) as usize],
            0xFF6C => 0xFE,
            0xFF70 => self.wram_bank | 0xF8,
            _ => return None,
        })
    }

    pub fn write_register(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0xFF4D => self.key1 = (self.key1 & 0x80) | (value & 1),
            0xFF4F => self.vram_bank = value & 1,
            0xFF51 => self.dma_source = (u16::from(value) << 8) | (self.dma_source & 0xF0),
            0xFF52 => self.dma_source = (self.dma_source & 0xFF00) | u16::from(value & 0xF0),
            0xFF53 => self.dma_dest = (u16::from(value & 0x1F) << 8) | (self.dma_dest & 0xF0),
            0xFF54 => self.dma_dest = (self.dma_dest & 0x1F00) | u16::from(value & 0xF0),
            0xFF68 => self.bg_index = value & 0xBF,
            0xFF69 => {
                self.bg_palette[(self.bg_index & 0x3F) as usize] = value;
                if self.bg_index & 0x80 != 0 {
                    self.bg_index = 0x80 | (self.bg_index.wrapping_add(1) & 0x3F);
                }
            }
            0xFF6A => self.obj_index = value & 0xBF,
            0xFF6B => {
                self.obj_palette[(self.obj_index & 0x3F) as usize] = value;
                if self.obj_index & 0x80 != 0 {
                    self.obj_index = 0x80 | (self.obj_index.wrapping_add(1) & 0x3F);
                }
            }
            0xFF70 => self.wram_bank = value & 7,
            0xFF6C => {} // Native CGB mode uses OAM order.
            _ => return false,
        }
        true
    }

    pub fn color(&self, object: bool, palette: u8, color: u8) -> [u8; 4] {
        let colors = if object {
            &self.obj_palette
        } else {
            &self.bg_palette
        };
        let index = usize::from(palette & 7) * 8 + usize::from(color & 3) * 2;
        let rgb = u16::from_le_bytes([colors[index], colors[index + 1]]);
        let expand = |v: u16| ((v << 3) | (v >> 2)) as u8;
        [
            expand(rgb & 31),
            expand((rgb >> 5) & 31),
            expand((rgb >> 10) & 31),
            255,
        ]
    }
}
