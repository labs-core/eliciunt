use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke, Vec2};
use egui_plot::{Bar, BarChart, HLine, Line, Plot, PlotPoints, VLine};

use crate::constants::{BYTE_RANGE, PLOT_HEIGHT_PX, UNIFORM_SPIKE_RATIO};
use crate::export::{export_bar_chart_png, export_line_chart_png, save_png_via_dialog};
use crate::models::{AnalysisResult, HexBookmark};
use crate::palette as pal;
use crate::ui::widgets::{card_frame, png_export_button, truncate_filename};

// ─────────────────────────────────────────────────────────────────────────────
// render_metric_plot
// ─────────────────────────────────────────────────────────────────────────────

pub fn render_metric_plot(
    ui:            &mut egui::Ui,
    metric_name:   &str,
    _metric_unit:  &str,
    series:        &[[f64; 2]],
    anomaly_band:  Option<(f64, f64, f64)>,
    file_index:    usize,
    plot_id:       usize,
    bookmarks:     &[HexBookmark],           // ← NEW
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
            // egui_plot doesn't have a native filled-rect primitive, so we
            // approximate a band with a pair of VLines plus a transparent
            // polygon drawn via the custom painting callback trick: we store
            // the screen transform and paint afterwards.  The simplest approach
            // that works with egui_plot 0.27-style API is to draw two
            // VLines with a thick alpha-blended stroke.  For a true filled
            // rect we need to use `plot_ui.ctx()` + `painter` after the show
            // closure; the approach below uses semi-transparent vertical lines
            // spaced 1 px apart in plot-space to fill the band.
            //
            // For a pixel-perfect fill we instead take the approach of drawing
            // an opaque `Polygon` using `plot_ui.polygon()` which is stable in
            // egui_plot >= 0.26.

            for (x0, x1, color, label) in &bm_snapshot {
                // Filled polygon spanning the full y-axis visible range.
                // We use a very tall rect so it always covers the plot area
                // regardless of zoom.
                let fill_color = Color32::from_rgba_unmultiplied(
                    color.r(), color.g(), color.b(), 35,
                );
                let border_color = Color32::from_rgba_unmultiplied(
                    color.r(), color.g(), color.b(), 160,
                );

                // Four corners at an extreme y range.
                let y_lo = -1e15_f64;
                let y_hi =  1e15_f64;
                let polygon = egui_plot::Polygon::new(PlotPoints::from(vec![
                    [*x0, y_lo],
                    [*x1, y_lo],
                    [*x1, y_hi],
                    [*x0, y_hi],
                ]))
                .fill_color(fill_color)
                .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                .name(label.as_str());
                plot_ui.polygon(polygon);

                // Left and right border VLines.
                plot_ui.vline(
                    VLine::new(*x0)
                        .color(border_color)
                        .width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name(label.as_str()),
                );
                plot_ui.vline(
                    VLine::new(*x1)
                        .color(border_color)
                        .width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name(""),
                );
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
    ui:             &mut egui::Ui,
    metric_name:    &str,
    metric_unit:    &str,
    column_entries: &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
    column_width:   f32,
    row_id:         usize,
    bookmarks:      &[HexBookmark],           // ← NEW
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
            if !bookmarks.is_empty() {
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
                            bookmarks,       // ← forwarded
                        ) {
                            clicked_file_offset = Some((*file_idx, byte_offset));
                        }

                        ui.horizontal(|ui| {
                            if png_export_button(ui) {
                                let y_label = format!("{metric_name} ({metric_unit})");
                                match export_line_chart_png(
                                    series,
                                    metric_name,
                                    "File offset (bytes)",
                                    &y_label,
                                    *anomaly_band,
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
// render_byte_distribution  (unchanged, kept here for completeness)
// ─────────────────────────────────────────────────────────────────────────────

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