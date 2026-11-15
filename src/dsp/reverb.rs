//! Freeverb-style algorithmic reverb (8 damped combs + 4 allpasses) with
//! pre-delay and wet-signal band limiting tuned for guitar.

use super::filters::{undenormal, Biquad, BiquadCoefs};
use crate::params::ReverbParams;

// Classic Freeverb tunings at 44.1 kHz.
const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
const MAX_PREDELAY_SECONDS: f32 = 0.16;
const INPUT_GAIN: f32 = 0.015;
const WET_SCALE: f32 = 3.0;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0 }
    }

    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = undenormal(out * (1.0 - damp) + self.store * damp);
        self.buf[self.idx] = x + self.store * feedback;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.store = 0.0;
        self.idx = 0;
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0 }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let bufout = self.buf[self.idx];
        self.buf[self.idx] = undenormal(x + bufout * 0.5);
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        bufout - x
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.idx = 0;
    }
}

pub struct Reverb {
    sr: f32,
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
    predelay: Vec<f32>,
    pre_idx: usize,
    wet_hpf: Biquad,
    wet_lpf: Biquad,
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            combs: Vec::new(),
            allpasses: Vec::new(),
            predelay: Vec::new(),
            pre_idx: 0,
            wet_hpf: Biquad::default(),
            wet_lpf: Biquad::default(),
        }
    }

    /// Allocates all delay lines. Must not be called from the audio thread.
    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        let scale = sr / 44_100.0;
        self.combs = COMB_TUNINGS
            .iter()
            .map(|&t| Comb::new((t as f32 * scale) as usize))
            .collect();
        self.allpasses = ALLPASS_TUNINGS
            .iter()
            .map(|&t| Allpass::new((t as f32 * scale) as usize))
            .collect();
        self.predelay = vec![0.0; (sr * MAX_PREDELAY_SECONDS) as usize + 2];
        self.wet_hpf.coefs = BiquadCoefs::highpass(sr, 180.0, 0.707);
        self.wet_lpf.coefs = BiquadCoefs::lowpass(sr, 7000.0, 0.707);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.combs.iter_mut().for_each(Comb::reset);
        self.allpasses.iter_mut().for_each(Allpass::reset);
        self.predelay.iter_mut().for_each(|s| *s = 0.0);
        self.pre_idx = 0;
        self.wet_hpf.reset();
        self.wet_lpf.reset();
    }

    pub fn process(&mut self, p: &ReverbParams, buf: &mut [f32]) {
        if self.predelay.is_empty() {
            return;
        }
        let n = buf.len() as u32;
        let size = p.size.smoothed.next_step(n);
        let damping = p.damping.smoothed.next_step(n);
        let feedback = 0.7 + size * 0.28;
        let damp = damping * 0.4;

        let pre_len = self.predelay.len();
        let pre_samples =
            ((p.predelay_ms.value() * 0.001 * self.sr) as usize).min(pre_len - 1);

        for s in buf.iter_mut() {
            let mix = p.mix.smoothed.next();
            let dry = *s;

            // Pre-delay line.
            self.predelay[self.pre_idx] = dry;
            let read = (self.pre_idx + pre_len - pre_samples) % pre_len;
            let delayed = self.predelay[read];
            self.pre_idx = (self.pre_idx + 1) % pre_len;

            let input = self.wet_hpf.process(delayed) * INPUT_GAIN;
            let mut wet = 0.0;
            for comb in self.combs.iter_mut() {
                wet += comb.process(input, feedback, damp);
            }
            for ap in self.allpasses.iter_mut() {
                wet = ap.process(wet);
            }
            let wet = self.wet_lpf.process(wet) * WET_SCALE;

            *s = dry * (1.0 - mix * 0.5) + wet * mix;
        }
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}
