use std::f32::consts::TAU;

use egui::{Color32, Shape, Stroke, Vec2};

use crate::theme::{CYAN, CYAN_SOFT, ORANGE};

/// Godot ArcReactor 装饰：同心圆 + 旋转叶片
pub fn show(ui: &mut egui::Ui, time: f64) -> egui::Response {
    let size = Vec2::new(104.0, 104.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let center = rect.center();
    let r = rect.width() * 0.42;
    let t = time as f32;
    let painter = ui.painter_at(rect);

    // 外环光晕
    for i in 0..4 {
        let glow_r = r + 8.0 + i as f32 * 4.0;
        let a = (0.06 - i as f32 * 0.012).max(0.02);
        painter.circle_stroke(
            center,
            glow_r,
            Stroke::new(2.0, Color32::from_rgba_premultiplied(0, 240, 255, (a * 255.0) as u8)),
        );
    }

    painter.circle_stroke(center, r, Stroke::new(3.0, CYAN));
    painter.circle_stroke(center, r * 0.72, Stroke::new(2.0, CYAN_SOFT));
    painter.circle_stroke(center, r * 0.38, Stroke::new(2.0, ORANGE));

    // 旋转叶片
    let blades = 8;
    for i in 0..blades {
        let angle = t * 1.8 + (i as f32 / blades as f32) * TAU;
        let inner = r * 0.42;
        let outer = r * 0.88;
        let dir = Vec2::angled(angle);
        let p0 = center + dir * inner;
        let p1 = center + dir * outer;
        painter.line_segment([p0, p1], Stroke::new(3.0, Color32::from_rgba_premultiplied(0, 220, 240, 200)));
    }

    // 中心核
    let pulse = 0.85 + (t * 3.0).sin() * 0.15;
    painter.circle_filled(center, r * 0.18 * pulse, ORANGE);
    painter.circle_filled(
        center,
        r * 0.10,
        Color32::from_rgba_premultiplied(255, 240, 200, 255),
    );

    // 三角形标记
    let tri_r = r * 0.55;
    let mut points = Vec::new();
    for i in 0..3 {
        let a = t * 0.5 + (i as f32 / 3.0) * TAU - std::f32::consts::FRAC_PI_2;
        points.push(center + Vec2::angled(a) * tri_r);
    }
    painter.add(Shape::closed_line(
        points,
        Stroke::new(1.5, Color32::from_rgba_premultiplied(0, 240, 255, 120)),
    ));

    response
}
