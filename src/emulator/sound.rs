//! Four-channel Game Boy PCM renderer, clocked at the PPU/master rate.
//! Register routing and the TRUEOS PCM interface follow the removed GBOI path;
//! oscillators, wave RAM, length, sweep, envelopes and stereo routing live here.

use alloc::vec::Vec;

const CLOCK: u64 = 4_194_304;
const RATE: u64 = 48_000;
const DUTY: [u8; 4] = [0x80, 0x81, 0xE1, 0x7E];

#[derive(Clone, Copy, Default)]
struct Voice {
    enabled: bool,
    timer: i32,
    phase: u8,
    length: u16,
    volume: u8,
    envelope_timer: u8,
}

pub struct Sound {
    regs: [u8; 0x30],
    voices: [Voice; 4],
    powered: bool,
    lfsr: u16,
    sequencer_dots: u32,
    sequencer_step: u8,
    sweep_shadow: u16,
    sweep_timer: u8,
    sweep_enabled: bool,
    sweep_negated: bool,
    sample_accum: u64,
    samples: Vec<i16>,
    capacitor: [i32; 2],
}

impl Sound {
    pub fn new() -> Self {
        let mut regs = [0; 0x30];
        regs[0x01] = 0x80;
        regs[0x02] = 0xF3;
        regs[0x14] = 0x77;
        regs[0x15] = 0xF3;
        Self {
            regs,
            voices: [Voice::default(); 4],
            powered: true,
            lfsr: 0x7FFF,
            sequencer_dots: 0,
            sequencer_step: 0,
            sweep_shadow: 0,
            sweep_timer: 0,
            sweep_enabled: false,
            sweep_negated: false,
            sample_accum: 0,
            samples: Vec::with_capacity(2048),
            capacitor: [0; 2],
        }
    }

