//! Analog-flavoured digital delay: interpolated read, filtered & softly
//! saturated feedback path, dry/wet mix.

use super::filters::{undenormal, Biquad, BiquadCoefs, OnePoleLp};
use crate::params::DelayParams;

const MAX_DELAY_SECONDS: f32 = 2.1;

pub struct Delay {
    sr: f32,
    buffer: Vec<f32>,
    write: usize,
    tone_lp: OnePoleLp,
    low_cut: Biquad,
    last_tone: f32,
}

impl Delay {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            buffer: Vec::new(),
            write: 0,
            tone_lp: OnePoleLp::default(),
            low_cut: Biquad::default(),
            last_tone: -1.0,
        }
    }

    /// Allocates the delay line. Must not be called from the audio thread.
    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let len = (sr * MAX_DELAY_SECONDS) as usize + 4;
        self.buffer = vec![0.0; len];
        self.low_cut.coefs = BiquadCoefs::highpass(sr, 90.0, 0.707);
        self.last_tone = -1.0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.buffer.iter_mut().for_each(|s| *s = 0.0);
        self.write = 0;
        self.tone_lp.reset();
        self.low_cut.reset();
    }

    #[inline]
    fn read(&self, delay_samples: f32) -> f32 {
        let len = self.buffer.len();
        let pos = self.write as f32 - delay_samples;
        let pos = if pos < 0.0 { pos + len as f32 } else { pos };
        let i0 = pos as usize % len;
        let i1 = (i0 + 1) % len;
        let frac = pos - pos.floor();
        self.buffer[i0] + (self.buffer[i1] - self.buffer[i0]) * frac
    }

    pub fn process(&mut self, p: &DelayParams, buf: &mut [f32]) {
        if self.buffer.is_empty() {
            return;
        }
        let tone = p.tone.smoothed.next_step(buf.len() as u32);
        if (tone - self.last_tone).abs() > 1.0e-4 {
            let freq = 1200.0 * (12_000.0f32 / 1200.0).powf(tone);
            self.tone_lp.set_cutoff(self.sr, freq);
            self.last_tone = tone;
        }
        let max_delay = (self.buffer.len() - 3) as f32;

        for s in buf.iter_mut() {
            let time_ms = p.time_ms.smoothed.next();
            let feedback = p.feedback.smoothed.next();
            let mix = p.mix.smoothed.next();

            let delay_samples = (time_ms * 0.001 * self.sr).clamp(1.0, max_delay);
            let wet = self.read(delay_samples);

            // Feedback path: darken, remove mud, and saturate softly so high
            // feedback settings stay musical instead of exploding.
            let fb = self.low_cut.process(self.tone_lp.process(wet)) * feedback;
            let fb = fb / (1.0 + fb.abs() * 0.3);

            let dry = *s;
            self.buffer[self.write] = undenormal(dry + fb);
            self.write = (self.write + 1) % self.buffer.len();

            *s = dry * (1.0 - mix * 0.5) + wet * mix;
        }
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}
