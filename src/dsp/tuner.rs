//! Chromatic tuner using the YIN pitch detection algorithm.
//!
//! The input is band-limited and decimated to ~12 kHz, then analysed ~20 times
//! a second. The detected frequency is published lock-free to the GUI.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::filters::{Biquad, BiquadCoefs};

const TARGET_RATE: f32 = 12_000.0;
const RING_LEN: usize = 2048;
const WINDOW: usize = 1024;
const MIN_FREQ: f32 = 60.0;
const MAX_FREQ: f32 = 1200.0;
const HOP: usize = 512;
const YIN_THRESHOLD: f32 = 0.15;
const GATE_RMS: f32 = 0.003; // ~ -50 dBFS
const MAX_TAU: usize = RING_LEN - WINDOW - 1;

/// Shared tuner result. A frequency of 0 means "no pitch detected".
#[derive(Default)]
pub struct TunerReadout {
    freq_bits: AtomicU32,
}

impl TunerReadout {
    pub fn frequency(&self) -> f32 {
        f32::from_bits(self.freq_bits.load(Ordering::Relaxed))
    }

    pub fn set_frequency(&self, f: f32) {
        self.freq_bits.store(f.to_bits(), Ordering::Relaxed);
    }
}

pub struct Tuner {
    readout: Arc<TunerReadout>,
    decimation: usize,
    decim_counter: usize,
    rate: f32,
    aa: [Biquad; 3],
    ring: Box<[f32; RING_LEN]>,
    ring_pos: usize,
    since_analysis: usize,
    // Scratch buffers, preallocated so analysis never allocates.
    frame: Box<[f32; RING_LEN]>,
    diff: Box<[f32; RING_LEN]>,
}

impl Tuner {
    pub fn new(readout: Arc<TunerReadout>) -> Self {
        Self {
            readout,
            decimation: 4,
            decim_counter: 0,
            rate: TARGET_RATE,
            aa: [Biquad::default(); 3],
            ring: Box::new([0.0; RING_LEN]),
            ring_pos: 0,
            since_analysis: 0,
            frame: Box::new([0.0; RING_LEN]),
            diff: Box::new([0.0; RING_LEN]),
        }
    }

    pub fn prepare(&mut self, sr: f32) {
        self.decimation = ((sr / TARGET_RATE).round() as usize).max(1);
        self.rate = sr / self.decimation as f32;
        let cutoff = (self.rate * 0.4).min(4000.0);
        self.aa = [
            Biquad::new(BiquadCoefs::highpass(sr, 40.0, 0.707)),
            Biquad::new(BiquadCoefs::lowpass(sr, cutoff, 0.54)),
            Biquad::new(BiquadCoefs::lowpass(sr, cutoff, 1.31)),
        ];
        self.reset();
    }

    pub fn reset(&mut self) {
        self.aa.iter_mut().for_each(Biquad::reset);
        self.ring.fill(0.0);
        self.ring_pos = 0;
        self.decim_counter = 0;
        self.since_analysis = 0;
        self.readout.set_frequency(0.0);
    }

    /// Analyse the signal (without modifying it).
    pub fn analyse(&mut self, buf: &[f32]) {
        for &s in buf {
            let mut x = s;
            for f in self.aa.iter_mut() {
                x = f.process(x);
            }
            self.decim_counter += 1;
            if self.decim_counter < self.decimation {
                continue;
            }
            self.decim_counter = 0;
            self.ring[self.ring_pos] = x;
            self.ring_pos = (self.ring_pos + 1) % RING_LEN;
            self.since_analysis += 1;
            if self.since_analysis >= HOP {
                self.since_analysis = 0;
                let f = self.detect();
                self.readout.set_frequency(f.unwrap_or(0.0));
            }
        }
    }

    fn detect(&mut self) -> Option<f32> {
        // Unroll the ring buffer, oldest sample first.
        for i in 0..RING_LEN {
            self.frame[i] = self.ring[(self.ring_pos + i) % RING_LEN];
        }
        detect_pitch(&self.frame[..], &mut self.diff[..], self.rate)
    }
}

