/**
 * @file      ui/hex.rs
 * @brief     Virtualised hex-dump panel with bookmark and drag-selection support.
 * @details   Renders only the visible rows of a binary file using egui's
 *            show_rows, overlays bookmark bands and the active drag selection,
 *            and tracks raw pointer events to build multi-row byte selections
 *            that are returned to the caller on mouse release.
 *
 * @copyright  (C) Core Labs
 *             All rights reserved.
 *
 * @author     Manoel Serafim
 * @email      manoel.serafim@proton.me
 * @github     https://github.com/manoel-serafim
 * SPDX-License-Identifier: GPL-3.0
 */

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

    // ── Auto-scroll configuration ────────────────────────────────────────
    //
    // HOT_ZONE_PX  – the band (in pixels) near the top/bottom edge of the
    //                panel that triggers auto-scrolling.  The further inside
    //                the band the pointer travels, the faster the scroll.
    // MAX_SPEED    – scroll speed in pixels/second at full deflection.
    // MAX_DT       – frame-time cap so a single slow frame never causes a
    //                huge jump (e.g. when the window was occluded).
    const HOT_ZONE_PX: f32 = 50.0;
    // Base speed (px/s) at t = 1, i.e. exactly at the panel edge.
    // Kept modest because the quadratic curve multiplies it by t² — so
    // every 50 px further outside the panel doubles the exponent and the
    // perceived speed increases sharply.
    const MAX_SPEED:   f32 = 200.0;
    const MAX_DT:      f32 = 0.1;

    // Stable key for persisting the scroll offset across frames without
    // requiring changes to the caller.  egui's ScrollArea stores its own
    // internal offset, but we need to read it back so that auto-scroll
    // deltas are always applied relative to wherever the view actually is
    // (including manual wheel/scrollbar movement by the user).
    let offset_id = egui::Id::new("hex_scroll_offset");
    let last_offset: f32 = ui.ctx().data(|d| d.get_temp(offset_id).unwrap_or(0.0_f32));

    // Capture the panel rect *before* the ScrollArea consumes the layout
    // space — this is the viewport we test the pointer against.
    let panel_rect = ui.available_rect_before_wrap();
    let max_scroll = ((total_lines as f32 * row_height) - panel_rect.height()).max(0.0);

    // Determine whether we need to override the ScrollArea offset, and if
    // so, to what value.  Priority: scroll_to_byte > auto-scroll > nothing.
    let mut force_offset: Option<f32> = None;

    // Priority 1 – explicit scroll-to-byte request from the caller.
    if let Some(byte) = scroll_to_byte {
        let line = byte / HEX_COLUMNS;
        force_offset = Some((line.saturating_sub(3)) as f32 * row_height);
    }

    // Priority 2 – auto-scroll while the user is dragging a selection and
    // the pointer is inside the hot zone near an edge.
    if force_offset.is_none() && !dialog_open && selection.drag_start.is_some() {
        let (hover_pos, dt) = ui
            .ctx()
            .input(|i| (i.pointer.hover_pos(), i.unstable_dt.min(MAX_DT)));

        if let Some(pos) = hover_pos {
            // Compute a normalised [0, 1] deflection inside the hot zone and
            // apply a quadratic ramp so motion starts slow and accelerates
            // naturally as the pointer moves further out of the panel.
            let delta = if pos.y < panel_rect.min.y + HOT_ZONE_PX {
                // Pointer is above (or near) the top edge → scroll up.
                // No upper clamp: t keeps growing as the cursor moves further
                // above the panel, so the quadratic ramp produces a natural
                // "the further out, the faster" feel.
                let t = ((panel_rect.min.y + HOT_ZONE_PX - pos.y) / HOT_ZONE_PX)
                    .max(0.0);
                -(MAX_SPEED * t * t * dt)
            } else if pos.y > panel_rect.max.y - HOT_ZONE_PX {
                // Pointer is below (or near) the bottom edge → scroll down.
                let t = ((pos.y - (panel_rect.max.y - HOT_ZONE_PX)) / HOT_ZONE_PX)
                    .max(0.0);
                MAX_SPEED * t * t * dt
            } else {
                0.0
            };

            if delta.abs() > f32::EPSILON {
                force_offset = Some((last_offset + delta).clamp(0.0, max_scroll));
                // Keep egui ticking on every frame while the pointer is held
                // in the hot zone, even if no other input arrives.
                ui.ctx().request_repaint();
            }
        }
    }

    // ── Highlight ranges ─────────────────────────────────────────────────
    let hl_range: Option<std::ops::RangeInclusive<usize>> = highlight.map(|(s, l)| {
        (s / HEX_COLUMNS)..=((s + l).saturating_sub(1) / HEX_COLUMNS)
    });
    let drag_range: Option<std::ops::RangeInclusive<usize>> =
        selection.normalised().map(|(s, l)| {
            (s / HEX_COLUMNS)..=((s + l).saturating_sub(1) / HEX_COLUMNS)
        });

    let is_selecting = selection.drag_start.is_some();

    // ── Build the ScrollArea ─────────────────────────────────────────────
    let mut scroll_area = egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false])
        // Disable egui's built-in drag-to-scroll while the user is making a
        // byte selection so the two gestures don't fight each other.
        .drag_to_scroll(!is_selecting)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);

    if let Some(offset) = force_offset {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }

    let mut row_zero_abs_y: f32 = 0.0;

    let output = scroll_area.show_rows(
        ui,
        text_height_sans_spacing,
        total_lines,
        |ui, visible_line_range| {
            // Absolute Y coordinate of the top of row 0.  Recomputed every
            // frame because the scroll position may have changed.
            row_zero_abs_y = ui.cursor().min.y
                - visible_line_range.start as f32 * row_height;

            for line_idx in visible_line_range {
                let byte_start = line_idx * HEX_COLUMNS;
                let byte_end   = (byte_start + HEX_COLUMNS).min(file_data.len());
                let line_bytes = &file_data[byte_start..byte_end];

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

    // ── Persist the actual post-render scroll offset ─────────────────────
    //
    // Store whatever offset the ScrollArea settled on (which accounts for
    // wheel scrolling, scrollbar drags, and the overrides above) so that
    // the next frame's auto-scroll delta is always applied to the real
    // current position rather than a stale or estimated one.
    ui.ctx().data_mut(|d| d.insert_temp(offset_id, output.state.offset.y));

    let inner_rect = output.inner_rect;

    if !dialog_open {
        ui.input(|input| {
            let pointer = &input.pointer;

            // Convert an absolute screen position to the row index it falls
            // on, clamped to [0, total_lines − 1].
            //
            // `row_zero_abs_y` is updated every frame inside `show_rows`, so
            // this closure always uses the freshly scrolled coordinate.
            //
            // When the pointer is above the panel (rel_y < 0) the division
            // yields a negative f32; Rust 1.45+ saturates negative-to-usize
            // casts to 0, but we guard it explicitly for clarity.
            let y_to_line = |pos: egui::Pos2| -> usize {
                let rel_y = pos.y - row_zero_abs_y;
                if rel_y < 0.0 {
                    0
                } else {
                    ((rel_y / row_height).floor() as usize)
                        .min(total_lines.saturating_sub(1))
                }
            };

            let just_pressed  = pointer.button_pressed(PointerButton::Primary);
            let held          = pointer.button_down(PointerButton::Primary);
            let just_released = pointer.button_released(PointerButton::Primary);

            if just_pressed {
                if let Some(pos) = pointer.interact_pos() {
                    if inner_rect.contains(pos) {
                        let start = y_to_line(pos) * HEX_COLUMNS;
                        selection.drag_start = Some(start);
                        selection.drag_end   = Some(start);
                    }
                }
            }

            // Update drag_end every held frame.  When auto-scrolling, the
            // pointer stays outside the panel but `row_zero_abs_y` advances
            // with the view, so `y_to_line` naturally clamps to whichever
            // boundary row has just scrolled into view.
            if held && selection.drag_start.is_some() {
                if let Some(pos) = pointer.hover_pos() {
                    let line     = y_to_line(pos);
                    let byte_end = ((line + 1) * HEX_COLUMNS)
                        .min(file_data.len())
                        .saturating_sub(1);
                    selection.drag_end = Some(byte_end);
                }
            }

            if just_released && selection.drag_start.is_some() {
                if let Some(norm) = selection.normalised() {
                    finished_selection = Some(norm);
                }
            }
        });
    }

    finished_selection
}