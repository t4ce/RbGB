use rbgb::Emulator;
use trueos::{
    audio::{self, PlaybackParams, Stream},
    vshell,
};

/// A bounded, nonblocking PCM stream. Short writes remain in the core buffer.
pub struct AudioOutput {
    stream: Option<Stream>,
    attempted: bool,
    started: bool,
}

impl AudioOutput {
    pub fn new() -> Self {
        Self {
            stream: None,
            attempted: false,
            started: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn pump(&mut self, emulator: &mut Emulator) {
        if emulator.audio_samples().is_empty() {
            return;
        }
        if !self.attempted {
            // Accumulate two native frames before opening the live stream.
            if emulator.audio_samples().len() < 3200 {
                return;
            }
            self.attempted = true;
            match Stream::open_playback(PlaybackParams::s16le_stereo_48k()) {
                Ok(stream) => self.stream = Some(stream),
                Err(code) => {
                    vshell::linef(format_args!("rbgb: audio unavailable (code {code})"));
                }
            }
        }
        let Some(stream) = self.stream else {
            let frames = emulator.audio_samples().len() / 2;
            emulator.consume_audio_frames(frames);
            return;
        };
        let result = (|| {
            // Keep input-to-sound latency below about 50 ms.
            let queued = stream.queued_frames()?;
            let frames = (2400usize.saturating_sub(queued)).min(emulator.audio_samples().len() / 2);
            if frames != 0 {
                let written =
                    stream.write_interleaved_i16(&emulator.audio_samples()[..frames * 2])?;
                emulator.consume_audio_frames(written);
            }
            if !self.started && frames != 0 {
                stream.start()?;
                self.started = true;
            }
            Ok::<(), i32>(())
        })();
        if let Err(code) = result {
            if code != audio::ERR_BUSY {
                vshell::linef(format_args!("rbgb: audio stream stopped (code {code})"));
                let _ = stream.close();
                self.stream = None;
            }
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.close();
        }
    }
}
