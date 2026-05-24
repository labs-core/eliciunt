use eframe::egui;
use egui::{Color32, Margin, RichText, Rounding, Stroke, Vec2};
use egui_plot::{Bar, BarChart, HLine, Line, Plot, PlotPoint, PlotPoints, Text, VLine};

use crate::constants::{BYTE_RANGE, PLOT_HEIGHT_PX, UNIFORM_SPIKE_RATIO};
use crate::export::{export_bar_chart_png, export_line_chart_png, save_png_via_dialog, ExportBookmark};
use crate::models::{AnalysisResult, HexBookmark};
use crate::padding::MultiRangeBookmark;
use crate::palette as pal;
use crate::ui::widgets::{card_frame, png_export_button, truncate_filename};

// ─────────────────────────────────────────────────────────────────────────────
// render_metric_plot
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_metric_plot(
    ui:              &mut egui::Ui,
    metric_name:     &str,
    _metric_unit:    &str,
    series:          &[[f64; 2]],
    anomaly_band:    Option<(f64, f64, f64)>,
    file_index:      usize,
    plot_id:         usize,
    bookmarks:       &[HexBookmark],
    multi_bookmarks: &[MultiRangeBookmark],
) -> Option<usize> {
    let (y_bound_lo, y_bound_hi) = if series.is_empty() {
        (0.0, 1.0)
    } else {
        let lo = series.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let hi = series.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let lo = anomaly_band.map(|(m, s, k)| lo.min(m - k * s)).unwrap_or(lo);
        let hi = anomaly_band.map(|(m, s, k)| hi.max(m + k * s)).unwrap_or(hi);
        (lo, hi)
    };

    // Snapshot bookmarks for the closure (egui requires 'static or move).
    let bm_snapshot: Vec<(f64, f64, Color32, String)> = bookmarks
        .iter()
        .map(|bm| {
            let (x0, x1) = bm.plot_x_range();
            (x0, x1, bm.color, bm.label.clone())
        })
        .collect();

    // Snapshot multi-range bookmark groups.  Each entry holds the group colour,
    // label, and the list of (x0, x1) sub-region pairs.
    let multi_snapshot: Vec<(Color32, String, Vec<(f64, f64)>)> = multi_bookmarks
        .iter()
        .map(|mbm| {
            let ranges = mbm.regions.iter()
                .map(|r| (r.start as f64, r.end() as f64))
                .collect();
            (mbm.color, mbm.label.clone(), ranges)
        })
        .collect();

    let plot_response = Plot::new(format!("metric_{file_index}_{plot_id}"))
        .height(PLOT_HEIGHT_PX)
        .show_axes([true, true])
        .show_grid([true, true])
        .include_y(y_bound_lo)
        .include_y(y_bound_hi)
        .auto_bounds([true, false].into())
        .set_margin_fraction(Vec2::new(0.02, 0.10))
        .x_axis_formatter(|mark, _, _| format!("0x{:X}", mark.value as usize))
        .y_axis_formatter(|mark, _, _| format!("{:.3}", mark.value))
        .label_formatter(move |label, point| {
            format!("offset: 0x{:X}\n{}: {:.4}", point.x as usize, label, point.y)
        })
        .show(ui, |plot_ui| {
            // ── bookmark background bands ─────────────────────────────────
            // Visual hierarchy goals:
            //   1. Main fill     – broad translucent region the eye can spot
            //                      at a glance without obscuring the data line.
            //   2. Edge accents  – narrow inner strips pressed against each
            //                      border wall; the higher-alpha "shadow" gives
            //                      the region a sense of depth and makes the
            //                      boundary crisp even when the region is thin.
            //   3. Solid borders – 2 px solid VLines read as hard walls rather
            //                      than the dashed "guide-line" look.
            //   4. Floating label – Text pinned at the data ceiling so the
            //                      bookmark name is always readable without
            //                      covering the line chart itself.

            for (x0, x1, color, label) in &bm_snapshot {
                let r = color.r();
                let g = color.g();
                let b = color.b();

                // ── colours at three opacity levels ──────────────────────
                // Fill: light  enough to see through, heavy enough to notice.
                let fill        = Color32::from_rgba_unmultiplied(r, g, b, 50);
                // Accent: inner edge strips – noticeably denser than the fill.
                let accent      = Color32::from_rgba_unmultiplied(r, g, b, 90);
                // Border: solid VLine walls – nearly opaque for hard edges.
                let border      = Color32::from_rgba_unmultiplied(r, g, b, 230);

                let y_lo = -1e15_f64;
                let y_hi =  1e15_f64;

                // ── 1. Main fill polygon ──────────────────────────────────
                plot_ui.polygon(
                    egui_plot::Polygon::new(PlotPoints::from(vec![
                        [*x0, y_lo], [*x1, y_lo], [*x1, y_hi], [*x0, y_hi],
                    ]))
                    .fill_color(fill)
                    .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                    // Only this primitive carries the legend name so the label
                    // appears exactly once in the legend, not once per VLine.
                    .name(label.as_str()),
                );

                // ── 2. Inner edge accent strips ───────────────────────────
                // Width is ~4 % of the bookmark span, clamped so it never
                // exceeds 1/4 of the span (important for narrow bookmarks).
                let span        = (x1 - x0).abs();
                let strip_w     = (span * 0.04).min(span * 0.25).max(1.0);

                plot_ui.polygon(
                    egui_plot::Polygon::new(PlotPoints::from(vec![
                        [*x0,           y_lo], [x0 + strip_w, y_lo],
                        [x0 + strip_w,  y_hi], [*x0,           y_hi],
                    ]))
                    .fill_color(accent)
                    .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                    .name(""),
                );
                plot_ui.polygon(
                    egui_plot::Polygon::new(PlotPoints::from(vec![
                        [x1 - strip_w,  y_lo], [*x1,           y_lo],
                        [*x1,           y_hi], [x1 - strip_w,  y_hi],
                    ]))
                    .fill_color(accent)
                    .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                    .name(""),
                );

                // ── 3. Solid border VLines ────────────────────────────────
                // Solid (no dash) + 2 px width reads as a hard region wall
                // rather than a data guide-line.  No legend name: the polygon
                // already registered the label above.
                plot_ui.vline(
                    VLine::new(*x0)
                        .color(border)
                        .width(2.0)
                        .name(""),
                );
                plot_ui.vline(
                    VLine::new(*x1)
                        .color(border)
                        .width(2.0)
                        .name(""),
                );

                // ── 4. Floating in-plot label ─────────────────────────────
                // Positioned at the data-space ceiling so it sits just below
                // the top axis edge and never overlaps the line chart itself.
                // Uses CENTER_TOP anchor so it is centred in the region and
                // drops downward from the ceiling.
                if !label.is_empty() {
                    let mid_x = (x0 + x1) / 2.0;
                    plot_ui.text(
                        Text::new(
                            PlotPoint::new(mid_x, y_bound_hi),
                            egui::RichText::new(label.as_str())
                                .size(10.0)
                                .strong()
                                .color(*color),
                        )
                        .anchor(egui::Align2::CENTER_TOP),
                    );
                }
            }

            // ── multi-range bookmark groups ───────────────────────────────
            // Each group is a set of sub-regions sharing one colour and label.
            // The fill/accent/border layers are identical to single-range bands,
            // but the floating label is drawn exactly once — above the first
            // sub-region — so the plot stays uncluttered regardless of how many
            // runs the group contains.
            for (color, label, ranges) in &multi_snapshot {
                let r = color.r();
                let g = color.g();
                let b = color.b();
                let fill   = Color32::from_rgba_unmultiplied(r, g, b, 50);
                let accent = Color32::from_rgba_unmultiplied(r, g, b, 90);
                let border = Color32::from_rgba_unmultiplied(r, g, b, 230);
                let y_lo = -1e15_f64;
                let y_hi =  1e15_f64;

                for (seg_idx, (x0, x1)) in ranges.iter().enumerate() {
                    let span   = (x1 - x0).abs();
                    let strip_w = (span * 0.04).min(span * 0.25).max(1.0);

                    // Main fill — name only on the first segment so the legend
                    // entry appears once for the whole group.
                    plot_ui.polygon(
                        egui_plot::Polygon::new(PlotPoints::from(vec![
                            [*x0, y_lo], [*x1, y_lo], [*x1, y_hi], [*x0, y_hi],
                        ]))
                        .fill_color(fill)
                        .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                        .name(if seg_idx == 0 { label.as_str() } else { "" }),
                    );

                    // Inner edge accent strips.
                    plot_ui.polygon(
                        egui_plot::Polygon::new(PlotPoints::from(vec![
                            [*x0,           y_lo], [x0 + strip_w, y_lo],
                            [x0 + strip_w,  y_hi], [*x0,          y_hi],
                        ]))
                        .fill_color(accent)
                        .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                        .name(""),
                    );
                    plot_ui.polygon(
                        egui_plot::Polygon::new(PlotPoints::from(vec![
                            [x1 - strip_w, y_lo], [*x1,          y_lo],
                            [*x1,          y_hi], [x1 - strip_w, y_hi],
                        ]))
                        .fill_color(accent)
                        .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                        .name(""),
                    );

                    // Solid border VLines.
                    plot_ui.vline(VLine::new(*x0).color(border).width(2.0).name(""));
                    plot_ui.vline(VLine::new(*x1).color(border).width(2.0).name(""));
                }

                // Label once, centred over the widest sub-region (most
                // visible placement regardless of run order).
                if !label.is_empty() {
                    let best = ranges.iter()
                        .max_by(|a, b| (a.1 - a.0).partial_cmp(&(b.1 - b.0)).unwrap_or(std::cmp::Ordering::Equal));
                    if let Some((x0, x1)) = best {
                        let mid_x = (x0 + x1) / 2.0;
                        plot_ui.text(
                            Text::new(
                                PlotPoint::new(mid_x, y_bound_hi),
                                egui::RichText::new(label.as_str())
                                    .size(10.0)
                                    .strong()
                                    .color(*color),
                            )
                            .anchor(egui::Align2::CENTER_TOP),
                        );
                    }
                }
            }

            // ── main metric line ──────────────────────────────────────────
            plot_ui.line(
                Line::new(PlotPoints::from(series.to_vec()))
                    .color(pal::RED)
                    .width(1.5)
                    .name(metric_name),
            );

            // ── anomaly band ──────────────────────────────────────────────
            if let Some((band_mean, band_sd, band_k)) = anomaly_band {
                plot_ui.hline(
                    HLine::new(band_mean)
                        .color(Color32::from_rgb(80, 80, 80))
                        .width(1.2)
                        .name("μ"),
                );
                for threshold_val in [band_mean + band_k * band_sd, band_mean - band_k * band_sd] {
                    plot_ui.hline(
                        HLine::new(threshold_val)
                            .color(Color32::from_rgb(160, 160, 160))
                            .width(1.0)
                            .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                            .name(if threshold_val > band_mean { "μ+kσ" } else { "μ−kσ" }),
                    );
                }
            }
        });

    if plot_response.response.hovered() {
        ui.input_mut(|input| {
            input.smooth_scroll_delta = Vec2::ZERO;
            input.raw_scroll_delta    = Vec2::ZERO;
        });
    }

    if plot_response.response.secondary_clicked() {
        if let Some(pointer_pos) = plot_response.response.interact_pointer_pos() {
            let plot_coords = plot_response.transform.value_from_position(pointer_pos);
            return Some(plot_coords.x.max(0.0) as usize);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// render_metric_row
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_metric_row(
    ui:              &mut egui::Ui,
    metric_name:     &str,
    metric_unit:     &str,
    column_entries:  &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
    column_width:    f32,
    row_id:          usize,
    bookmarks:       &[HexBookmark],
    multi_bookmarks: &[MultiRangeBookmark],
) -> Option<(usize, usize)> {
    let mut clicked_file_offset: Option<(usize, usize)> = None;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{metric_name}  ({metric_unit})"))
                    .strong().color(pal::TEXT).size(12.0),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("right-click → jump to hex")
                    .size(10.0).color(pal::MUTED).italics(),
            );

            // ── bookmark legend pills ─────────────────────────────────────
            if !bookmarks.is_empty() || !multi_bookmarks.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new("│").size(10.0).color(pal::BORDER));
                ui.add_space(4.0);
                for bm in bookmarks {
                    let pill_color = bm.color;
                    let (frame_fill, frame_stroke) = (
                        Color32::from_rgba_unmultiplied(
                            pill_color.r(), pill_color.g(), pill_color.b(), 30,
                        ),
                        Stroke::new(1.0, pill_color),
                    );
                    egui::Frame::none()
                        .fill(frame_fill)
                        .stroke(frame_stroke)
                        .rounding(Rounding::same(3.0))
                        .inner_margin(Margin::symmetric(4.0, 1.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(&bm.label)
                                    .size(9.5)
                                    .color(pill_color),
                            );
                        });
                    ui.add_space(2.0);
                }
                // Multi-range group pills — one pill per group, not per sub-region.
                for mbm in multi_bookmarks {
                    let pill_color = mbm.color;
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(
                            pill_color.r(), pill_color.g(), pill_color.b(), 30,
                        ))
                        .stroke(Stroke::new(1.0, pill_color))
                        .rounding(Rounding::same(3.0))
                        .inner_margin(Margin::symmetric(4.0, 1.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(&mbm.label)
                                    .size(9.5)
                                    .color(pill_color),
                            );
                        });
                    ui.add_space(2.0);
                }
            }
        });
        ui.add_space(4.0);

        let body_text_height = ui.text_style_height(&egui::TextStyle::Body);
        let row_height = PLOT_HEIGHT_PX
            + body_text_height
            + body_text_height
            + ui.spacing().item_spacing.y * 4.0;

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), row_height),
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui| {
                let col_count = column_entries.len();
                for (col_idx, (series, anomaly_band, file_idx, file_name)) in
                    column_entries.iter().enumerate()
                {
                    ui.vertical(|ui| {
                        ui.set_max_width(column_width);
                        ui.set_max_height(row_height);

                        if col_count > 1 {
                            let display_name = truncate_filename(file_name, 30);
                            ui.label(
                                RichText::new(display_name).size(10.0).color(pal::MUTED).italics(),
                            );
                        }

                        if let Some(byte_offset) = render_metric_plot(
                            ui,
                            metric_name,
                            metric_unit,
                            series,
                            *anomaly_band,
                            *file_idx,
                            row_id * 64 + col_idx,
                            bookmarks,
                            multi_bookmarks,
                        ) {
                            clicked_file_offset = Some((*file_idx, byte_offset));
                        }

                        ui.horizontal(|ui| {
                            if png_export_button(ui) {
                                let y_label = format!("{metric_name} ({metric_unit})");
                                // Single-range bookmarks.
                                let mut export_bookmarks: Vec<ExportBookmark> = bookmarks
                                    .iter()
                                    .map(|bm| {
                                        let (x0, x1) = bm.plot_x_range();
                                        ExportBookmark {
                                            x0,
                                            x1,
                                            color: (bm.color.r(), bm.color.g(), bm.color.b()),
                                            label: bm.label.clone(),
                                        }
                                    })
                                    .collect();
                                // Multi-range bookmarks (includes auto-padding):
                                // flatten each group's sub-regions and carry the
                                // group label only on the first sub-region so the
                                // legend entry appears once per group.
                                for mbm in multi_bookmarks.iter() {
                                    let c = (mbm.color.r(), mbm.color.g(), mbm.color.b());
                                    for (seg_idx, r) in mbm.regions.iter().enumerate() {
                                        export_bookmarks.push(ExportBookmark {
                                            x0:    r.start as f64,
                                            x1:    r.end()  as f64,
                                            color: c,
                                            label: if seg_idx == 0 { mbm.label.clone() } else { String::new() },
                                        });
                                    }
                                }
                                match export_line_chart_png(
                                    series,
                                    metric_name,
                                    "File offset (bytes)",
                                    &y_label,
                                    *anomaly_band,
                                    &export_bookmarks,
                                ) {
                                    Ok(png) => save_png_via_dialog(
                                        png,
                                        &format!(
                                            "{}_{file_idx}",
                                            metric_name.to_lowercase().replace(' ', "_")
                                        ),
                                    ),
                                    Err(e) => eprintln!("PNG export error: {e}"),
                                }
                            }
                        });
                    });

                    if col_idx + 1 < col_count {
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                    }
                }
            },
        );
    });
    clicked_file_offset
}

