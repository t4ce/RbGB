pub const FRAME_NS: u64 = (70224 * 1_000_000_000 + 2_097_152) / 4_194_304;

/// Wall-clock admission for native LCD frames, independent of GPU admission.
pub struct Pacer {
    next: u64,
}

impl Pacer {
    pub fn new(now: u64) -> Self {
        Self { next: now }
    }

    pub fn reset(&mut self, now: u64) {
        self.next = now;
    }

    pub fn due(&mut self, now: u64) -> bool {
        // Treat a long VM suspension as a pause, rather than fast-forwarding
        // minutes of old gameplay/audio when that VM resumes.
        if now.saturating_sub(self.next) > 250_000_000 {
            self.next = now;
        }
        now >= self.next
    }

    pub fn advance(&mut self) {
        self.next = self.next.saturating_add(FRAME_NS);
    }

    pub fn wait_ns(&self, now: u64) -> u64 {
        self.next.saturating_sub(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_gpu_stalls_preserve_owed_frames() {
        let mut pacer = Pacer::new(0);
        assert!(pacer.due(0));
        pacer.advance();
        let now = FRAME_NS * 4 + 100;
        let mut frames = 0;
        while pacer.due(now) {
            pacer.advance();
            frames += 1;
        }
        assert_eq!(frames, 4);
        assert_eq!(pacer.wait_ns(now), FRAME_NS - 100);
    }

    #[test]
    fn a_long_vm_pause_reanchors_without_a_catchup_burst() {
        let mut pacer = Pacer::new(0);
        assert!(pacer.due(10_000_000_000));
        pacer.advance();
        assert!(!pacer.due(10_000_000_000));
    }
}