/// YIN pitch estimate of `frame` (length >= WINDOW + MAX_TAU + 1) at `rate` Hz.
pub fn detect_pitch(frame: &[f32], diff: &mut [f32], rate: f32) -> Option<f32> {
    // Use the most recent part of the frame for the analysis window.
    let start = frame.len() - WINDOW - MAX_TAU - 1;
    let x = &frame[start..];

    let rms = (x[..WINDOW].iter().map(|v| v * v).sum::<f32>() / WINDOW as f32).sqrt();
    if rms < GATE_RMS {
        return None;
    }

    let min_tau = ((rate / MAX_FREQ) as usize).max(2);
    let max_tau = ((rate / MIN_FREQ) as usize).min(MAX_TAU);

    // Difference function + cumulative mean normalisation.
    diff[0] = 1.0;
    let mut running = 0.0;
    for tau in 1..=max_tau {
        let mut d = 0.0;
        for j in 0..WINDOW {
            let delta = x[j] - x[j + tau];
            d += delta * delta;
        }
        running += d;
        diff[tau] = if running > 0.0 { d * tau as f32 / running } else { 1.0 };
    }

    // First dip below the threshold, then walk down to its local minimum.
    let mut tau = min_tau;
    let mut found = None;
    while tau < max_tau {
        if diff[tau] < YIN_THRESHOLD {
            while tau + 1 < max_tau && diff[tau + 1] < diff[tau] {
                tau += 1;
            }
            found = Some(tau);
            break;
        }
        tau += 1;
    }
    let tau = found?;

    // Parabolic interpolation for sub-sample accuracy.
    let refined = if tau > 1 && tau < max_tau {
        let (a, b, c) = (diff[tau - 1], diff[tau], diff[tau + 1]);
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1.0e-9 {
            tau as f32 + 0.5 * (a - c) / denom
        } else {
            tau as f32
        }
    } else {
        tau as f32
    };

    Some(rate / refined)
}

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Convert a frequency to (note name, octave, cents offset).
pub fn note_for_frequency(freq: f32, reference: f32) -> (&'static str, i32, f32) {
    let midi = 69.0 + 12.0 * (freq / reference).log2();
    let nearest = midi.round();
    let cents = (midi - nearest) * 100.0;
    let n = nearest as i32;
    (NOTE_NAMES[n.rem_euclid(12) as usize], n.div_euclid(12) - 1, cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_tone(freq: f32, sr: f32) -> f32 {
        let readout = Arc::new(TunerReadout::default());
        let mut tuner = Tuner::new(readout.clone());
        tuner.prepare(sr);
        // Guitar-like tone: fundamental plus strong harmonics.
        let sig: Vec<f32> = (0..(sr as usize))
            .map(|i| {
                let t = i as f32 / sr;
                let w = 2.0 * std::f32::consts::PI * freq * t;
                0.3 * w.sin() + 0.25 * (2.0 * w).sin() + 0.15 * (3.0 * w).sin()
            })
            .collect();
        tuner.analyse(&sig);
        readout.frequency()
    }

    #[test]
    fn detects_open_strings() {
        for &(f, sr) in &[(82.41, 48_000.0), (110.0, 44_100.0), (196.0, 96_000.0), (329.63, 48_000.0)] {
            let detected = detect_tone(f, sr);
            let cents = 1200.0 * (detected / f).log2();
            assert!(cents.abs() < 3.0, "f={f} sr={sr} detected={detected} ({cents} cents)");
        }
    }

    #[test]
    fn silence_gives_no_pitch() {
        let readout = Arc::new(TunerReadout::default());
        let mut tuner = Tuner::new(readout.clone());
        tuner.prepare(48_000.0);
        tuner.analyse(&vec![0.0; 48_000]);
        assert_eq!(readout.frequency(), 0.0);
    }

    #[test]
    fn note_names() {
        let (n, o, c) = note_for_frequency(440.0, 440.0);
        assert_eq!((n, o), ("A", 4));
        assert!(c.abs() < 0.01);
        let (n, o, _) = note_for_frequency(82.41, 440.0);
        assert_eq!((n, o), ("E", 2));
    }
}