// ─────────────────────────────────────────────────────────────────────────────
// render_manual_bookmark_form
//
// Renders a compact "Add bookmark" form inside whatever panel the caller
// places it in (typically the bookmark sidebar / tab).  All mutable draft
// state (start, end, label, colour, validation error) is stored in egui's
// per-Id temporary data map so the form survives redraws without requiring
// any new fields on the app struct.
//
// Return value
//   Some(HexBookmark) – user clicked "Add" and the inputs are valid.
//                       The caller should push this into its bookmark list.
//   None              – form is still being edited, or was empty / invalid.
// ─────────────────────────────────────────────────────────────────────────────

/// Persistent draft state stored in egui's temp-data map.
#[derive(Clone, Default)]
struct BookmarkDraft {
    start_str: String,
    end_str:   String,
    label:     String,
    /// Stored as [r, g, b] so it fits in egui's Any map without extra derives.
    color_rgb: [u8; 3],
    error:     Option<String>,
    /// Cycles through a small palette so successive bookmarks get distinct
    /// colours without the user having to touch the colour picker each time.
    color_gen: u8,
}

impl BookmarkDraft {
    fn color(&self) -> Color32 {
        Color32::from_rgb(self.color_rgb[0], self.color_rgb[1], self.color_rgb[2])
    }

