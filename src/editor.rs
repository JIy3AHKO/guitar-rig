//! The plugin GUI: a pedalboard view of the signal chain plus a parameter
//! panel for the selected module.

mod widgets;

use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, epaint::CubicBezierShape, pos2, vec2, Align2, Color32, CornerRadius, FontId, Frame,
    Pos2, Rect, Sense, Stroke, StrokeKind, Ui,
};
use nih_plug_egui::{create_egui_editor, egui::Context};
use std::sync::Arc;

use crate::chain::{self, Chain, ModuleKind};
use crate::dsp::tuner::note_for_frequency;
use crate::params::GuitarRigParams;
use crate::SharedState;
use widgets::{knob, meter, module_id, paint_knob, toggle, TEXT, TEXT_DIM};

const BG: Color32 = Color32::from_rgb(22, 22, 27);
const PANEL_BG: Color32 = Color32::from_rgb(32, 32, 39);
const BOARD_BG: Color32 = Color32::from_rgb(44, 40, 38);
const CABLE: Color32 = Color32::from_rgb(20, 20, 20);
const CABLE_HI: Color32 = Color32::from_rgb(70, 70, 75);
const STRIP_HEIGHT: f32 = 214.0;
const CABLE_GAP: f32 = 36.0;
const JACK_W: f32 = 34.0;

fn module_color(kind: ModuleKind) -> Color32 {
    match kind {
        ModuleKind::Tuner => Color32::from_rgb(38, 140, 140),
        ModuleKind::Distortion => Color32::from_rgb(222, 112, 32),
        ModuleKind::Amp => Color32::from_rgb(201, 162, 80),
        ModuleKind::Delay => Color32::from_rgb(48, 98, 190),
        ModuleKind::Reverb => Color32::from_rgb(126, 74, 176),
    }
}

fn module_size(kind: ModuleKind) -> egui::Vec2 {
    match kind {
        ModuleKind::Amp => vec2(176.0, 150.0),
        _ => vec2(112.0, 150.0),
    }
}

fn is_enabled(params: &GuitarRigParams, kind: ModuleKind) -> bool {
    enabled_param(params, kind).value()
}

fn enabled_param(params: &GuitarRigParams, kind: ModuleKind) -> &BoolParam {
    match kind {
        ModuleKind::Tuner => &params.tuner.enabled,
        ModuleKind::Distortion => &params.distortion.enabled,
        ModuleKind::Amp => &params.amp.enabled,
        ModuleKind::Delay => &params.delay.enabled,
        ModuleKind::Reverb => &params.reverb.enabled,
    }
}

/// Normalised values of the most important controls, for the mini knobs drawn on a pedal.
fn mini_knob_values(params: &GuitarRigParams, kind: ModuleKind) -> Vec<f32> {
    match kind {
        ModuleKind::Tuner => vec![],
        ModuleKind::Distortion => {
            let p = &params.distortion;
            vec![p.drive.unmodulated_normalized_value(), p.tone.unmodulated_normalized_value(), p.level.unmodulated_normalized_value()]
        }
        ModuleKind::Amp => {
            let p = &params.amp;
            [&p.gain, &p.bass, &p.mid, &p.treble, &p.presence, &p.master]
                .iter()
                .map(|f| f.unmodulated_normalized_value())
                .collect()
        }
        ModuleKind::Delay => {
            let p = &params.delay;
            vec![p.time_ms.unmodulated_normalized_value(), p.feedback.unmodulated_normalized_value(), p.mix.unmodulated_normalized_value()]
        }
        ModuleKind::Reverb => {
            let p = &params.reverb;
            vec![p.size.unmodulated_normalized_value(), p.damping.unmodulated_normalized_value(), p.mix.unmodulated_normalized_value()]
        }
    }
}

struct DragState {
    kind: ModuleKind,
    /// Horizontal offset between the pointer and the module centre when the drag began.
    grab_dx: f32,
}

/// Smoothed tuner display state.
#[derive(Default)]
struct TunerDisplay {
    freq: f32,
    cents: f32,
    last_seen: f64,
}

