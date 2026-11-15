//! Custom egui widgets bound to nih-plug parameters.

use nih_plug::prelude::*;
use nih_plug_egui::egui::{
    self, epaint::PathStroke, pos2, vec2, Align2, Color32, FontId, Id, Pos2, Rect, Response,
    Sense, Shape, Stroke, StrokeKind, Ui,
};
use std::f32::consts::PI;

pub const TEXT: Color32 = Color32::from_rgb(225, 225, 230);
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 150);
pub const KNOB_BG: Color32 = Color32::from_rgb(28, 28, 34);
pub const TRACK: Color32 = Color32::from_rgb(60, 60, 70);

// Screen y points down, so angles increase clockwise. 0.75π is "7:30 o'clock".
const START_ANGLE: f32 = PI * 0.75;
const SWEEP: f32 = PI * 1.5; // 270°, ending at "4:30 o'clock"

fn knob_angle(normalized: f32) -> f32 {
    START_ANGLE + SWEEP * normalized.clamp(0.0, 1.0)
}

fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    pos2(center.x + radius * angle.cos(), center.y + radius * angle.sin())
}

fn arc_points(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let segments = ((to - from).abs() * radius / 3.0).ceil().max(2.0) as usize;
    (0..=segments)
        .map(|i| polar(center, radius, from + (to - from) * i as f32 / segments as f32))
        .collect()
}

/// Draw just the knob graphic (also used for the mini knobs on the pedals).
pub fn paint_knob(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    normalized: f32,
    accent: Color32,
    with_track: bool,
) {
    let a0 = knob_angle(0.0);
    let a = knob_angle(normalized);
    if with_track {
        let track_r = radius + 5.0;
        painter.add(Shape::line(
            arc_points(center, track_r, a0, knob_angle(1.0)),
            PathStroke::new(3.0, TRACK),
        ));
        if normalized > 0.001 {
            painter.add(Shape::line(arc_points(center, track_r, a0, a), PathStroke::new(3.0, accent)));
        }
    }
    painter.circle_filled(center + vec2(0.0, 1.5), radius, Color32::from_black_alpha(90));
    painter.circle_filled(center, radius, KNOB_BG);
    painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::from_gray(80)));
    painter.line_segment(
        [polar(center, radius * 0.25, a), polar(center, radius * 0.85, a)],
        Stroke::new(2.5, Color32::WHITE),
    );
}

/// A rotary knob controlling a parameter.
///
/// Drag vertically to change (hold Shift for fine control), scroll to nudge,
/// double-click to reset to the default.
pub fn knob<P: Param>(
    ui: &mut Ui,
    setter: &ParamSetter,
    param: &P,
    diameter: f32,
    accent: Color32,
) -> Response {
    let width = diameter.max(64.0) + 12.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, diameter + 52.0), Sense::click_and_drag());
    let id = response.id;
    let drag_id = id.with("drag_value");

    if response.drag_started() {
        setter.begin_set_parameter(param);
        ui.memory_mut(|m| m.data.insert_temp(drag_id, param.unmodulated_normalized_value()));
    }
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let speed = if fine { 0.0008 } else { 0.005 };
        let start: f32 = ui
            .memory(|m| m.data.get_temp(drag_id))
            .unwrap_or_else(|| param.unmodulated_normalized_value());
        let value = (start - response.drag_delta().y * speed).clamp(0.0, 1.0);
        ui.memory_mut(|m| m.data.insert_temp(drag_id, value));
        setter.set_parameter_normalized(param, value);
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }
    if response.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let value = (param.unmodulated_normalized_value() + scroll * 0.0015).clamp(0.0, 1.0);
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, value);
            setter.end_set_parameter(param);
        }
    }

    let painter = ui.painter_at(rect.expand(4.0));
    let center = pos2(rect.center().x, rect.top() + 8.0 + diameter / 2.0);
    let highlight = response.hovered() || response.dragged();
    let color = if highlight { accent.gamma_multiply(1.2) } else { accent };
    paint_knob(&painter, center, diameter / 2.0 - 4.0, param.unmodulated_normalized_value(), color, true);

    painter.text(
        pos2(rect.center().x, rect.bottom() - 30.0),
        Align2::CENTER_CENTER,
        param.name(),
        FontId::proportional(13.0),
        TEXT,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 13.0),
        Align2::CENTER_CENTER,
        param.normalized_value_to_string(param.unmodulated_normalized_value(), true),
        FontId::monospace(12.0),
        if highlight { TEXT } else { TEXT_DIM },
    );

    response.on_hover_text("Drag to adjust · Shift for fine · Double-click to reset")
}

/// An on/off switch for a boolean parameter, drawn as a labelled pill.
pub fn toggle(ui: &mut Ui, setter: &ParamSetter, param: &BoolParam, label: &str, accent: Color32) -> Response {
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), TEXT);
    let size = vec2(galley.size().x + 52.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, !param.value());
        setter.end_set_parameter(param);
    }

    let on = param.value();
    let painter = ui.painter();
    let switch = Rect::from_min_size(rect.left_center() - vec2(0.0, 9.0), vec2(36.0, 18.0));
    painter.rect_filled(switch, 9.0, if on { accent } else { TRACK });
    let knob_x = if on { switch.right() - 9.0 } else { switch.left() + 9.0 };
    painter.circle_filled(pos2(knob_x, switch.center().y), 7.0, Color32::WHITE);
    painter.text(
        pos2(switch.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        if on { TEXT } else { TEXT_DIM },
    );
    if response.hovered() {
        painter.rect_stroke(rect.expand(2.0), 6.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Outside);
    }
    response
}

/// Vertical peak meter with a dB scale from -60 to +6.
pub fn meter(ui: &mut Ui, peak: f32, height: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(8.0, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, KNOB_BG);
    let db = util::gain_to_db(peak.max(1.0e-6));
    let frac = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
    if frac > 0.0 {
        let fill = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * frac), rect.max);
        let color = if db > -0.1 {
            Color32::from_rgb(230, 60, 50)
        } else if db > -9.0 {
            Color32::from_rgb(230, 190, 60)
        } else {
            Color32::from_rgb(80, 200, 120)
        };
        painter.rect_filled(fill, 2.0, color);
    }
    response.on_hover_text(format!("{db:.1} dBFS"))
}

/// Helper for stable per-module widget ids.
pub fn module_id(name: &str, suffix: &str) -> Id {
    Id::new(("guitar_rig_module", name, suffix))
}