    /// Palette of visually distinct, saturated colours used for auto-cycling.
    fn next_auto_color(gen: u8) -> [u8; 3] {
        const PALETTE: [[u8; 3]; 16] = [
            [255, 160,  60],  // amber
            [ 80, 200, 120],  // mint
            [100, 160, 255],  // sky-blue
            [220,  80, 120],  // rose
            [180, 120, 255],  // violet
            [ 60, 210, 210],  // teal
            [255, 210,  60],  // gold
            [255, 110,  80],  // coral
            [255, 120, 180],  // pink
            [ 40, 200, 160],  // seafoam
            [160, 100, 240],  // purple
            [255, 200,  80],  // yellow-gold
            [100, 200, 255],  // sky
            [140, 220, 100],  // lime
            [200, 160, 120],  // tan
            [ 80, 180,  80],  // green
        ];
        PALETTE[(gen as usize) % PALETTE.len()]
    }
}

/// Parse a user-entered string as either a hex (`0x…` / `0X…`) or decimal
/// byte offset.  Returns `None` on any parse failure.
fn parse_offset(s: &str) -> Option<usize> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        usize::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse::<usize>().ok()
    }
}

pub fn render_manual_bookmark_form(
    ui:        &mut egui::Ui,
    file_len:  usize,          // upper bound for validation
) -> Option<HexBookmark> {
    // ── load (or initialise) draft state ─────────────────────────────────
    let form_id = egui::Id::new("manual_bookmark_draft");
    let mut draft: BookmarkDraft = ui
        .ctx()
        .data(|d| d.get_temp::<BookmarkDraft>(form_id).unwrap_or_default());

    // First-run: pick an initial colour from the palette.
    if draft.color_rgb == [0, 0, 0] {
        draft.color_rgb = BookmarkDraft::next_auto_color(draft.color_gen);
    }

    let mut result: Option<HexBookmark> = None;

    card_frame().show(ui, |ui| {
        // ── section header ────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Add Bookmark")
                    .strong()
                    .color(pal::TEXT)
                    .size(12.0),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(4.0);
            ui.label(
                RichText::new("hex (0x…) or decimal")
                    .size(10.0)
                    .color(pal::MUTED)
                    .italics(),
            );
        });
        ui.add_space(6.0);

        // ── start / end offset row ────────────────────────────────────────
        ui.horizontal(|ui| {
            // "Start" label + field
            ui.label(RichText::new("Start").size(11.0).color(pal::MUTED));
            ui.add_space(2.0);
            let start_resp = ui.add(
                egui::TextEdit::singleline(&mut draft.start_str)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("0x0000"),
            );
            if start_resp.changed() { draft.error = None; }

            ui.add_space(8.0);

            // "End" label + field
            ui.label(RichText::new("End").size(11.0).color(pal::MUTED));
            ui.add_space(2.0);
            let end_resp = ui.add(
                egui::TextEdit::singleline(&mut draft.end_str)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("0x00FF"),
            );
            if end_resp.changed() { draft.error = None; }
        });

        ui.add_space(4.0);

        // ── label + colour row ────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(RichText::new("Label").size(11.0).color(pal::MUTED));
            ui.add_space(2.0);
            let lbl_resp = ui.add(
                egui::TextEdit::singleline(&mut draft.label)
                    .desired_width(140.0)
                    .hint_text("optional name"),
            );
            if lbl_resp.changed() { draft.error = None; }
        });

        ui.add_space(4.0);

        // ── colour preset swatches ────────────────────────────────────────
        // Drawn inline (no popup) so they are always visible and clickable,
        // matching the behaviour of the hex-selection BookmarkDialog.
        const PRESETS: &[[u8; 3]] = &[
            [220,  80,  80],   // red
            [ 80, 180,  80],   // green
            [ 80, 140, 220],   // blue
            [220, 180,  50],   // yellow
            [200,  90, 200],   // purple
            [ 60, 200, 200],   // cyan
            [230, 140,  50],   // orange
            [180, 180, 180],   // grey
            [255, 120, 180],   // pink
            [ 40, 200, 160],   // teal
            [160, 100, 240],   // violet
            [255, 200,  80],   // gold
            [100, 200, 255],   // sky
            [255, 140, 100],   // coral
            [140, 220, 100],   // lime
            [200, 160, 120],   // tan
        ];

        ui.horizontal(|ui| {
            ui.label(RichText::new("Colour").size(11.0).color(pal::MUTED));
            ui.add_space(4.0);
            for preset in PRESETS {
                let preset_color = Color32::from_rgb(preset[0], preset[1], preset[2]);
                let selected     = draft.color_rgb == *preset;
                let swatch_size  = Vec2::splat(18.0);
                let (rect, resp) = ui.allocate_exact_size(swatch_size, egui::Sense::click());

                ui.painter().rect_filled(rect, Rounding::same(3.0), preset_color);
                ui.painter().rect_stroke(
                    rect,
                    Rounding::same(3.0),
                    Stroke::new(
                        if selected { 2.0 } else { 1.0 },
                        if selected { Color32::WHITE } else { Color32::from_rgba_unmultiplied(255, 255, 255, 60) },
                    ),
                );
                if resp.clicked() {
                    draft.color_rgb = *preset;
                    draft.error = None;
                }
            }
        });

        ui.add_space(6.0);

        // ── validation error ──────────────────────────────────────────────
        if let Some(ref err) = draft.error.clone() {
            ui.label(
                RichText::new(format!("⚠  {err}"))
                    .size(10.5)
                    .color(Color32::from_rgb(220, 100, 80)),
            );
            ui.add_space(4.0);
        }

        // ── Add button ────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            let add_btn = ui.add_sized(
                [80.0, 22.0],
                egui::Button::new(RichText::new("＋ Add").size(11.5).color(pal::TEXT))
                    .fill(Color32::from_rgba_unmultiplied(
                        draft.color_rgb[0],
                        draft.color_rgb[1],
                        draft.color_rgb[2],
                        40,
                    ))
                    .stroke(Stroke::new(1.0, draft.color())),
            );

            if add_btn.clicked() {
                // ── validate ──────────────────────────────────────────────
                let start_parsed = parse_offset(&draft.start_str);
                let end_parsed   = parse_offset(&draft.end_str);

                match (start_parsed, end_parsed) {
                    (None, _) => {
                        draft.error = Some("Invalid start offset.".into());
                    }
                    (_, None) => {
                        draft.error = Some("Invalid end offset.".into());
                    }
                    (Some(s), Some(e)) if s >= e => {
                        draft.error = Some("Start must be less than end.".into());
                    }
                    (Some(s), Some(_e)) if file_len > 0 && s >= file_len => {
                        draft.error = Some(format!(
                            "Start 0x{s:X} is past end of file (0x{file_len:X})."
                        ));
                    }
                    (Some(s), Some(e)) => {
                        // Clamp end to file length so the bookmark never
                        // overshoots the data; warn the user if we did.
                        let e_clamped = if file_len > 0 { e.min(file_len) } else { e };
                        if e_clamped < e {
                            draft.error = Some(format!(
                                "End clamped to file size (0x{e_clamped:X})."
                            ));
                        } else {
                            draft.error = None;
                        }

                        let len = e_clamped - s;
                        result = Some(HexBookmark {
                            start:  s,
                            len,
                            label:  draft.label.trim().to_string(),
                            color:  draft.color(),
                        });

                        // Reset form for the next bookmark; cycle colour.
                        let next_gen = draft.color_gen.wrapping_add(1);
                        draft = BookmarkDraft {
                            color_gen: next_gen,
                            color_rgb: BookmarkDraft::next_auto_color(next_gen),
                            ..Default::default()
                        };
                    }
                }
            }

            // "Clear" link on the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(
                        RichText::new("Clear").size(10.5).color(pal::MUTED),
                    ).frame(false))
                    .clicked()
                {
                    let next_gen = draft.color_gen.wrapping_add(1);
                    draft = BookmarkDraft {
                        color_gen: next_gen,
                        color_rgb: BookmarkDraft::next_auto_color(next_gen),
                        ..Default::default()
                    };
                }
            });
        });
    });

    // ── persist draft back into egui temp-data ────────────────────────────
    ui.ctx().data_mut(|d| d.insert_temp(form_id, draft));

    result
}