struct EditorState {
    params: Arc<GuitarRigParams>,
    shared: Arc<SharedState>,
    selected: ModuleKind,
    drag: Option<DragState>,
    tuner: TunerDisplay,
}

pub fn create(params: Arc<GuitarRigParams>, shared: Arc<SharedState>) -> Option<Box<dyn Editor>> {
    let state = EditorState {
        params: params.clone(),
        shared,
        selected: ModuleKind::Distortion,
        drag: None,
        tuner: TunerDisplay::default(),
    };
    create_egui_editor(
        params.editor_state.clone(),
        state,
        |ctx, _state| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG;
            ctx.set_visuals(visuals);
        },
        draw,
    )
}

fn draw(ctx: &Context, setter: &ParamSetter, state: &mut EditorState) {
    // Meters and tuner are live data: keep repainting.
    ctx.request_repaint();
    update_tuner_display(ctx, state);

    egui::CentralPanel::default()
        .frame(Frame::new().fill(BG).inner_margin(14.0))
        .show(ctx, |ui| {
            top_bar(ui, setter, state);
            ui.add_space(10.0);
            chain_view(ui, setter, state);
            ui.add_space(8.0);
            toolbar(ui, state);
            ui.add_space(8.0);
            parameter_panel(ui, setter, state);
        });
}

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------

fn top_bar(ui: &mut Ui, setter: &ParamSetter, state: &EditorState) {
    let params = &state.params;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new("GUITAR RIG").size(26.0).strong().color(TEXT));
            ui.label(egui::RichText::new("pedalboard · amp · cab").size(12.0).color(TEXT_DIM));
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            meter(ui, state.shared.output_peak.get(), 70.0);
            knob(ui, setter, &params.output_gain, 44.0, Color32::from_rgb(200, 200, 210));
            ui.add_space(16.0);
            meter(ui, state.shared.input_peak.get(), 70.0);
            knob(ui, setter, &params.input_gain, 44.0, Color32::from_rgb(200, 200, 210));
        });
    });
}

// ---------------------------------------------------------------------------
// Pedalboard / chain view
// ---------------------------------------------------------------------------

/// Lay out `kinds` horizontally, centred in `strip`. Returns (input jack, module rects, output jack).
fn layout(strip: Rect, kinds: &[ModuleKind]) -> (Pos2, Vec<Rect>, Pos2) {
    let modules_w: f32 = kinds.iter().map(|k| module_size(*k).x).sum();
    let total = modules_w + CABLE_GAP * (kinds.len() as f32 + 1.0) + JACK_W * 2.0;
    // If it doesn't fit, squeeze the cables.
    let gap = if total > strip.width() - 20.0 {
        ((strip.width() - 20.0 - modules_w - JACK_W * 2.0) / (kinds.len() as f32 + 1.0)).max(8.0)
    } else {
        CABLE_GAP
    };
    let total = modules_w + gap * (kinds.len() as f32 + 1.0) + JACK_W * 2.0;
    let cy = strip.center().y;
    let mut x = strip.center().x - total / 2.0;
    let in_jack = pos2(x + JACK_W / 2.0, cy);
    x += JACK_W + gap;
    let mut rects = Vec::with_capacity(kinds.len());
    for k in kinds {
        let size = module_size(*k);
        rects.push(Rect::from_min_size(pos2(x, cy - size.y / 2.0), size));
        x += size.x + gap;
    }
    let out_jack = pos2(x + JACK_W / 2.0, cy);
    (in_jack, rects, out_jack)
}

fn draw_cable(painter: &egui::Painter, from: Pos2, to: Pos2, active: bool) {
    let sag = ((to.x - from.x).abs() * 0.35).clamp(10.0, 40.0);
    let points = [from, from + vec2(sag, sag), to + vec2(-sag, sag), to];
    let color = if active { CABLE } else { Color32::from_rgb(35, 35, 35) };
    painter.add(CubicBezierShape::from_points_stroke(points, false, Color32::TRANSPARENT, Stroke::new(6.0, color)));
    painter.add(CubicBezierShape::from_points_stroke(
        points.map(|p| p + vec2(0.0, -1.5)),
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.0, CABLE_HI),
    ));
    for p in [from, to] {
        painter.circle_filled(p, 5.0, Color32::from_gray(150));
        painter.circle_filled(p, 2.5, Color32::from_gray(60));
    }
}

