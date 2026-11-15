//! Overdrive / distortion pedal.
//!
//! Signal flow: tighten HPF -> mid "hump" pre-emphasis -> drive gain ->
//! asymmetric ADAA tanh clipper -> DC blocker -> fizz LPF -> tone LPF -> level.

use super::filters::{Biquad, BiquadCoefs};
use super::shaper::AdaaTanh;
use crate::params::DistortionParams;
use nih_plug::util;

const BIAS: f32 = 0.2;

pub struct Distortion {
    sr: f32,
    tighten: Biquad,
    hump: Biquad,
    clipper: AdaaTanh,
    dc_block: Biquad,
    fizz: Biquad,
    tone: Biquad,
    last_tone: f32,
    bias_offset: f32,
}

impl Distortion {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            tighten: Biquad::default(),
            hump: Biquad::default(),
            clipper: AdaaTanh::default(),
            dc_block: Biquad::default(),
            fizz: Biquad::default(),
            tone: Biquad::default(),
            last_tone: -1.0,
            bias_offset: BIAS.tanh(),
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.tighten.coefs = BiquadCoefs::highpass(sr, 140.0, 0.707);
        self.hump.coefs = BiquadCoefs::peaking(sr, 800.0, 0.8, 7.0);
        self.dc_block.coefs = BiquadCoefs::highpass(sr, 20.0, 0.707);
        self.fizz.coefs = BiquadCoefs::lowpass(sr, 7500.0, 0.707);
        self.last_tone = -1.0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.tighten.reset();
        self.hump.reset();
        self.clipper.reset();
        self.dc_block.reset();
        self.fizz.reset();
        self.tone.reset();
    }

    pub fn process(&mut self, p: &DistortionParams, buf: &mut [f32]) {
        let n = buf.len() as u32;
        let tone = p.tone.smoothed.next_step(n);
        if (tone - self.last_tone).abs() > 1.0e-4 {
            // Exponential sweep 700 Hz .. 9 kHz.
            let freq = 700.0 * (9000.0f32 / 700.0).powf(tone);
            self.tone.coefs = BiquadCoefs::lowpass(self.sr, freq, 0.6);
            self.last_tone = tone;
        }

        for s in buf.iter_mut() {
            let drive = p.drive.smoothed.next();
            let level = p.level.smoothed.next();
            // 0 .. 48 dB of drive with a perceptually even taper.
            let gain = util::db_to_gain(6.0 + drive * 42.0);

            let mut x = self.tighten.process(*s);
            x = self.hump.process(x);
            x = self.clipper.process(x * gain + BIAS) - self.bias_offset;
            x = self.dc_block.process(x);
            x = self.fizz.process(x);
            x = self.tone.process(x);
            *s = x * level;
        }
    }
}

impl Default for Distortion {
    fn default() -> Self {
        Self::new()
    }
}
