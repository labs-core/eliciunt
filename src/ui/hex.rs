use eframe::egui;
use egui::{Color32, PointerButton, Rect, Rounding, Sense, Stroke, Vec2};

use crate::constants::HEX_COLUMNS;
use crate::models::{HexBookmark, HexSelectionState};
use crate::palette as pal;

pub fn render_hex_view(
    ui:             &mut egui::Ui,
    file_data:      &[u8],
    scroll_to_byte: Option<usize>,
    highlight:      Option<(usize, usize)>,
    bookmarks:      &[HexBookmark],
    selection:      &mut HexSelectionState,
    dialog_open:    bool,
) -> Option<(usize, usize)> {
    let mut finished_selection: Option<(usize, usize)> = None;

    let total_lines              = (file_data.len() + HEX_COLUMNS - 1) / HEX_COLUMNS;
    let text_height_sans_spacing = ui.text_style_height(&egui::TextStyle::Monospace);
    let spacing_y                = ui.spacing().item_spacing.y;
    let row_height               = text_height_sans_spacing + spacing_y;

    let target_offset_y: Option<f32> = scroll_to_byte.map(|b| {
        let line = b / HEX_COLUMNS;
        (line.saturating_sub(3)) as f32 * row_height
    });

    let hl_range: Option<std::ops::RangeInclusive<usize>> = highlight.map(|(s, l)| {
        (s / HEX_COLUMNS)..=((s + l).saturating_sub(1) / HEX_COLUMNS)
    });
    let drag_range: Option<std::ops::RangeInclusive<usize>> =
        selection.normalised().map(|(s, l)| {
            (s / HEX_COLUMNS)..=((s + l).saturating_sub(1) / HEX_COLUMNS)
        });

    // Disable the scroll area's own drag-to-scroll while we own the pointer
    // for a selection, so the two gestures don't fight each other.
    let is_selecting = selection.drag_start.is_some();

    let mut scroll_area = egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false])
        .drag_to_scroll(!is_selecting)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);

    if let Some(offset_y) = target_offset_y {
        scroll_area = scroll_area.vertical_scroll_offset(offset_y);
    }

    let mut row_zero_abs_y: f32 = 0.0;

    let output = scroll_area.show_rows(
        ui,
        text_height_sans_spacing,
        total_lines,
        |ui, visible_line_range| {
            row_zero_abs_y = ui.cursor().min.y
                - visible_line_range.start as f32 * row_height;

            for line_idx in visible_line_range {
                let byte_start = line_idx * HEX_COLUMNS;
                let byte_end   = (byte_start + HEX_COLUMNS).min(file_data.len());
                let line_bytes = &file_data[byte_start..byte_end];

                // ── hex text ──────────────────────────────────────────────
                let mut hex = format!("{:08X}  ", byte_start);
                let mut asc = String::with_capacity(HEX_COLUMNS);
                for (i, b) in line_bytes.iter().enumerate() {
                    if i == 8 { hex.push(' '); }
                    hex.push_str(&format!("{:02X} ", b));
                    asc.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '·' });
                }
                let missing = HEX_COLUMNS - line_bytes.len();
                for p in 0..missing {
                    if line_bytes.len() + p == 8 { hex.push(' '); }
                    hex.push_str("   ");
                }
                hex.push_str(&format!(" │ {}", asc));

                const ROW_W: f32 = 720.0;
                let (row_rect, _) =
                    ui.allocate_exact_size(Vec2::new(ROW_W, text_height_sans_spacing), Sense::hover());

                let is_hl  = hl_range.as_ref().map_or(false, |r| r.contains(&line_idx));
                let is_sel = drag_range.as_ref().map_or(false, |r| r.contains(&line_idx));
                let bm     = bookmarks.iter().find(|bm| {
                    line_idx >= bm.start / HEX_COLUMNS && line_idx <= bm.end() / HEX_COLUMNS
                });

                let p = ui.painter();

                if let Some(bm) = bm {
                    let fill   = Color32::from_rgba_unmultiplied(bm.color.r(), bm.color.g(), bm.color.b(), 40);
                    let border = Color32::from_rgba_unmultiplied(bm.color.r(), bm.color.g(), bm.color.b(), 130);
                    p.rect(row_rect.expand2(Vec2::new(2.0, 0.5)), Rounding::same(2.0), fill, Stroke::new(1.0, border));

                    if line_idx == bm.start / HEX_COLUMNS && !bm.label.is_empty() {
                        let galley = ui.fonts(|f| {
                            f.layout_no_wrap(bm.label.clone(), egui::FontId::proportional(9.5), bm.color)
                        });
                        let pill = Rect::from_min_size(
                            row_rect.right_top() + Vec2::new(-galley.size().x - 8.0, 1.0),
                            galley.size() + Vec2::new(6.0, 2.0),
                        );
                        p.rect(pill, Rounding::same(3.0),
                            Color32::from_rgba_unmultiplied(bm.color.r(), bm.color.g(), bm.color.b(), 30),
                            Stroke::new(1.0, bm.color));
                        p.galley(pill.min + Vec2::new(3.0, 1.0), galley, bm.color);
                    }
                }

                if is_sel {
                    p.rect(row_rect.expand(1.0), Rounding::same(2.0),
                        Color32::from_rgba_unmultiplied(80, 140, 220, 55),
                        Stroke::new(1.0, Color32::from_rgb(80, 140, 220)));
                }
                if is_hl {
                    p.rect(row_rect.expand(1.0), Rounding::same(2.0),
                        pal::HL_BG, Stroke::new(1.0, pal::HL_BORDER));
                }

                let text_color = if is_hl {
                    pal::HL_BORDER
                } else if is_sel {
                    Color32::from_rgb(180, 210, 255)
                } else if let Some(bm) = bm {
                    Color32::from_rgba_unmultiplied(
                        bm.color.r().saturating_add(20),
                        bm.color.g().saturating_add(20),
                        bm.color.b().saturating_add(20),
                        255,
                    )
                } else {
                    pal::TEXT
                };

                p.text(row_rect.left_center(), egui::Align2::LEFT_CENTER,
                    &hex, egui::FontId::monospace(12.0), text_color);
            }
        },
    );

    // ── Raw-pointer drag tracking ─────────────────────────────────────────
    //
    // We read pointer state directly from ui.input() so a single gesture
    // spans as many rows as needed.  We guard against the dialog being open
    // so that clicking a colour swatch doesn't start a new selection.
    let inner_rect = output.inner_rect;

    if !dialog_open {
        ui.input(|input| {
            let pointer = &input.pointer;

            let y_to_line = |pos: egui::Pos2| -> usize {
                let rel_y = pos.y - row_zero_abs_y;
                ((rel_y / row_height).floor() as usize).min(total_lines.saturating_sub(1))
            };

            let just_pressed  = pointer.button_pressed(PointerButton::Primary);
            let held          = pointer.button_down(PointerButton::Primary);
            let just_released = pointer.button_released(PointerButton::Primary);

            // Start selection only when the click lands inside the hex panel.
            if just_pressed {
                if let Some(pos) = pointer.interact_pos() {
                    if inner_rect.contains(pos) {
                        let start = y_to_line(pos) * HEX_COLUMNS;
                        selection.drag_start = Some(start);
                        selection.drag_end   = Some(start);
                    }
                }
            }

            // Update end every frame while button is held, even outside panel.
            if held && selection.drag_start.is_some() {
                if let Some(pos) = pointer.hover_pos() {
                    let line     = y_to_line(pos);
                    let byte_end = ((line + 1) * HEX_COLUMNS)
                        .min(file_data.len())
                        .saturating_sub(1);
                    selection.drag_end = Some(byte_end);
                }
            }

            // Finalise on release.
            if just_released && selection.drag_start.is_some() {
                if let Some(norm) = selection.normalised() {
                    finished_selection = Some(norm);
                }
                // Keep drag_start set so blue band persists until dialog clears it.
            }
        });
    }

    finished_selection
}