fn draw_jack(painter: &egui::Painter, center: Pos2, label: &str) {
    let r = Rect::from_center_size(center, vec2(JACK_W, 50.0));
    painter.rect_filled(r, 6.0, Color32::from_gray(30));
    painter.rect_stroke(r, 6.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);
    painter.circle_filled(center, 8.0, Color32::from_gray(170));
    painter.circle_filled(center, 4.0, Color32::from_gray(20));
    painter.text(r.center_bottom() + vec2(0.0, 12.0), Align2::CENTER_CENTER, label, FontId::proportional(11.0), TEXT_DIM);
}

fn footswitch_rect(kind: ModuleKind, rect: Rect) -> Rect {
    match kind {
        ModuleKind::Amp => Rect::from_center_size(pos2(rect.right() - 22.0, rect.top() + 22.0), vec2(22.0, 22.0)),
        _ => Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 26.0), vec2(30.0, 30.0)),
    }
}

fn draw_module(
    painter: &egui::Painter,
    state: &EditorState,
    kind: ModuleKind,
    rect: Rect,
    selected: bool,
    hovered: bool,
    floating: bool,
) {
    let params = &state.params;
    let on = is_enabled(params, kind);
    let color = module_color(kind);

    // Drop shadow (bigger when picked up).
    let shadow_off = if floating { 8.0 } else { 3.0 };
    painter.rect_filled(rect.translate(vec2(0.0, shadow_off)), 12.0, Color32::from_black_alpha(110));

    if kind == ModuleKind::Amp {
        draw_amp_body(painter, params, rect, color);
    } else {
        draw_pedal_body(painter, state, kind, rect, color);
    }

    // Status LED.
    let led = if kind == ModuleKind::Amp {
        pos2(rect.right() - 46.0, rect.top() + 22.0)
    } else {
        pos2(rect.center().x, rect.top() + 14.0)
    };
    let led_color = if on { Color32::from_rgb(255, 50, 40) } else { Color32::from_rgb(70, 20, 20) };
    if on {
        painter.circle_filled(led, 8.0, Color32::from_rgba_unmultiplied(255, 60, 40, 60));
    }
    painter.circle_filled(led, 4.0, led_color);

    // Footswitch / power switch.
    let fs = footswitch_rect(kind, rect);
    painter.circle_filled(fs.center(), fs.width() / 2.0, Color32::from_gray(120));
    painter.circle_filled(fs.center(), fs.width() / 2.0 - 3.0, Color32::from_gray(185));
    painter.circle_stroke(fs.center(), fs.width() / 2.0 - 6.0, Stroke::new(1.0, Color32::from_gray(140)));

    if !on {
        painter.rect_filled(rect, 12.0, Color32::from_black_alpha(110));
    }
    let outline = if selected {
        Stroke::new(2.5, Color32::WHITE)
    } else if hovered {
        Stroke::new(1.5, Color32::from_gray(160))
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(160))
    };
    painter.rect_stroke(rect, 12.0, outline, StrokeKind::Outside);
}

fn draw_pedal_body(painter: &egui::Painter, state: &EditorState, kind: ModuleKind, rect: Rect, color: Color32) {
    painter.rect_filled(rect, 12.0, color);
    // Subtle top highlight.
    let hi = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + rect.height() * 0.45));
    painter.rect_filled(hi, CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }, Color32::from_white_alpha(18));

    painter.text(
        pos2(rect.center().x, rect.top() + 32.0),
        Align2::CENTER_CENTER,
        kind.name().to_uppercase(),
        FontId::proportional(13.0),
        Color32::WHITE,
    );

    if kind == ModuleKind::Tuner {
        let screen = Rect::from_center_size(pos2(rect.center().x, rect.top() + 72.0), vec2(80.0, 40.0));
        painter.rect_filled(screen, 4.0, Color32::from_rgb(12, 20, 20));
        let (text, col) = match tuner_note(state) {
            Some((note, cents)) => (note, cents_color(cents)),
            None => ("--".to_owned(), Color32::from_gray(90)),
        };
        painter.text(screen.center(), Align2::CENTER_CENTER, text, FontId::monospace(20.0), col);
    } else {
        let values = mini_knob_values(&state.params, kind);
        let n = values.len() as f32;
        for (i, v) in values.iter().enumerate() {
            let x = rect.left() + rect.width() * (i as f32 + 0.5) / n;
            paint_knob(painter, pos2(x, rect.top() + 70.0), 12.0, *v, Color32::WHITE, false);
        }
    }
}

