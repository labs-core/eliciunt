use eframe::egui;
use egui::{RichText, Rounding, Stroke, Vec2};

use crate::constants::HEX_COLUMNS;
use crate::palette as pal;

pub fn render_hex_view(
    ui:             &mut egui::Ui,
    file_data:      &[u8],
    scroll_to_byte: Option<usize>,
    highlight:      Option<(usize, usize)>,
) {
    let total_lines = (file_data.len() + HEX_COLUMNS - 1) / HEX_COLUMNS;

    // show_rows expects the height of one row WITHOUT item_spacing (it adds
    // spacing internally, computing row_height_with_spacing = sans + spacing.y).
    // We must use the WITH-spacing value for the pixel offset so both match.
    let text_height_sans_spacing = ui.text_style_height(&egui::TextStyle::Monospace);
    let spacing_y                = ui.spacing().item_spacing.y;
    let row_height_with_spacing  = text_height_sans_spacing + spacing_y;

    let target_offset_y: Option<f32> = scroll_to_byte.map(|byte_offset| {
        let target_line = byte_offset / HEX_COLUMNS;
        // Subtract 3 lines so the target lands a few rows from the top.
        (target_line.saturating_sub(3)) as f32 * row_height_with_spacing
    });

    let highlighted_line_range: Option<std::ops::RangeInclusive<usize>> =
        highlight.map(|(start_byte, byte_len)| {
            let first_line = start_byte / HEX_COLUMNS;
            let last_line  = start_byte.saturating_add(byte_len).saturating_sub(1) / HEX_COLUMNS;
            first_line..=last_line
        });

    // Build the scroll area.  vertical_scroll_offset is applied by egui INSIDE
    // show_viewport_dyn, after loading persisted state but before clamping to
    // content bounds — so it always overrides any previous user scroll position.
    let mut scroll_area = egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false]);

    if let Some(offset_y) = target_offset_y {
        scroll_area = scroll_area.vertical_scroll_offset(offset_y);
    }

    // Pass text_height_sans_spacing so show_rows' internal arithmetic
    // (row_height_with_spacing = sans + spacing.y) is correct.
    scroll_area.show_rows(ui, text_height_sans_spacing, total_lines, |ui, visible_line_range| {
        for line_idx in visible_line_range {
            let byte_start = line_idx * HEX_COLUMNS;
            let byte_end   = (byte_start + HEX_COLUMNS).min(file_data.len());
            let line_bytes = &file_data[byte_start..byte_end];

            let mut hex_segment   = format!("{:08X}  ", byte_start);
            let mut ascii_segment = String::with_capacity(HEX_COLUMNS);

            for (col_idx, byte_val) in line_bytes.iter().enumerate() {
                if col_idx == 8 { hex_segment.push(' '); }
                hex_segment.push_str(&format!("{:02X} ", byte_val));
                ascii_segment.push(
                    if byte_val.is_ascii_graphic() || *byte_val == b' ' {
                        *byte_val as char
                    } else {
                        '·'
                    },
                );
            }

            let missing_cols = HEX_COLUMNS - line_bytes.len();
            for pad_col in 0..missing_cols {
                if line_bytes.len() + pad_col == 8 { hex_segment.push(' '); }
                hex_segment.push_str("   ");
            }
            hex_segment.push_str(&format!(" │ {}", ascii_segment));

            const HEX_ROW_RENDER_WIDTH: f32 = 720.0;
            let is_highlighted = highlighted_line_range
                .as_ref()
                .map_or(false, |range| range.contains(&line_idx));

            if is_highlighted {
                let desired_size  = Vec2::new(HEX_ROW_RENDER_WIDTH, text_height_sans_spacing);
                let (row_rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
                ui.painter().rect(
                    row_rect.expand(1.0),
                    Rounding::same(2.0),
                    pal::HL_BG,
                    Stroke::new(1.0, pal::HL_BORDER),
                );
                ui.painter().text(
                    row_rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &hex_segment,
                    egui::FontId::monospace(12.0),
                    pal::HL_BORDER,
                );
            } else {
                ui.label(
                    RichText::new(&hex_segment)
                        .monospace()
                        .size(12.0)
                        .color(pal::TEXT),
                );
            }
        }
    });
}