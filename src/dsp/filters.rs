//! Basic filter building blocks: RBJ-cookbook biquads and one-pole filters.

use std::f32::consts::PI;

/// Biquad filter coefficients (normalised so that a0 == 1).
#[derive(Clone, Copy, Debug)]
pub struct BiquadCoefs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Default for BiquadCoefs {
    fn default() -> Self {
        Self::identity()
    }
}

impl BiquadCoefs {
    pub const fn identity() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }
    }

    fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv = 1.0 / a0;
        Self { b0: b0 * inv, b1: b1 * inv, b2: b2 * inv, a1: a1 * inv, a2: a2 * inv }
    }

    fn omega(sr: f32, freq: f32) -> (f32, f32) {
        // Keep the frequency safely below Nyquist.
        let f = freq.clamp(5.0, sr * 0.49);
        let w = 2.0 * PI * f / sr;
        (w.sin(), w.cos())
    }

    pub fn lowpass(sr: f32, freq: f32, q: f32) -> Self {
        let (sin, cos) = Self::omega(sr, freq);
        let alpha = sin / (2.0 * q);
        let b1 = 1.0 - cos;
        Self::normalize(b1 * 0.5, b1, b1 * 0.5, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn highpass(sr: f32, freq: f32, q: f32) -> Self {
        let (sin, cos) = Self::omega(sr, freq);
        let alpha = sin / (2.0 * q);
        let b1 = -(1.0 + cos);
        Self::normalize(-b1 * 0.5, b1, -b1 * 0.5, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn peaking(sr: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let (sin, cos) = Self::omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sin / (2.0 * q);
        Self::normalize(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        )
    }

    pub fn low_shelf(sr: f32, freq: f32, gain_db: f32) -> Self {
        let (sin, cos) = Self::omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sin / 2.0 * std::f32::consts::SQRT_2; // shelf slope S = 1
        let sq = 2.0 * a.sqrt() * alpha;
        Self::normalize(
            a * ((a + 1.0) - (a - 1.0) * cos + sq),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sq),
            (a + 1.0) + (a - 1.0) * cos + sq,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sq,
        )
    }

    pub fn high_shelf(sr: f32, freq: f32, gain_db: f32) -> Self {
        let (sin, cos) = Self::omega(sr, freq);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sin / 2.0 * std::f32::consts::SQRT_2;
        let sq = 2.0 * a.sqrt() * alpha;
        Self::normalize(
            a * ((a + 1.0) + (a - 1.0) * cos + sq),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sq),
            (a + 1.0) - (a - 1.0) * cos + sq,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sq,
        )
    }
}

/// Transposed direct form II biquad.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub coefs: BiquadCoefs,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new(coefs: BiquadCoefs) -> Self {
        Self { coefs, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let c = &self.coefs;
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Simple one-pole lowpass, used for damping and smoothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePoleLp {
    a: f32,
    z: f32,
}

impl OnePoleLp {
    pub fn set_cutoff(&mut self, sr: f32, freq: f32) {
        let f = freq.clamp(1.0, sr * 0.49);
        self.a = (-2.0 * PI * f / sr).exp();
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z = x + self.a * (self.z - x);
        self.z
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

/// Flush denormals / tiny values to zero to avoid CPU spikes in feedback loops.
#[inline]
pub fn undenormal(x: f32) -> f32 {
    if x.abs() < 1.0e-20 {
        0.0
    } else {
        x
    }
}