fn draw_amp_body(painter: &egui::Painter, params: &GuitarRigParams, rect: Rect, gold: Color32) {
    // Black tolex head with a gold control panel and grille cloth.
    painter.rect_filled(rect, 12.0, Color32::from_rgb(26, 24, 22));
    let panel = Rect::from_min_max(rect.min + vec2(8.0, 8.0), pos2(rect.max.x - 8.0, rect.min.y + 62.0));
    painter.rect_filled(panel, 5.0, gold);
    painter.text(
        panel.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        "AMP",
        FontId::proportional(15.0),
        Color32::from_rgb(40, 30, 10),
    );

    let values = mini_knob_values(params, ModuleKind::Amp);
    let knob_row = Rect::from_min_max(pos2(rect.left() + 8.0, panel.bottom() + 6.0), pos2(rect.right() - 8.0, panel.bottom() + 30.0));
    let n = values.len() as f32;
    for (i, v) in values.iter().enumerate() {
        let x = knob_row.left() + knob_row.width() * (i as f32 + 0.5) / n;
        paint_knob(painter, pos2(x, knob_row.center().y), 9.0, *v, Color32::WHITE, false);
    }

    let grille = Rect::from_min_max(pos2(rect.left() + 8.0, knob_row.bottom() + 6.0), rect.max - vec2(8.0, 8.0));
    painter.rect_filled(grille, 5.0, Color32::from_rgb(52, 46, 40));
    let clip = painter.with_clip_rect(grille);
    let mut x = grille.left() - grille.height();
    while x < grille.right() {
        clip.line_segment(
            [pos2(x, grille.bottom()), pos2(x + grille.height(), grille.top())],
            Stroke::new(1.0, Color32::from_rgb(64, 58, 50)),
        );
        x += 6.0;
    }
    let cab = if params.amp.cabinet.value() { "4x12 CAB" } else { "DI (no cab)" };
    painter.text(grille.center(), Align2::CENTER_CENTER, cab, FontId::proportional(11.0), Color32::from_rgb(150, 135, 110));
}

