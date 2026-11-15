//! Guitar Rig — a modular pedalboard + amp plugin (VST3 / CLAP).

use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub mod chain;
pub mod dsp;
mod editor;
pub mod params;

use chain::{Chain, ModuleKind};
use dsp::{amp::Amp, delay::Delay, distortion::Distortion, reverb::Reverb, tuner::Tuner};
use dsp::tuner::TunerReadout;
use params::GuitarRigParams;

/// Processing happens in sub-blocks of at most this many samples so that
/// filter coefficients can follow smoothed parameters without per-sample cost.
const BLOCK: usize = 64;
/// Footswitch crossfade time.
const BYPASS_FADE_SECONDS: f32 = 0.012;

/// Lock-free peak meter value shared with the GUI.
#[derive(Default)]
pub struct PeakMeter(AtomicU32);

impl PeakMeter {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// State the audio thread publishes for the GUI.
#[derive(Default)]
pub struct SharedState {
    pub tuner: Arc<TunerReadout>,
    pub input_peak: PeakMeter,
    pub output_peak: PeakMeter,
}

pub struct GuitarRig {
    params: Arc<GuitarRigParams>,
    shared: Arc<SharedState>,
    sample_rate: f32,
    num_inputs: usize,

    tuner: Tuner,
    distortion: Distortion,
    amp: Amp,
    delay: Delay,
    reverb: Reverb,

    /// Per-module wet amount used for click-free bypass, indexed by `ModuleKind as usize`.
    bypass_mix: [f32; ModuleKind::ALL.len()],
    prev_chain: Chain,
    meter_in: f32,
    meter_out: f32,
}

impl Default for GuitarRig {
    fn default() -> Self {
        let shared = Arc::new(SharedState::default());
        Self {
            params: Arc::new(GuitarRigParams::default()),
            tuner: Tuner::new(shared.tuner.clone()),
            shared,
            sample_rate: 48_000.0,
            num_inputs: 2,
            distortion: Distortion::new(),
            amp: Amp::new(),
            delay: Delay::new(),
            reverb: Reverb::new(),
            bypass_mix: [1.0; ModuleKind::ALL.len()],
            prev_chain: Chain::default(),
            meter_in: 0.0,
            meter_out: 0.0,
        }
    }
}

impl GuitarRig {
    fn is_enabled(&self, kind: ModuleKind) -> bool {
        let p = &self.params;
        match kind {
            ModuleKind::Tuner => p.tuner.enabled.value(),
            ModuleKind::Distortion => p.distortion.enabled.value(),
            ModuleKind::Amp => p.amp.enabled.value(),
            ModuleKind::Delay => p.delay.enabled.value(),
            ModuleKind::Reverb => p.reverb.enabled.value(),
        }
    }

    fn reset_module(&mut self, kind: ModuleKind) {
        match kind {
            ModuleKind::Tuner => self.tuner.reset(),
            ModuleKind::Distortion => self.distortion.reset(),
            ModuleKind::Amp => self.amp.reset(),
            ModuleKind::Delay => self.delay.reset(),
            ModuleKind::Reverb => self.reverb.reset(),
        }
    }

    fn process_module(&mut self, kind: ModuleKind, buf: &mut [f32]) {
        let p = self.params.clone();
        match kind {
            ModuleKind::Tuner => {
                self.tuner.analyse(buf);
                if p.tuner.mute.value() {
                    buf.fill(0.0);
                }
            }
            ModuleKind::Distortion => self.distortion.process(&p.distortion, buf),
            ModuleKind::Amp => self.amp.process(&p.amp, buf),
            ModuleKind::Delay => self.delay.process(&p.delay, buf),
            ModuleKind::Reverb => self.reverb.process(&p.reverb, buf),
        }
    }

    /// Run one module with a crossfaded footswitch.
    fn run_slot(&mut self, kind: ModuleKind, buf: &mut [f32]) {
        let idx = kind as usize;
        let target = if self.is_enabled(kind) { 1.0 } else { 0.0 };
        let mut mix = self.bypass_mix[idx];

        if mix == 0.0 && target == 0.0 {
            if kind == ModuleKind::Tuner {
                self.shared.tuner.set_frequency(0.0);
            }
            return;
        }
        if mix == 1.0 && target == 1.0 {
            self.process_module(kind, buf);
            return;
        }

        let mut dry = [0.0f32; BLOCK];
        dry[..buf.len()].copy_from_slice(buf);
        self.process_module(kind, buf);
        let step = 1.0 / (BYPASS_FADE_SECONDS * self.sample_rate);
        for (wet, &d) in buf.iter_mut().zip(dry.iter()) {
            mix = if target > mix { (mix + step).min(1.0) } else { (mix - step).max(0.0) };
            *wet = d + (*wet - d) * mix;
        }
        self.bypass_mix[idx] = mix;
        if mix == 0.0 {
            // Fully bypassed: clear state so it starts fresh when re-enabled.
            self.reset_module(kind);
        }
    }

