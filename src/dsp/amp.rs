//! A two-stage "British-American" style amp with a cabinet simulation.
//!
//! Signal flow:
//!   input HPF -> preamp stage 1 (tanh) -> coupling HPF -> bright LPF ->
//!   preamp stage 2 (tanh) -> coupling HPF -> tone stack (bass/mid/treble) ->
//!   power amp saturation -> presence shelf -> cabinet -> master volume

use super::filters::{Biquad, BiquadCoefs};
use super::shaper::AdaaTanh;
use crate::params::AmpParams;
use nih_plug::util;

const BIAS1: f32 = 0.15;
const BIAS2: f32 = -0.1;

#[derive(Default)]
struct ToneValues {
    bass: f32,
    mid: f32,
    treble: f32,
    presence: f32,
}

pub struct Amp {
    sr: f32,
    input_hpf: Biquad,
    stage1: AdaaTanh,
    couple1: Biquad,
    interstage_lpf: Biquad,
    stage2: AdaaTanh,
    couple2: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    power: AdaaTanh,
    presence: Biquad,
    cab: [Biquad; 6],
    last: ToneValues,
    cab_was_on: bool,
}

impl Amp {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            input_hpf: Biquad::default(),
            stage1: AdaaTanh::default(),
            couple1: Biquad::default(),
            interstage_lpf: Biquad::default(),
            stage2: AdaaTanh::default(),
            couple2: Biquad::default(),
            bass: Biquad::default(),
            mid: Biquad::default(),
            treble: Biquad::default(),
            power: AdaaTanh::default(),
            presence: Biquad::default(),
            cab: [Biquad::default(); 6],
            last: ToneValues { bass: -1.0, ..Default::default() },
            cab_was_on: true,
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.input_hpf.coefs = BiquadCoefs::highpass(sr, 80.0, 0.707);
        self.couple1.coefs = BiquadCoefs::highpass(sr, 25.0, 0.707);
        self.interstage_lpf.coefs = BiquadCoefs::lowpass(sr, 9000.0, 0.707);
        self.couple2.coefs = BiquadCoefs::highpass(sr, 25.0, 0.707);

        // A rough 4x12 closed-back cabinet: low resonance, scooped low mids,
        // presence peak, and a steep roll-off above ~5 kHz.
        self.cab = [
            Biquad::new(BiquadCoefs::highpass(sr, 75.0, 0.8)),
            Biquad::new(BiquadCoefs::peaking(sr, 110.0, 1.2, 3.5)),
            Biquad::new(BiquadCoefs::peaking(sr, 450.0, 1.0, -3.0)),
            Biquad::new(BiquadCoefs::peaking(sr, 2400.0, 1.4, 3.0)),
            Biquad::new(BiquadCoefs::lowpass(sr, 4800.0, 0.8)),
            Biquad::new(BiquadCoefs::lowpass(sr, 6500.0, 0.6)),
        ];
        self.last.bass = -1.0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.input_hpf.reset();
        self.stage1.reset();
        self.couple1.reset();
        self.interstage_lpf.reset();
        self.stage2.reset();
        self.couple2.reset();
        self.bass.reset();
        self.mid.reset();
        self.treble.reset();
        self.power.reset();
        self.presence.reset();
        self.cab.iter_mut().for_each(Biquad::reset);
    }

    fn update_tone(&mut self, p: &AmpParams, n: u32) {
        let v = ToneValues {
            bass: p.bass.smoothed.next_step(n),
            mid: p.mid.smoothed.next_step(n),
            treble: p.treble.smoothed.next_step(n),
            presence: p.presence.smoothed.next_step(n),
        };
        let l = &self.last;
        let changed = (v.bass - l.bass).abs() > 1.0e-3
            || (v.mid - l.mid).abs() > 1.0e-3
            || (v.treble - l.treble).abs() > 1.0e-3
            || (v.presence - l.presence).abs() > 1.0e-3;
        if changed {
            let sr = self.sr;
            // Each 0..10 dial maps to a symmetric boost/cut around 5.
            self.bass.coefs = BiquadCoefs::low_shelf(sr, 140.0, (v.bass - 5.0) * 2.4);
            self.mid.coefs = BiquadCoefs::peaking(sr, 700.0, 0.7, (v.mid - 5.0) * 2.0);
            self.treble.coefs = BiquadCoefs::high_shelf(sr, 2800.0, (v.treble - 5.0) * 2.4);
            self.presence.coefs = BiquadCoefs::high_shelf(sr, 5000.0, (v.presence - 5.0) * 1.6);
            self.last = v;
        }
    }

    pub fn process(&mut self, p: &AmpParams, buf: &mut [f32]) {
        self.update_tone(p, buf.len() as u32);

        let cab_on = p.cabinet.value();
        if cab_on && !self.cab_was_on {
            self.cab.iter_mut().for_each(Biquad::reset);
        }
        self.cab_was_on = cab_on;

        let bias1 = BIAS1.tanh();
        let bias2 = BIAS2.tanh();

        for s in buf.iter_mut() {
            let g = p.gain.smoothed.next() / 10.0;
            let master = p.master.smoothed.next();
            let pre1 = util::db_to_gain(4.0 + g * 30.0);
            let pre2 = util::db_to_gain(g * 14.0);

            let mut x = self.input_hpf.process(*s);
            x = self.stage1.process(x * pre1 + BIAS1) - bias1;
            x = self.couple1.process(x);
            x = self.interstage_lpf.process(x);
            x = self.stage2.process(x * pre2 * 1.5 + BIAS2) - bias2;
            x = self.couple2.process(x);

            x = self.bass.process(x);
            x = self.mid.process(x);
            x = self.treble.process(x);

            // Power amp: gentle extra compression.
            x = self.power.process(x * 0.9) * 1.1;
            x = self.presence.process(x);

            if cab_on {
                for f in self.cab.iter_mut() {
                    x = f.process(x);
                }
                // Compensate for level lost in the cabinet filtering.
                x *= 1.4;
            }

            *s = x * master;
        }
    }
}

impl Default for Amp {
    fn default() -> Self {
        Self::new()
    }
}
