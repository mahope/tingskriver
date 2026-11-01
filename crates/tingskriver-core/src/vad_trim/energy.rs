/// High-pass energy rejects mains hum; a slowly adapting noise floor prevents
/// low stationary noise from becoming speech. State persists across input chunks.
pub(super) struct EnergyVad {
    previous: f32,
    high_pass: f32,
    floor: f32,
    recent: [f32; 4],
    frames: usize,
}
impl EnergyVad {
    pub fn new() -> Self {
        Self {
            previous: 0.0,
            high_pass: 0.0,
            floor: 0.0003,
            recent: [0.0; 4],
            frames: 0,
        }
    }
    pub fn speech(&mut self, frame: &[f32], threshold: f32) -> bool {
        let mut energy = 0.0;
        let mut raw_energy = 0.0;
        let mut difference_energy = 0.0;
        for &sample in frame {
            raw_energy += sample * sample;
            difference_energy += (sample - self.previous).powi(2);
            // One-pole high-pass, approximately 200 Hz at 16 kHz.
            self.high_pass = 0.925 * (self.high_pass + sample - self.previous);
            self.previous = sample;
            energy += self.high_pass * self.high_pass;
        }
        let rms = (energy / frame.len() as f32).sqrt();
        let broadband = difference_energy > raw_energy * 0.02;
        self.recent[self.frames % 4] = rms;
        self.frames = self.frames.saturating_add(1);
        // Estimate stationary background independently of the speech decision.
        // Four similar frames qualify before the default 150 ms speech minimum.
        // Exclude louder input and windows with speech-like energy variation.
        if self.frames >= 4 {
            let low = self.recent.iter().copied().fold(f32::INFINITY, f32::min);
            let high = self.recent.iter().copied().fold(0.0, f32::max);
            if broadband && high < low * 1.15 && high < 0.006 {
                self.floor = self.floor.max(low);
            }
        }
        let gate = (0.0015 + threshold * 0.003).max(self.floor * (1.5 + threshold));
        let speech = rms > gate && broadband;
        if !speech && broadband {
            self.floor = 0.98 * self.floor + 0.02 * rms;
        }
        speech
    }
}
