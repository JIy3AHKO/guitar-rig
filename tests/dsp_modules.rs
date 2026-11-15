//! Smoke tests running every module with extreme settings.

use guitar_rig::dsp::{amp::Amp, delay::Delay, distortion::Distortion, reverb::Reverb};
use guitar_rig::params::GuitarRigParams;
use nih_plug::prelude::*;

const SR: f32 = 48_000.0;

/// Force a parameter's smoothed value (what the DSP reads) to `v`.
fn set(param: &FloatParam, v: f32) {
    param.smoothed.reset(v);
}

fn params() -> GuitarRigParams {
    let p = GuitarRigParams::default();
    // Make smoothers start at their defaults, as the host wrapper would.
    let all = [
        &p.input_gain, &p.output_gain, &p.tuner.reference,
        &p.distortion.drive, &p.distortion.tone, &p.distortion.level,
        &p.amp.gain, &p.amp.bass, &p.amp.mid, &p.amp.treble, &p.amp.presence, &p.amp.master,
        &p.delay.time_ms, &p.delay.feedback, &p.delay.tone, &p.delay.mix,
        &p.reverb.size, &p.reverb.damping, &p.reverb.predelay_ms, &p.reverb.mix,
    ];
    for f in all {
        set(f, f.value());
    }
    p
}

/// A loud, noisy, guitar-ish test signal.
fn signal(len: usize) -> Vec<f32> {
    let mut seed = 12345u32;
    (0..len)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
            let t = i as f32 / SR;
            0.8 * (2.0 * std::f32::consts::PI * 110.0 * t).sin() + 0.2 * noise
        })
        .collect()
}

fn run(mut f: impl FnMut(&mut [f32]), input: &[f32]) -> Vec<f32> {
    let mut out = input.to_vec();
    for chunk in out.chunks_mut(64) {
        f(chunk);
    }
    out
}

fn assert_sane(name: &str, out: &[f32], max: f32) {
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(out.iter().all(|v| v.is_finite()), "{name}: non-finite output");
    assert!(peak < max, "{name}: peak {peak} exceeds {max}");
    assert!(peak > 1.0e-4, "{name}: output is silent");
}

#[test]
fn all_modules_stay_bounded_at_extreme_settings() {
    let p = params();
    let input = signal(SR as usize * 3);

    // Max out the most dangerous controls.
    set(&p.distortion.drive, 1.0);
    set(&p.amp.gain, 10.0);
    set(&p.amp.bass, 10.0);
    set(&p.amp.treble, 10.0);
    set(&p.delay.feedback, 0.95);
    set(&p.reverb.size, 1.0);

    let mut d = Distortion::new();
    d.prepare(SR);
    assert_sane("distortion", &run(|b| d.process(&p.distortion, b), &input), 4.0);

    let mut a = Amp::new();
    a.prepare(SR);
    assert_sane("amp", &run(|b| a.process(&p.amp, b), &input), 4.0);

    let mut dl = Delay::new();
    dl.prepare(SR);
    assert_sane("delay", &run(|b| dl.process(&p.delay, b), &input), 8.0);

    let mut r = Reverb::new();
    r.prepare(SR);
    assert_sane("reverb", &run(|b| r.process(&p.reverb, b), &input), 8.0);
}

#[test]
fn delay_produces_echo_at_set_time() {
    let p = params();
    set(&p.delay.time_ms, 250.0);
    set(&p.delay.mix, 1.0);
    set(&p.delay.feedback, 0.0);
    let mut dl = Delay::new();
    dl.prepare(SR);

    let mut input = vec![0.0; SR as usize];
    input[0] = 1.0;
    let out = run(|b| dl.process(&p.delay, b), &input);
    let echo_at = out.iter().enumerate().skip(10).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
    let expected = (0.25 * SR) as usize;
    assert!(echo_at.abs_diff(expected) <= 1, "echo at {echo_at}, expected {expected}");
}
