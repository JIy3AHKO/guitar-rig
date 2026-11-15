//! All automatable parameters of the plugin.
//!
//! Every module has its own parameter group. Parameter IDs are part of saved
//! sessions and automation, so they must never change once released.

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::chain;

pub const EDITOR_WIDTH: u32 = 960;
pub const EDITOR_HEIGHT: u32 = 600;

fn gain_param(name: &str, default_db: f32, min_db: f32, max_db: f32) -> FloatParam {
    FloatParam::new(
        name,
        util::db_to_gain(default_db),
        FloatRange::Skewed {
            min: util::db_to_gain(min_db),
            max: util::db_to_gain(max_db),
            factor: FloatRange::gain_skew_factor(min_db, max_db),
        },
    )
    .with_smoother(SmoothingStyle::Logarithmic(30.0))
    .with_unit(" dB")
    .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
    .with_string_to_value(formatters::s2v_f32_gain_to_db())
}

/// A 0..10 "amp style" control.
fn dial_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 10.0 })
        .with_smoother(SmoothingStyle::Linear(30.0))
        .with_value_to_string(formatters::v2s_f32_rounded(1))
}

fn percent_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(30.0))
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

#[derive(Params)]
pub struct GuitarRigParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// Packed signal chain order, see [`crate::chain`].
    #[persist = "chain"]
    pub chain: Arc<AtomicU64>,

    #[id = "in_gain"]
    pub input_gain: FloatParam,
    #[id = "out_gain"]
    pub output_gain: FloatParam,

    #[nested(group = "Tuner")]
    pub tuner: TunerParams,
    #[nested(group = "Distortion")]
    pub distortion: DistortionParams,
    #[nested(group = "Amp")]
    pub amp: AmpParams,
    #[nested(group = "Delay")]
    pub delay: DelayParams,
    #[nested(group = "Reverb")]
    pub reverb: ReverbParams,
}

#[derive(Params)]
pub struct TunerParams {
    #[id = "tun_on"]
    pub enabled: BoolParam,
    #[id = "tun_mute"]
    pub mute: BoolParam,
    #[id = "tun_ref"]
    pub reference: FloatParam,
}

#[derive(Params)]
pub struct DistortionParams {
    #[id = "dist_on"]
    pub enabled: BoolParam,
    #[id = "dist_drive"]
    pub drive: FloatParam,
    #[id = "dist_tone"]
    pub tone: FloatParam,
    #[id = "dist_level"]
    pub level: FloatParam,
}

#[derive(Params)]
pub struct AmpParams {
    #[id = "amp_on"]
    pub enabled: BoolParam,
    #[id = "amp_gain"]
    pub gain: FloatParam,
    #[id = "amp_bass"]
    pub bass: FloatParam,
    #[id = "amp_mid"]
    pub mid: FloatParam,
    #[id = "amp_treble"]
    pub treble: FloatParam,
    #[id = "amp_pres"]
    pub presence: FloatParam,
    #[id = "amp_master"]
    pub master: FloatParam,
    #[id = "amp_cab"]
    pub cabinet: BoolParam,
}

#[derive(Params)]
pub struct DelayParams {
    #[id = "dly_on"]
    pub enabled: BoolParam,
    #[id = "dly_time"]
    pub time_ms: FloatParam,
    #[id = "dly_fb"]
    pub feedback: FloatParam,
    #[id = "dly_tone"]
    pub tone: FloatParam,
    #[id = "dly_mix"]
    pub mix: FloatParam,
}

#[derive(Params)]
pub struct ReverbParams {
    #[id = "rev_on"]
    pub enabled: BoolParam,
    #[id = "rev_size"]
    pub size: FloatParam,
    #[id = "rev_damp"]
    pub damping: FloatParam,
    #[id = "rev_pre"]
    pub predelay_ms: FloatParam,
    #[id = "rev_mix"]
    pub mix: FloatParam,
}

impl Default for GuitarRigParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(EDITOR_WIDTH, EDITOR_HEIGHT),
            chain: Arc::new(chain::new_shared_chain()),
            input_gain: gain_param("Input", 0.0, -24.0, 24.0),
            output_gain: gain_param("Output", 0.0, -36.0, 12.0),
            tuner: TunerParams {
                enabled: BoolParam::new("Tuner On", true),
                mute: BoolParam::new("Tuner Mute", false),
                reference: FloatParam::new(
                    "Tuner Reference",
                    440.0,
                    FloatRange::Linear { min: 430.0, max: 450.0 },
                )
                .with_step_size(0.5)
                .with_unit(" Hz")
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            },
            distortion: DistortionParams {
                enabled: BoolParam::new("Distortion On", true),
                drive: percent_param("Drive", 0.5),
                tone: percent_param("Tone", 0.5),
                level: gain_param("Dist Level", -6.0, -30.0, 12.0),
            },
            amp: AmpParams {
                enabled: BoolParam::new("Amp On", true),
                gain: dial_param("Amp Gain", 5.0),
                bass: dial_param("Bass", 5.0),
                mid: dial_param("Mid", 5.0),
                treble: dial_param("Treble", 5.0),
                presence: dial_param("Presence", 5.0),
                master: gain_param("Master", -6.0, -40.0, 6.0),
                cabinet: BoolParam::new("Cabinet", true),
            },
            delay: DelayParams {
                enabled: BoolParam::new("Delay On", true),
                time_ms: FloatParam::new(
                    "Delay Time",
                    380.0,
                    FloatRange::Skewed {
                        min: 20.0,
                        max: 2000.0,
                        factor: FloatRange::skew_factor(-1.0),
                    },
                )
                // Slow smoothing gives a natural "tape" pitch bend when the time changes.
                .with_smoother(SmoothingStyle::Linear(150.0))
                .with_unit(" ms")
                .with_value_to_string(formatters::v2s_f32_rounded(0)),
                feedback: FloatParam::new("Feedback", 0.35, FloatRange::Linear { min: 0.0, max: 0.95 })
                    .with_smoother(SmoothingStyle::Linear(30.0))
                    .with_unit("%")
                    .with_value_to_string(formatters::v2s_f32_percentage(0))
                    .with_string_to_value(formatters::s2v_f32_percentage()),
                tone: percent_param("Delay Tone", 0.6),
                mix: percent_param("Delay Mix", 0.3),
            },
            reverb: ReverbParams {
                enabled: BoolParam::new("Reverb On", true),
                size: percent_param("Size", 0.6),
                damping: percent_param("Damping", 0.4),
                predelay_ms: FloatParam::new(
                    "Pre-delay",
                    15.0,
                    FloatRange::Linear { min: 0.0, max: 150.0 },
                )
                .with_unit(" ms")
                .with_value_to_string(formatters::v2s_f32_rounded(0)),
                mix: percent_param("Reverb Mix", 0.25),
            },
        }
    }
}