fn chain_view(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState) {
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP_HEIGHT), Sense::hover());
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 10.0, BOARD_BG);
    painter.text(
        strip.left_top() + vec2(12.0, 10.0),
        Align2::LEFT_TOP,
        "Drag pedals to reorder · click to edit · footswitch to bypass",
        FontId::proportional(11.0),
        Color32::from_gray(120),
    );

    let chain = chain::load(&state.params.chain);
    let kinds = chain.to_vec();
    let pointer = ui.input(|i| i.pointer.interact_pos());

    // While dragging, preview the order the chain would have if dropped now.
    let mut display_order = kinds.clone();
    let mut floating: Option<(ModuleKind, Rect)> = None;
    if let (Some(drag), Some(p)) = (&state.drag, pointer) {
        if kinds.contains(&drag.kind) {
            let (_, base_rects, _) = layout(strip, &kinds);
            let dragged_center = p.x - drag.grab_dx;
            let others: Vec<ModuleKind> = kinds.iter().copied().filter(|k| *k != drag.kind).collect();
            let target = kinds
                .iter()
                .zip(base_rects.iter())
                .filter(|(k, r)| **k != drag.kind && r.center().x < dragged_center)
                .count();
            display_order = others;
            display_order.insert(target.min(display_order.len()), drag.kind);

            let size = module_size(drag.kind);
            let r = Rect::from_center_size(pos2(dragged_center, strip.center().y - 6.0), size);
            floating = Some((drag.kind, r));
        }
    }

    let (in_jack, rects, out_jack) = layout(strip, &display_order);

    // Cables first so pedals sit on top of them.
    let mut prev = in_jack + vec2(JACK_W / 2.0, 0.0);
    for rect in &rects {
        draw_cable(&painter, prev, rect.left_center() + vec2(0.0, 10.0), true);
        prev = rect.right_center() + vec2(0.0, 10.0);
    }
    draw_cable(&painter, prev, out_jack - vec2(JACK_W / 2.0, 0.0), true);
    draw_jack(&painter, in_jack, "IN");
    draw_jack(&painter, out_jack, "OUT");

    let mut new_order: Option<Vec<ModuleKind>> = None;
    for (kind, rect) in display_order.iter().copied().zip(rects.iter().copied()) {
        let is_floating = floating.map(|(k, _)| k == kind).unwrap_or(false);
        if is_floating {
            // Placeholder where the pedal will land.
            painter.rect_stroke(rect, 12.0, Stroke::new(1.5, Color32::from_gray(110)), StrokeKind::Inside);
        }
        let hit_rect = if is_floating { floating.unwrap().1 } else { rect };

        let body = ui.interact(hit_rect, module_id(kind.name(), "body"), Sense::click_and_drag());
        let fs = ui.interact(footswitch_rect(kind, hit_rect), module_id(kind.name(), "fs"), Sense::click());

        if fs.clicked() {
            let p = enabled_param(&state.params, kind);
            setter.begin_set_parameter(p);
            setter.set_parameter(p, !p.value());
            setter.end_set_parameter(p);
        } else if body.clicked() {
            state.selected = kind;
        }
        if body.drag_started() {
            if let Some(p) = body.interact_pointer_pos() {
                state.drag = Some(DragState { kind, grab_dx: p.x - rect.center().x });
                state.selected = kind;
            }
        }
        if body.drag_stopped() {
            new_order = Some(display_order.clone());
            state.drag = None;
        }

        if !is_floating {
            draw_module(&painter, state, kind, rect, state.selected == kind, body.hovered() || fs.hovered(), false);
        }
        fs.on_hover_text(if is_enabled(&state.params, kind) { "Bypass" } else { "Engage" });
    }

    // Draw the picked-up pedal last so it floats above everything.
    if let Some((kind, r)) = floating {
        let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, module_id(kind.name(), "float")));
        draw_module(&top, state, kind, r, true, true, true);
    }

    if let Some(order) = new_order {
        chain::store(&state.params.chain, &Chain::from_slice(&order));
    }
    if chain.is_empty() {
        painter.text(
            strip.center() + vec2(0.0, -36.0),
            Align2::CENTER_CENTER,
            "Empty chain — add a module below",
            FontId::proportional(14.0),
            TEXT_DIM,
        );
    }
}

// ---------------------------------------------------------------------------
// Toolbar: add / move / remove modules
// ---------------------------------------------------------------------------

fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    let chain = chain::load(&state.params.chain);
    let mut kinds = chain.to_vec();
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Add:").color(TEXT_DIM));
        let mut any = false;
        for kind in ModuleKind::ALL {
            if !chain.contains(kind) {
                any = true;
                let button = egui::Button::new(egui::RichText::new(format!("+ {}", kind.name())).color(Color32::WHITE))
                    .fill(module_color(kind).gamma_multiply(0.7));
                if ui.add(button).clicked() {
                    // Amps and time-based effects have conventional positions.
                    let pos = insertion_index(&kinds, kind);
                    kinds.insert(pos, kind);
                    state.selected = kind;
                    changed = true;
                }
            }
        }
        if !any {
            ui.label(egui::RichText::new("all modules connected").color(Color32::from_gray(100)));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let sel = state.selected;
            if let Some(i) = kinds.iter().position(|k| *k == sel) {
                if ui.button("Remove").on_hover_text("Disconnect from the chain").clicked() {
                    kinds.remove(i);
                    changed = true;
                }
                if ui.add_enabled(i + 1 < kinds.len(), egui::Button::new("Move ▶")).clicked() {
                    kinds.swap(i, i + 1);
                    changed = true;
                }
                if ui.add_enabled(i > 0, egui::Button::new("◀ Move")).clicked() {
                    kinds.swap(i, i - 1);
                    changed = true;
                }
                ui.label(egui::RichText::new(sel.name()).color(TEXT_DIM));
            }
        });
    });

    if changed {
        chain::store(&state.params.chain, &Chain::from_slice(&kinds));
    }
}

