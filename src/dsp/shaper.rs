//! Waveshapers.

/// `tanh` waveshaper with first-order antiderivative anti-aliasing (ADAA).
///
/// ADAA greatly reduces the aliasing a plain `tanh` produces at high drive,
/// at the cost of half a sample of latency, without needing oversampling.
#[derive(Clone, Copy, Debug, Default)]
pub struct AdaaTanh {
    x1: f32,
    f1: f32,
}

/// Antiderivative of tanh: ln(cosh(x)), computed in a numerically stable way.
#[inline]
fn log_cosh(x: f32) -> f32 {
    let a = x.abs();
    a + (-2.0 * a).exp().ln_1p() - std::f32::consts::LN_2
}

impl AdaaTanh {
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let f = log_cosh(x);
        let dx = x - self.x1;
        let y = if dx.abs() < 1.0e-4 {
            // Ill-conditioned: fall back to evaluating at the midpoint.
            (0.5 * (x + self.x1)).tanh()
        } else {
            (f - self.f1) / dx
        };
        self.x1 = x;
        self.f1 = f;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.f1 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximates_tanh() {
        let mut s = AdaaTanh::default();
        // For a slowly varying input ADAA should track tanh closely.
        let mut x = -3.0f32;
        let mut prev = x;
        s.process(x);
        while x < 3.0 {
            x += 0.01;
            let y = s.process(x);
            let expected = (0.5 * (x + prev)).tanh();
            assert!((y - expected).abs() < 1.0e-3, "x={x} y={y} expected={expected}");
            prev = x;
        }
    }
}
