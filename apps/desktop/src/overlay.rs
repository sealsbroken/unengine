//! Drawing the table over the camera picture, as on iOS: a box around every toy (white core, dark
//! halo, so it reads on any table) and its stable number badge, white until counted, yellow and
//! larger while the tutor counts it, blue once counted, green when the count was right.

use egui::{Align2, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use un_core::{Box2, TableState, TrackedDetection};

use crate::un::{overlay_colors as oc, rounded};

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

#[derive(Clone, Copy, PartialEq)]
pub enum Badge {
    Idle,
    Lit,
    Counted,
    Correct,
}

fn badge_state(number: u32, highlight: Option<u32>, correct: bool) -> Badge {
    if correct {
        return Badge::Correct;
    }
    match highlight {
        Some(h) if number < h => Badge::Counted,
        Some(h) if number == h => Badge::Lit,
        _ => Badge::Idle,
    }
}

/// The toys: boxes for every detection, badges for the numbered (tracked) ones.
pub fn draw_toys(
    painter: &egui::Painter,
    map: &Mapper,
    state: &TableState,
    tracked: &[TrackedDetection],
    highlight: Option<u32>,
    correct: bool,
) {
    let badge_size = (map.rect.width() * 0.052).clamp(46.0, 78.0);
    let numbered = |b: &Box2| tracked.iter().find(|t| t.detection.bbox == *b).map(|t| t.number).unwrap_or(0);
    for d in &state.detections {
        let rect = map.box_rect(&d.bbox);
        let number = numbered(&d.bbox);
        let state = if number > 0 { badge_state(number, highlight, correct) } else { Badge::Idle };
        let (stroke, width) = match state {
            Badge::Lit => (oc::BADGE_LIT, 5.0),
            Badge::Correct => (oc::CORRECT_STROKE, 5.0),
            _ => (Color32::WHITE, 3.0),
        };
        let radius = (rect.width().min(rect.height()) / 5.0).min(22.0);
        painter.rect_stroke(rect, radius, Stroke::new(width + 3.0, oc::halo()), StrokeKind::Middle);
        painter.rect_stroke(rect, radius, Stroke::new(width, stroke), StrokeKind::Middle);
        if number > 0 {
            badge(painter, rect.min + Vec2::splat(6.0), number, state, badge_size);
        }
    }
}

/// One number badge centred on `center`.
pub fn badge(painter: &egui::Painter, center: Pos2, number: u32, state: Badge, size: f32) {
    let scale = match state {
        Badge::Lit => 1.25,
        Badge::Correct => 1.1,
        _ => 1.0,
    };
    let r = size * scale / 2.0;
    let (fill, text) = match state {
        Badge::Idle => (Color32::WHITE, oc::INK),
        Badge::Lit => (oc::BADGE_LIT, Color32::BLACK),
        Badge::Counted => (oc::BADGE_COUNTED, Color32::WHITE),
        Badge::Correct => (oc::BADGE_CORRECT, Color32::WHITE),
    };
    match state {
        Badge::Lit => {
            painter.circle_filled(center, r + 12.0, oc::BADGE_LIT.gamma_multiply(0.45));
        }
        Badge::Correct => {
            painter.circle_filled(center, r + 10.0, oc::CORRECT_STROKE.gamma_multiply(0.35));
        }
        _ => {}
    }
    painter.circle_filled(center + Vec2::new(0.0, 4.0), r + 2.0, Color32::from_black_alpha(60));
    painter.circle_filled(center, r + 1.5, oc::halo());
    let ring = if matches!(state, Badge::Idle | Badge::Counted) { Color32::WHITE } else { fill };
    painter.circle_filled(center, r, ring);
    painter.circle_filled(center, r - 4.0, fill);
    painter.text(center, Align2::CENTER_CENTER, number.to_string(), rounded(size * scale * 0.56), text);
}

/// The table area: a dashed white frame (the simulated table and the area editor).
pub fn dashed_rect(painter: &egui::Painter, rect: Rect, color: Color32) {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    for w in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(2.0, color), 8.0, 6.0));
    }
}

/// A dashed outline with rounded corners (the "Add a child" tile).
pub fn dashed_rounded(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32) {
    let mut points = vec![];
    let corners = [
        (rect.right_top() + Vec2::new(-radius, radius), -90.0f32),
        (rect.right_bottom() + Vec2::new(-radius, -radius), 0.0),
        (rect.left_bottom() + Vec2::new(radius, -radius), 90.0),
        (rect.left_top() + Vec2::new(radius, radius), 180.0),
    ];
    for (center, start) in corners {
        for k in 0..=8 {
            let a = (start + 90.0 * k as f32 / 8.0).to_radians();
            points.push(center + Vec2::new(a.cos(), a.sin()) * radius);
        }
    }
    points.push(points[0]);
    painter.extend(egui::Shape::dashed_line(&points, Stroke::new(1.5, color), 6.0, 5.0));
}
