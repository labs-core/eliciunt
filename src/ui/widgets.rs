use eframe::egui;
use egui::{Frame, Margin, RichText, Rounding, Stroke, Vec2};

use crate::palette as pal;

pub fn card_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(14.0),
        outer_margin: Margin::symmetric(0.0, 4.0),
        rounding:     Rounding::same(6.0),
        fill:         pal::PANEL,
        stroke:       Stroke::new(1.0, pal::BORDER),
        ..Default::default()
    }
}

pub fn png_export_button(ui: &mut egui::Ui) -> bool {
    ui.add(
        egui::Button::new(RichText::new("⬇ PNG").size(10.0).color(pal::RED))
            .fill(pal::RED_FAINT)
            .stroke(Stroke::new(1.0, pal::RED_MID))
            .rounding(Rounding::same(3.0))
            .min_size(Vec2::new(44.0, 16.0)),
    )
    .clicked()
}

pub fn truncate_filename(name: &str, max_chars: usize) -> String {
    if name.len() > max_chars {
        format!("…{}", &name[name.len().saturating_sub(max_chars - 3)..])
    } else {
        name.to_owned()
    }
}