    fn reg(&self, addr: u16) -> u8 {
        self.regs[(addr - 0xFF10) as usize]
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        if addr == 0xFF26 {
            let mut bits = 0;
            for (i, voice) in self.voices.iter().enumerate() {
                if voice.enabled {
                    bits |= 1 << i;
                }
            }
            return 0x70 | if self.powered { 0x80 } else { 0 } | bits;
        }
        if (0xFF30..=0xFF3F).contains(&addr) {
            return self.reg(addr);
        }
        const MASKS: [u8; 22] = [
            0x80, 0x3F, 0, 0xFF, 0xBF, 0xFF, 0x3F, 0, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF,
            0xFF, 0xFF, 0, 0, 0xBF, 0, 0,
        ];
        if addr <= 0xFF25 {
            self.reg(addr) | MASKS[(addr - 0xFF10) as usize]
        } else {
            0xFF
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        if addr == 0xFF26 {
            let powered = value & 0x80 != 0;
            if !powered {
                self.regs[..0x17].fill(0);
                self.voices = [Voice::default(); 4];
            }
            if powered != self.powered {
                self.sequencer_dots = 0;
                self.sequencer_step = 0;
            }
            self.powered = powered;
            return;
        }
        if (0xFF30..=0xFF3F).contains(&addr) {
            self.regs[(addr - 0xFF10) as usize] = value;
            return;
        }
        if !self.powered || addr > 0xFF25 {
            return;
        }
        self.regs[(addr - 0xFF10) as usize] = value;
        match addr {
            0xFF11 => self.voices[0].length = 64 - u16::from(value & 63),
            0xFF16 => self.voices[1].length = 64 - u16::from(value & 63),
            0xFF1B => self.voices[2].length = 256 - u16::from(value),
            0xFF20 => self.voices[3].length = 64 - u16::from(value & 63),
            0xFF12 | 0xFF17 | 0xFF21 => {
                let channel = if addr == 0xFF12 {
                    0
                } else if addr == 0xFF17 {
                    1
                } else {
                    3
                };
                if value & 0xF8 == 0 {
                    self.voices[channel].enabled = false;
                }
            }
            0xFF1A if value & 0x80 == 0 => self.voices[2].enabled = false,
            0xFF14 | 0xFF19 | 0xFF1E | 0xFF23 if value & 0x80 != 0 => {
                let channel = match addr {
                    0xFF14 => 0,
                    0xFF19 => 1,
                    0xFF1E => 2,
                    _ => 3,
                };
                self.trigger(channel);
            }
            0xFF10 if self.sweep_negated && value & 8 == 0 => self.voices[0].enabled = false,
            _ => {}
        }
    }

    fn frequency(&self, channel: usize) -> u16 {
        let low = [0xFF13, 0xFF18, 0xFF1D][channel];
        u16::from(self.reg(low)) | (u16::from(self.reg(low + 1) & 7) << 8)
    }

    fn period(&self, channel: usize) -> i32 {
        if channel == 3 {
            let value = self.reg(0xFF22);
            let divisor = [8, 16, 32, 48, 64, 80, 96, 112][(value & 7) as usize];
            divisor << (value >> 4)
        } else {
            i32::from(2048 - self.frequency(channel)) * if channel == 2 { 2 } else { 4 }
        }
    }

    fn envelope(&self, channel: usize) -> u8 {
        self.reg([0xFF12, 0xFF17, 0xFF1A, 0xFF21][channel])
    }

    fn trigger(&mut self, channel: usize) {
        let envelope = self.envelope(channel);
        let period = self.period(channel);
        let voice = &mut self.voices[channel];
        voice.enabled = if channel == 2 {
            envelope & 0x80 != 0
        } else {
            envelope & 0xF8 != 0
        };
        if voice.length == 0 {
            voice.length = if channel == 2 { 256 } else { 64 };
        }
        voice.timer = period;
        voice.phase = 0;
        voice.volume = envelope >> 4;
        voice.envelope_timer = (envelope & 7).max(1);
        if channel == 3 {
            self.lfsr = 0x7FFF;
        }
        if channel == 0 {
            self.sweep_shadow = self.frequency(0);
            self.sweep_timer = ((self.reg(0xFF10) >> 4) & 7).max(1);
            self.sweep_enabled = self.reg(0xFF10) & 0x77 != 0;
            self.sweep_negated = false;
            if self.reg(0xFF10) & 7 != 0 {
                self.sweep_calculate();
            }
        }
    }

    /// Dots are independent of the CGB CPU speed. Silence consumes time too.
    pub fn step(&mut self, dots: u32) {
        for _ in 0..dots {
            if self.powered {
                for i in 0..4 {
                    if !self.voices[i].enabled {
                        continue;
                    }
                    self.voices[i].timer -= 1;
                    if self.voices[i].timer <= 0 {
                        self.voices[i].timer += self.period(i);
                        self.voices[i].phase =
                            self.voices[i].phase.wrapping_add(1) & if i == 2 { 31 } else { 7 };
                        if i == 3 {
                            let bit = (self.lfsr ^ (self.lfsr >> 1)) & 1;
                            self.lfsr = (self.lfsr >> 1) | (bit << 14);
                            if self.reg(0xFF22) & 8 != 0 {
                                self.lfsr = (self.lfsr & !0x40) | (bit << 6);
                            }
                        }
                    }
                }
                self.sequencer_dots += 1;
                if self.sequencer_dots == 8192 {
                    self.sequencer_dots = 0;
                    self.clock_sequencer();
                    self.sequencer_step = (self.sequencer_step + 1) & 7;
                }
            }
            self.sample_accum += RATE;
            if self.sample_accum >= CLOCK {
                self.sample_accum -= CLOCK;
                let stereo = self.render_sample();
                // Bound the buffer when a caller runs without an audio sink.
                if self.samples.len() < 8192 {
                    self.samples.extend_from_slice(&stereo);
                }
            }
        }
    }

    fn clock_sequencer(&mut self) {
        if self.sequencer_step & 1 == 0 {
            for i in 0..4 {
                if self.reg([0xFF14, 0xFF19, 0xFF1E, 0xFF23][i]) & 0x40 != 0
                    && self.voices[i].length != 0
                {
                    self.voices[i].length -= 1;
                    if self.voices[i].length == 0 {
                        self.voices[i].enabled = false;
                    }
                }
            }
        }
        if self.sequencer_step == 2 || self.sequencer_step == 6 {
            self.sweep_timer = self.sweep_timer.saturating_sub(1);
            if self.sweep_timer == 0 {
                let period = (self.reg(0xFF10) >> 4) & 7;
                self.sweep_timer = if period == 0 { 8 } else { period };
                if self.sweep_enabled && period != 0 && self.reg(0xFF10) & 7 != 0 {
                    if let Some(frequency) = self.sweep_calculate() {
                        self.sweep_shadow = frequency;
                        self.regs[3] = frequency as u8;
                        self.regs[4] = (self.regs[4] & !7) | (frequency >> 8) as u8;
                        self.sweep_calculate();
                    }
                }
            }
        }
        if self.sequencer_step == 7 {
            for i in [0, 1, 3] {
                let envelope = self.envelope(i);
                if envelope & 7 == 0 {
                    continue;
                }
                self.voices[i].envelope_timer = self.voices[i].envelope_timer.saturating_sub(1);
                if self.voices[i].envelope_timer == 0 {
                    self.voices[i].envelope_timer = envelope & 7;
                    let volume = self.voices[i].volume;
                    self.voices[i].volume = if envelope & 8 != 0 {
                        (volume + 1).min(15)
                    } else {
                        volume.saturating_sub(1)
                    };
                }
            }
        }
    }

    fn sweep_calculate(&mut self) -> Option<u16> {
        let sweep = self.reg(0xFF10);
        let delta = self.sweep_shadow >> (sweep & 7);
        let frequency = if sweep & 8 != 0 {
            self.sweep_negated = true;
            self.sweep_shadow - delta
        } else {
            self.sweep_shadow + delta
        };
        if frequency > 2047 {
            self.voices[0].enabled = false;
            None
        } else {
            Some(frequency)
        }
    }

    fn render_sample(&mut self) -> [i16; 2] {
        let mut mix = [0i32; 2];
        if self.powered {
            let routing = self.reg(0xFF25);
            for i in 0..4 {
                if !self.voices[i].enabled {
                    continue;
                }
                let voice = self.voices[i];
                let level = match i {
                    0 | 1 => {
                        let duty = self.reg(if i == 0 { 0xFF11 } else { 0xFF16 }) >> 6;
                        if DUTY[duty as usize] & (1 << voice.phase) != 0 {
                            voice.volume
                        } else {
                            0
                        }
                    }
                    2 => {
                        let wave = self.reg(0xFF30 + u16::from(voice.phase / 2));
                        let sample = if voice.phase & 1 == 0 {
                            wave >> 4
                        } else {
                            wave & 15
                        };
                        match (self.reg(0xFF1C) >> 5) & 3 {
                            0 => 0,
                            shift => sample >> (shift - 1),
                        }
                    }
                    _ => {
                        if self.lfsr & 1 == 0 {
                            voice.volume
                        } else {
                            0
                        }
                    }
                };
                let amplitude = i32::from(level) * 2 - 15;
                if routing & (1 << (i + 4)) != 0 {
                    mix[0] += amplitude;
                }
                if routing & (1 << i) != 0 {
                    mix[1] += amplitude;
                }
            }
            let volume = self.reg(0xFF24);
            mix[0] *= i32::from(((volume >> 4) & 7) + 1) * 64;
            mix[1] *= i32::from((volume & 7) + 1) * 64;
        }
        let mut out = [0; 2];
        for i in 0..2 {
            let high_pass = (mix[i] << 8) - self.capacitor[i];
            self.capacitor[i] += high_pass / 256;
            out[i] = (high_pass >> 8).clamp(-32768, 32767) as i16;
        }
        out
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    pub fn consume_frames(&mut self, frames: usize) {
        let count = frames.saturating_mul(2).min(self.samples.len());
        self.samples.drain(..count);
    }
}