pub fn render_byte_distribution(
    ui:         &mut egui::Ui,
    result:     &AnalysisResult,
    file_name:  &str,
    file_index: usize,
) {
    let byte_counts   = &result.byte_counts;
    let total_bytes: usize = byte_counts.iter().sum();
    let uniform_level = total_bytes as f64 / BYTE_RANGE as f64;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Byte Frequency Distribution")
                    .strong().color(pal::TEXT).size(13.0),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display_name = truncate_filename(file_name, 40);
            ui.label(RichText::new(display_name).size(11.0).color(pal::MUTED).italics());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if png_export_button(ui) {
                    let chart_title = format!("Byte Frequency Distribution — {file_name}");
                    match export_bar_chart_png(byte_counts, &chart_title) {
                        Ok(png) => save_png_via_dialog(png, &format!("byte_dist_{file_index}")),
                        Err(e)  => eprintln!("PNG export error: {e}"),
                    }
                }
                ui.label(
                    RichText::new(format!(
                        "max occurrences = {}",
                        byte_counts.iter().cloned().max().unwrap_or(0)
                    ))
                    .size(11.0).color(pal::MUTED),
                );
            });
        });
        ui.add_space(6.0);

        let bars: Vec<Bar> = (0..BYTE_RANGE)
            .map(|byte_idx| {
                let count = byte_counts[byte_idx] as f64;
                Bar::new(byte_idx as f64, count)
                    .width(0.9)
                    .fill(if count > uniform_level * UNIFORM_SPIKE_RATIO {
                        pal::RED
                    } else {
                        pal::RED_MID
                    })
                    .stroke(Stroke::NONE)
                    .name(format!("0x{:02X}", byte_idx))
            })
            .collect();

        Plot::new(format!("byte_dist_{file_index}"))
            .height(220.0)
            .show_grid([true, true])
            .include_x(0.0)
            .include_x(255.0)
            .include_y(0.0)
            .auto_bounds([false, true].into())
            .x_axis_formatter(|mark, _, _| {
                let v = mark.value.round() as i64;
                if v >= 0 && v <= 255 && v % 32 == 0 {
                    format!("0x{:02X}", v as u8)
                } else {
                    String::new()
                }
            })
            .y_axis_formatter(|mark, _, _| {
                let v = mark.value as usize;
                if      v >= 1_000_000 { format!("{}M", v / 1_000_000) }
                else if v >= 1_000     { format!("{}k", v / 1_000) }
                else                   { format!("{v}") }
            })
            .label_formatter(move |label, point| {
                format!("byte: {}\noccurrences: {}", label, point.y as usize)
            })
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(BarChart::new(bars).color(pal::RED).name("count"));
                plot_ui.hline(
                    HLine::new(uniform_level)
                        .color(Color32::from_rgb(80, 80, 80))
                        .width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name("uniform"),
                );
            });
    });
}