    fn sync_chain(&mut self) -> Chain {
        let chain = chain::load(&self.params.chain);
        if chain != self.prev_chain {
            for kind in chain.iter() {
                if !self.prev_chain.contains(kind) {
                    // Newly connected: start clean and fade in.
                    self.reset_module(kind);
                    self.bypass_mix[kind as usize] = 0.0;
                }
            }
            if !chain.contains(ModuleKind::Tuner) {
                self.shared.tuner.set_frequency(0.0);
            }
            self.prev_chain = chain;
        }
        chain
    }
}

impl Plugin for GuitarRig {
    const NAME: &'static str = "Guitar Rig";
    const VENDOR: &'static str = "Andrew Luzan";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.shared.clone())
    }

    fn initialize(
        &mut self,
        layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        self.sample_rate = sr;
        self.num_inputs = layout.main_input_channels.map(|c| c.get() as usize).unwrap_or(0);

        // All allocation happens here, off the audio thread.
        self.tuner.prepare(sr);
        self.distortion.prepare(sr);
        self.amp.prepare(sr);
        self.delay.prepare(sr);
        self.reverb.prepare(sr);
        true
    }

    fn reset(&mut self) {
        for kind in ModuleKind::ALL {
            self.reset_module(kind);
            self.bypass_mix[kind as usize] = if self.is_enabled(kind) { 1.0 } else { 0.0 };
        }
        self.prev_chain = chain::load(&self.params.chain);
        self.meter_in = 0.0;
        self.meter_out = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let chain = self.sync_chain();
        let params = self.params.clone();

        let channels = buffer.as_slice();
        let num_channels = channels.len();
        if num_channels == 0 {
            return ProcessStatus::Normal;
        }
        let num_samples = channels[0].len();
        let num_inputs = self.num_inputs.min(num_channels).max(1);
        let input_scale = 1.0 / num_inputs as f32;

        let meter_decay = (-(BLOCK as f32) / (0.25 * self.sample_rate)).exp();
        let mut pos = 0;
        while pos < num_samples {
            let len = BLOCK.min(num_samples - pos);
            let mut mono = [0.0f32; BLOCK];
            let mono = &mut mono[..len];

            // Guitar is mono: fold the input channels down.
            let mut peak_in = 0.0f32;
            for (i, s) in mono.iter_mut().enumerate() {
                let mut x = 0.0;
                for ch in channels.iter().take(num_inputs) {
                    x += ch[pos + i];
                }
                x *= input_scale * params.input_gain.smoothed.next();
                peak_in = peak_in.max(x.abs());
                *s = x;
            }

            for kind in chain.iter() {
                self.run_slot(kind, mono);
            }

            let mut peak_out = 0.0f32;
            for (i, s) in mono.iter_mut().enumerate() {
                let y = *s * params.output_gain.smoothed.next();
                // Guard against NaN/inf ever reaching the host.
                let y = if y.is_finite() { y } else { 0.0 };
                peak_out = peak_out.max(y.abs());
                for ch in channels.iter_mut() {
                    ch[pos + i] = y;
                }
            }

            self.meter_in = peak_in.max(self.meter_in * meter_decay);
            self.meter_out = peak_out.max(self.meter_out * meter_decay);
            pos += len;
        }

        self.shared.input_peak.set(self.meter_in);
        self.shared.output_peak.set(self.meter_out);

        // Delay and reverb tails keep ringing after the input stops.
        ProcessStatus::Normal
    }
}

impl ClapPlugin for GuitarRig {
    const CLAP_ID: &'static str = "com.andrewluzan.guitar-rig";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Modular guitar pedalboard and amp");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Distortion,
        ClapFeature::Delay,
        ClapFeature::Reverb,
    ];
}

impl Vst3Plugin for GuitarRig {
    const VST3_CLASS_ID: [u8; 16] = *b"GuitarRigALuzan1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Delay,
        Vst3SubCategory::Reverb,
    ];
}

nih_export_clap!(GuitarRig);
nih_export_vst3!(GuitarRig);
