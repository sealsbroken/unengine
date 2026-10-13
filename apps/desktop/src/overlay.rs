//! Drawing the table state over the camera picture.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use un_core::hands::CONNECTIONS;
use un_core::{Box2, HandObservation, TableState, TrackedDetection};

pub const GREEN: Color32 = Color32::from_rgb(90, 200, 90);
pub const AMBER: Color32 = Color32::from_rgb(240, 190, 60);
pub const RED: Color32 = Color32::from_rgb(230, 80, 80);
pub const BLUE: Color32 = Color32::from_rgb(90, 160, 255);
pub const WHITE: Color32 = Color32::WHITE;

/// Maps normalized frame coordinates onto the rectangle the picture occupies.
#[derive(Clone, Copy)]
pub struct Mapper {
    pub rect: Rect,
}

impl Mapper {
    pub fn to_screen(self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.rect.min.x + x * self.rect.width(), self.rect.min.y + y * self.rect.height())
    }

    pub fn to_norm(self, pos: Pos2) -> (f32, f32) {
        (
            ((pos.x - self.rect.min.x) / self.rect.width()).clamp(0.0, 1.0),
            ((pos.y - self.rect.min.y) / self.rect.height()).clamp(0.0, 1.0),
        )
    }

    pub fn box_rect(&self, b: &Box2) -> Rect {
        Rect::from_min_max(self.to_screen(b.x1, b.y1), self.to_screen(b.x2, b.y2))
    }
}

pub fn draw_roi(painter: &egui::Painter, map: &Mapper, roi: Option<Box2>, drag: Option<Box2>) {
    if let Some(r) = roi {
        painter.rect_stroke(map.box_rect(&r), 4.0, Stroke::new(2.0, AMBER), egui::StrokeKind::Outside);
    }
    if let Some(d) = drag {
        painter.rect_stroke(map.box_rect(&d), 2.0, Stroke::new(2.0, WHITE), egui::StrokeKind::Outside);
    }
}

pub fn draw_detections(
    painter: &egui::Painter,
    map: &Mapper,
    state: &TableState,
    tracked: &[TrackedDetection],
    highlight: Option<u32>,
) {
    let color = if state.hands_present {
        AMBER
    } else if state.is_stable {
        GREEN
    } else {
        WHITE
    };
    for d in &state.detections {
        let rect = map.box_rect(&d.bbox);
        painter.rect_stroke(rect, 3.0, Stroke::new(2.0, color), egui::StrokeKind::Outside);
        if let Some(label) = &d.label {
            painter.text(rect.min + Vec2::new(4.0, -2.0), Align2::LEFT_BOTTOM, label, FontId::proportional(14.0), color);
        }
    }
    for t in tracked {
        let rect = map.box_rect(&t.detection.bbox);
        let center = rect.center();
        let lit = highlight.is_some_and(|n| t.number <= n);
        let fill = if lit { RED } else { Color32::from_black_alpha(160) };
        painter.circle_filled(center, if lit { 22.0 } else { 16.0 }, fill);
        painter.text(
            center,
            Align2::CENTER_CENTER,
            t.number.to_string(),
            FontId::proportional(if lit { 26.0 } else { 18.0 }),
            WHITE,
        );
    }
}

pub fn draw_hands(painter: &egui::Painter, map: &Mapper, hands: &[HandObservation]) {
    for hand in hands {
        for (a, b) in CONNECTIONS {
            if let (Some(pa), Some(pb)) = (hand.joints.get(&a), hand.joints.get(&b)) {
                painter.line_segment([map.to_screen(pa.0, pa.1), map.to_screen(pb.0, pb.1)], Stroke::new(2.0, BLUE));
            }
        }
        for p in hand.joints.values() {
            painter.circle_filled(map.to_screen(p.0, p.1), 3.0, WHITE);
        }
        if let (Some(b), Some(fingers)) = (hand.bbox, hand.raised_finger_count) {
            painter.text(
                map.to_screen(b.x1, b.y1) + Vec2::new(0.0, -6.0),
                Align2::LEFT_BOTTOM,
                format!("{:?} {fingers}", hand.chirality),
                FontId::proportional(16.0),
                BLUE,
            );
        }
    }
}

/// A rounded panel with text, used for the tutor bubble and the summary card.
pub fn bubble(painter: &egui::Painter, rect: Rect, text: &str, size: f32, fill: Color32) {
    painter.rect_filled(rect, 14.0, fill);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(size), WHITE);
}