/// Where to insert a module so the chain follows the classic order
/// tuner -> drive -> amp -> delay -> reverb.
fn insertion_index(kinds: &[ModuleKind], kind: ModuleKind) -> usize {
    let rank = |k: ModuleKind| ModuleKind::ALL.iter().position(|x| *x == k).unwrap_or(0);
    kinds.iter().position(|k| rank(*k) > rank(kind)).unwrap_or(kinds.len())
}

// ---------------------------------------------------------------------------
// Parameter panel
// ---------------------------------------------------------------------------

fn parameter_panel(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState) {
    let kind = state.selected;
    let accent = module_color(kind);
    let connected = chain::load(&state.params.chain).contains(kind);

    Frame::new()
        .fill(PANEL_BG)
        .corner_radius(10.0)
        .inner_margin(16.0)
        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.5)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(ui.available_height() - 4.0);

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(kind.name().to_uppercase()).size(18.0).strong().color(accent));
                if !connected {
                    ui.label(egui::RichText::new("(not connected)").color(TEXT_DIM));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle(ui, setter, enabled_param(&state.params, kind), "On", accent);
                });
            });
            ui.add_space(10.0);

            let params = state.params.clone();
            match kind {
                ModuleKind::Tuner => tuner_panel(ui, setter, state, accent),
                ModuleKind::Distortion => {
                    let p = &params.distortion;
                    ui.horizontal(|ui| {
                        knob(ui, setter, &p.drive, 72.0, accent);
                        knob(ui, setter, &p.tone, 72.0, accent);
                        knob(ui, setter, &p.level, 72.0, accent);
                    });
                }
                ModuleKind::Amp => {
                    let p = &params.amp;
                    ui.horizontal(|ui| {
                        knob(ui, setter, &p.gain, 66.0, accent);
                        ui.add_space(10.0);
                        knob(ui, setter, &p.bass, 66.0, accent);
                        knob(ui, setter, &p.mid, 66.0, accent);
                        knob(ui, setter, &p.treble, 66.0, accent);
                        knob(ui, setter, &p.presence, 66.0, accent);
                        ui.add_space(10.0);
                        knob(ui, setter, &p.master, 66.0, accent);
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.add_space(30.0);
                            toggle(ui, setter, &p.cabinet, "Cabinet sim", accent);
                        });
                    });
                }
                ModuleKind::Delay => {
                    let p = &params.delay;
                    ui.horizontal(|ui| {
                        knob(ui, setter, &p.time_ms, 72.0, accent);
                        knob(ui, setter, &p.feedback, 72.0, accent);
                        knob(ui, setter, &p.tone, 72.0, accent);
                        knob(ui, setter, &p.mix, 72.0, accent);
                    });
                }
                ModuleKind::Reverb => {
                    let p = &params.reverb;
                    ui.horizontal(|ui| {
                        knob(ui, setter, &p.size, 72.0, accent);
                        knob(ui, setter, &p.damping, 72.0, accent);
                        knob(ui, setter, &p.predelay_ms, 72.0, accent);
                        knob(ui, setter, &p.mix, 72.0, accent);
                    });
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Tuner
// ---------------------------------------------------------------------------

const TUNER_HOLD_SECONDS: f64 = 0.8;

fn update_tuner_display(ctx: &Context, state: &mut EditorState) {
    let now = ctx.input(|i| i.time);
    let f = state.shared.tuner.frequency();
    let t = &mut state.tuner;
    if f > 0.0 {
        let reference = state.params.tuner.reference.value();
        let (_, _, cents) = note_for_frequency(f, reference);
        // Jump on note changes, otherwise smooth the needle.
        let jumped = t.freq <= 0.0 || (f / t.freq).log2().abs() > 1.0 / 24.0;
        if jumped {
            t.freq = f;
            t.cents = cents;
        } else {
            t.freq += (f - t.freq) * 0.3;
            let (_, _, c) = note_for_frequency(t.freq, reference);
            t.cents = c;
        }
        t.last_seen = now;
    } else if now - t.last_seen > TUNER_HOLD_SECONDS {
        t.freq = 0.0;
    }
}

fn tuner_note(state: &EditorState) -> Option<(String, f32)> {
    let t = &state.tuner;
    if t.freq <= 0.0 || !is_enabled(&state.params, ModuleKind::Tuner) {
        return None;
    }
    let (note, _, _) = note_for_frequency(t.freq, state.params.tuner.reference.value());
    Some((note.to_owned(), t.cents))
}

fn cents_color(cents: f32) -> Color32 {
    if cents.abs() < 3.0 {
        Color32::from_rgb(80, 230, 120)
    } else if cents.abs() < 15.0 {
        Color32::from_rgb(230, 200, 70)
    } else {
        Color32::from_rgb(240, 80, 60)
    }
}

fn tuner_panel(ui: &mut Ui, setter: &ParamSetter, state: &mut EditorState, accent: Color32) {
    let params = state.params.clone();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(520.0, 150.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, Color32::from_rgb(12, 18, 18));

        let reference = params.tuner.reference.value();
        let reading = tuner_note(state);
        let t = &state.tuner;

        // Note name + octave.
        let (label, color) = match &reading {
            Some((_, cents)) => {
                let (note, octave, _) = note_for_frequency(t.freq, reference);
                (format!("{note}{octave}"), cents_color(*cents))
            }
            None => ("—".to_owned(), Color32::from_gray(80)),
        };
        painter.text(pos2(rect.center().x, rect.top() + 40.0), Align2::CENTER_CENTER, label, FontId::proportional(40.0), color);

        // Cents scale -50 .. +50.
        let scale = Rect::from_min_max(pos2(rect.left() + 30.0, rect.top() + 82.0), pos2(rect.right() - 30.0, rect.top() + 112.0));
        for c in (-50..=50).step_by(10) {
            let x = scale.left() + scale.width() * (c as f32 + 50.0) / 100.0;
            let h = if c == 0 { 30.0 } else if c % 50 == 0 { 20.0 } else { 12.0 };
            painter.line_segment(
                [pos2(x, scale.center().y - h / 2.0), pos2(x, scale.center().y + h / 2.0)],
                Stroke::new(if c == 0 { 2.0 } else { 1.0 }, Color32::from_gray(if c == 0 { 200 } else { 90 })),
            );
        }
        let in_tune = Rect::from_center_size(scale.center(), vec2(scale.width() * 0.06, scale.height()));
        painter.rect_filled(in_tune, 3.0, Color32::from_rgba_unmultiplied(80, 230, 120, 30));

        if let Some((_, cents)) = reading {
            let x = scale.left() + scale.width() * (cents.clamp(-50.0, 50.0) + 50.0) / 100.0;
            painter.line_segment([pos2(x, scale.top() - 6.0), pos2(x, scale.bottom() + 6.0)], Stroke::new(4.0, cents_color(cents)));
            painter.text(
                pos2(rect.center().x, rect.bottom() - 16.0),
                Align2::CENTER_CENTER,
                format!("{:+.1} cents   ·   {:.2} Hz", cents, t.freq),
                FontId::monospace(13.0),
                TEXT_DIM,
            );
        } else {
            let hint = if !is_enabled(&params, ModuleKind::Tuner) {
                "Tuner is bypassed"
            } else if !chain::load(&params.chain).contains(ModuleKind::Tuner) {
                "Tuner is not connected"
            } else {
                "Play a single note"
            };
            painter.text(pos2(rect.center().x, rect.bottom() - 16.0), Align2::CENTER_CENTER, hint, FontId::proportional(13.0), TEXT_DIM);
        }

        ui.add_space(20.0);
        ui.vertical(|ui| {
            knob(ui, setter, &params.tuner.reference, 60.0, accent);
            ui.add_space(6.0);
            toggle(ui, setter, &params.tuner.mute, "Mute output", accent);
        });
    });
}
