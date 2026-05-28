/**
 * @file      export.rs
 * @brief     PNG chart export via the Plotters backend.
 * @details   Renders line charts and byte-distribution bar charts to in-memory
 *            PNG byte vectors.  Line charts display a filled area under the
 *            curve, a solid mean reference line, and a shaded sigma band
 *            (instead of bare dashed lines) so anomalous regions are
 *            immediately visible at a glance.  Chi-squared series are stored
 *            as reduced chi-squared (chi2/df) so the y = 1.0 gridline marks
 *            the uniform-random baseline.
 *
 *            Publication-quality export settings:
 *              • 3200 × 1800 px canvas with 300 DPI pHYs metadata embedded
 *                → imports at correct physical size in Word / LaTeX / Illustrator
 *              • All text scaled for legibility when the figure is reduced to
 *                a single journal column (~88 mm wide at 300 DPI)
 *              • Tufte-style grid: heavy lines only, no light sub-grid noise
 *
 * @copyright  (C) Core Labs
 *             All rights reserved.
 *
 * @author     Manoel Serafim
 * @email      manoel.serafim@proton.me
 * @github     https://github.com/manoel-serafim
 * SPDX-License-Identifier: GPL-3.0
 */

use std::fs;
use plotters::prelude::*;
use rfd::FileDialog;

use crate::constants::{BYTE_RANGE, UNIFORM_SPIKE_RATIO};

// ── Publication-quality canvas dimensions ────────────────────────────────────
// 3200 × 1800 @ 300 DPI  →  ~27 × 15 cm physical size.
// Scale down in your paper to a single column (~8.8 cm) and the result is
// ~300 DPI, which is the standard minimum for raster figures in IEEE / Elsevier
// / Nature journals.  The constants from the app (PNG_CHART_WIDTH / HEIGHT) are
// intentionally NOT used here so the interactive UI and the export can have
// independent sizes without a constants-file change.
const PUB_W: u32 = 3200;
const PUB_H: u32 = 1800;

// Pixels-per-metre for 300 DPI (300 / 0.0254 ≈ 11811).
// Embedded in the pHYs PNG chunk so importing tools know the intended DPI.
const DPI_300_PPM: u32 = 11_811;

// ── Colour palette ────────────────────────────────────────────────────────────
// Chosen for distinguishability in greyscale print (IEEE double-column figures
// are often printed black-and-white by readers).
const C_LINE:       RGBColor = RGBColor( 31,  73, 125);   // deep steel-blue line
const C_FILL:       RGBColor = RGBColor( 70, 130, 180);   // steel-blue area fill
const C_MEAN:       RGBColor = RGBColor( 20,  20,  20);   // near-black mean
const C_BAND_EDGE:  RGBColor = RGBColor(200,  60,  30);   // red band edge
const C_ANOMALY:    RGBColor = RGBColor(200,  60,  30);   // red anomaly tint
const C_BAR_NORMAL: RGBColor = RGBColor( 55, 100, 170);   // bar – normal
const C_BAR_SPIKE:  RGBColor = RGBColor(180,  35,  20);   // bar – spike
const C_UNIFORM:    RGBColor = RGBColor( 30,  30,  30);   // uniform reference
const C_BG:         RGBColor = RGBColor(255, 255, 255);   // white background
const C_GRID:       RGBColor = RGBColor(210, 212, 218);   // grid lines
const C_AXIS:       RGBColor = RGBColor( 50,  52,  60);   // axis spines / ticks

/// A bookmark region to overlay on an exported PNG chart.
///
/// `x0` and `x1` are data-space x coordinates (e.g. byte offsets).
/// `color` is an `(R, G, B)` triple matching the bookmark's `egui::Color32`.
/// `label` is the text shown inside the region.
pub struct ExportBookmark {
    pub x0:    f64,
    pub x1:    f64,
    pub color: (u8, u8, u8),
    pub label: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Line / metric chart
// ─────────────────────────────────────────────────────────────────────────────

pub fn export_line_chart_png(
    series:       &[[f64; 2]],
    title:        &str,
    x_axis_label: &str,
    y_axis_label: &str,
    anomaly_band: Option<(f64, f64, f64)>,
    bookmarks:    &[ExportBookmark],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let w = PUB_W;
    let h = PUB_H;
    let mut pixel_buffer = vec![0u8; (w * h * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        root.fill(&C_BG)?;

        if series.is_empty() {
            root.present()?;
            drop(root);
            return encode_rgb_to_png_300dpi(&pixel_buffer, w, h);
        }

        let x_min = series.iter().map(|p| p[0]).fold(f64::INFINITY,     f64::min);
        let x_max = series.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y_min = series.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let y_max = series.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);

        let (y_lo, y_hi) = if let Some((mean, sd, k)) = anomaly_band {
            let lo  = y_min.min(mean - k * sd - 0.05 * (y_max - y_min).max(1e-9));
            let hi  = y_max.max(mean + k * sd + 0.05 * (y_max - y_min).max(1e-9));
            let pad = (hi - lo).max(1e-9) * 0.10;
            (lo - pad, hi + pad)
        } else {
            // Extra top padding reserves visual room for the in-plot legend box.
            let pad = (y_max - y_min).max(1e-9) * 0.18;
            (y_min - pad * 0.5, y_max + pad)
        };

        // ── font sizes scaled to 3200-wide canvas ─────────────────────────
        let font_caption  = ("sans-serif", 56).into_font().style(FontStyle::Bold);
        let font_axis     = ("sans-serif", 67).into_font().style(FontStyle::Bold);
        let font_tick     = ("sans-serif", 57).into_font();
        let font_tick_sz  = 57i32;   // keep in sync with font_tick
        let font_axis_sz  = 67i32;   // keep in sync with font_axis

        // Dynamically size the y-label strip so the rotated axis-description
        // never collides with the tick numbers, regardless of value magnitude.
        //   strip = (widest tick label in chars × ~0.55 char-width factor)
        //         + one line-height for the axis description
        //         + a small padding gap
        let max_y_abs      = y_hi.abs().max(y_lo.abs());
        let max_tick_chars = format!("{:.3}", max_y_abs).len() as i32;
        let char_w_est     = (font_tick_sz as f64 * 0.55) as i32;
        let y_label_area   = (max_tick_chars * char_w_est + font_axis_sz + 35).max(205) as u32;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, font_caption.color(&RGBColor(20, 20, 20)))
            .margin(80)
            .x_label_area_size(125)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d(x_min..x_max, y_lo..y_hi)?;

        // ── Tufte-style mesh: horizontal reference lines only ─────────────
        // Strip from the y-axis label any word already present in the title
        // so the same term (e.g. "Entropy") is never shown twice.
        let y_desc_deduped: String = {
            let title_lc = title.to_lowercase();
            let words: Vec<&str> = y_axis_label
                .split_whitespace()
                .filter(|w| !title_lc.contains(&w.to_lowercase() as &str))
                .collect();
            words.join(" ")
        };

        chart.configure_mesh()
            .x_desc(x_axis_label)
            .y_desc(y_desc_deduped.as_str())
            .x_labels(12)
            .y_labels(10)
            .x_label_formatter(&|v| format!("0x{:X}", *v as usize))
            .y_label_formatter(&|v| format!("{:.3}", v))
            .axis_style(ShapeStyle::from(&C_AXIS).stroke_width(3))
            .bold_line_style(ShapeStyle::from(&C_GRID).stroke_width(1))
            .light_line_style(ShapeStyle::from(&RGBAColor(0, 0, 0, 0.0)).stroke_width(0))
            .x_label_style(font_tick.clone().color(&RGBColor(40, 40, 40)))
            .y_label_style(font_tick.clone().color(&RGBColor(40, 40, 40)))
            .axis_desc_style(font_axis.clone().color(&RGBColor(20, 20, 20)))
            .draw()?;

        // ── bookmark regions ──────────────────────────────────────────────
        // Clean translucent fill + solid left/right borders only.
        // No accent strips; labels go into the in-plot legend box below.
        for bm in bookmarks {
            let (r, g, b) = bm.color;
            let bx0 = bm.x0.max(x_min);
            let bx1 = bm.x1.min(x_max);
            if bx0 >= bx1 { continue; }

            let fill = RGBAColor(r, g, b, 0.13);
            chart.draw_series(std::iter::once(
                Rectangle::new(
                    [(bx0, y_lo), (bx1, y_hi)],
                    ShapeStyle { color: fill, filled: true, stroke_width: 0 },
                ),
            ))?;

            let border = RGBColor(r, g, b);
            for &edge_x in &[bx0, bx1] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(edge_x, y_lo), (edge_x, y_hi)],
                    ShapeStyle::from(&border).stroke_width(2),
                )))?;
            }
        }

        // ── downsample for dense series ───────────────────────────────────
        // Min-max envelope: preserves spikes while capping segments to
        // ~2× canvas pixel width.
        let target_pts = (PUB_W as usize) * 2;
        let display_series: Vec<[f64; 2]> = if series.len() > target_pts {
            let chunk_size = (series.len() + target_pts - 1) / (target_pts / 2);
            let mut out = Vec::with_capacity(target_pts);
            for chunk in series.chunks(chunk_size) {
                let (min_i, _) = chunk.iter().enumerate()
                    .min_by(|(_, a), (_, b)| a[1].partial_cmp(&b[1])
                        .unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap();
                let (max_i, _) = chunk.iter().enumerate()
                    .max_by(|(_, a), (_, b)| a[1].partial_cmp(&b[1])
                        .unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap();
                if min_i <= max_i {
                    out.push(chunk[min_i]);
                    out.push(chunk[max_i]);
                } else {
                    out.push(chunk[max_i]);
                    out.push(chunk[min_i]);
                }
            }
            out
        } else {
            series.to_vec()
        };

        // ── area fill under the data line ─────────────────────────────────
        let area_fill     = RGBAColor(C_FILL.0, C_FILL.1, C_FILL.2, 0.10);
        let area_baseline = y_lo.max(0.0);
        chart.draw_series(AreaSeries::new(
            display_series.iter().map(|p| (p[0], p[1])),
            area_baseline,
            area_fill,
        ))?;

        // ── data line ─────────────────────────────────────────────────────
        chart.draw_series(LineSeries::new(
            display_series.iter().map(|p| (p[0], p[1])),
            ShapeStyle::from(&C_LINE).stroke_width(4),
        ))?;

        // ── anomaly band overlays — drawn on top of data ──────────────────
        // No translucent fill.  Only the dashed ±k·σ threshold lines and the
        // solid mean reference line are drawn, with inline right-edge labels.
        //
        // Dashes are emulated via alternating PathElement segments because
        // Plotters has no native dash/stroke-pattern API.  Widths are
        // expressed as fractions of the x span so they scale with file size.
        if let Some((mean, sd, k)) = anomaly_band {
            let lo_band  = mean - k * sd;
            let hi_band  = mean + k * sd;
            let x_span   = (x_max - x_min).max(1.0);
            let dash_on  = x_span * 0.008;   // 0.8 % of span filled
            let dash_off = x_span * 0.004;   // 0.4 % gap

            // Dashed threshold lines — translucent dark red.
            let edge_color = RGBAColor(220, 60, 40, 0.55);
            for &edge_y in &[lo_band, hi_band] {
                let mut x = x_min;
                while x < x_max {
                    let x_end = (x + dash_on).min(x_max);
                    chart.draw_series(std::iter::once(PathElement::new(
                        vec![(x, edge_y), (x_end, edge_y)],
                        ShapeStyle { color: edge_color, filled: false, stroke_width: 2 },
                    )))?;
                    x = x_end + dash_off;
                }
            }

            // Mean reference line — solid, near-black, 3 px.
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(x_min, mean), (x_max, mean)],
                ShapeStyle::from(&C_MEAN).stroke_width(3),
            )))?;

            // ── right-edge labels ─────────────────────────────────────────
            // Draw directly on `root` (i32 pixel coords) rather than through
            // `chart` (f64 data coords) so Rectangle/Text compile cleanly.
            //
            // Known pixel layout for margin(80), y_label_area_size(220),
            // x_label_area_size(150) on a 3200×1800 canvas:
            //   plot_top    ≈ 150 px    plot_bottom ≈ 1570 px
            //   plot_right  = 3200 - 80 = 3120 px
            let plot_top_px:    f64 = 150.0;
            let plot_bottom_px: f64 = (PUB_H as f64) - 230.0;
            let plot_right_px:  i32 = (w as i32) - 86;
            let plot_h_px = plot_bottom_px - plot_top_px;
            let y_range   = (y_hi - y_lo).max(1e-12);

            let data_y_to_px = |dy: f64| -> i32 {
                (plot_top_px + (y_hi - dy) / y_range * plot_h_px) as i32
            };

            let font_ann  = ("sans-serif", 47).into_font().style(FontStyle::Italic);
            let ann_color = RGBColor(190, 45, 25);

            // "+/-Ks threshold" labels on both dashed lines.
            // ASCII only: avoids missing-glyph boxes on Windows system fonts.
            let k_str = if (k - k.round()).abs() < 0.05 {
                format!("+/-{:.0}s threshold", k)
            } else {
                format!("+/-{:.1}s threshold", k)
            };
            let k_label_w = k_str.len() as i32 * 15 + 8;

            for &edge_y in &[lo_band, hi_band] {
                let py = data_y_to_px(edge_y);
                root.draw(&Rectangle::new(
                    [(plot_right_px - k_label_w - 6, py - 22),
                     (plot_right_px,                  py +  6)],
                    ShapeStyle { color: RGBAColor(255, 255, 255, 0.88),
                                 filled: true, stroke_width: 0 },
                ))?;
                root.draw(&Text::new(
                    k_str.as_str(),
                    (plot_right_px - k_label_w - 4, py - 20),
                    font_ann.clone().color(&ann_color),
                ))?;
            }

            // "mean" label on the solid reference line.
            let mean_label   = "mean";
            let mean_label_w = mean_label.len() as i32 * 15 + 8;
            let mean_py = data_y_to_px(mean);
            root.draw(&Rectangle::new(
                [(plot_right_px - mean_label_w - 6, mean_py - 22),
                 (plot_right_px,                    mean_py +  6)],
                ShapeStyle { color: RGBAColor(255, 255, 255, 0.88),
                             filled: true, stroke_width: 0 },
            ))?;
            root.draw(&Text::new(
                mean_label,
                (plot_right_px - mean_label_w - 4, mean_py - 20),
                font_ann.clone().color(&RGBColor(30, 30, 30)),
            ))?;
        }

        // ── in-plot bookmark legend ───────────────────────────────────────
        // Drawn on `root` (i32 pixel coords) in the top-right corner of the
        // plot area.  Colour swatch + black label per unique bookmark group.
        let mut legend_entries: Vec<(String, (u8, u8, u8))> = Vec::new();
        for bm in bookmarks {
            if !bm.label.is_empty()
                && !legend_entries.iter().any(|(l, _)| l == &bm.label)
            {
                legend_entries.push((bm.label.clone(), bm.color));
            }
        }

        if !legend_entries.is_empty() {
            // Pixel layout constants matching ChartBuilder config above.
            let plot_x0: i32 = 300;               // margin(80) + y_label_area(220)
            let plot_y0: i32 = 150;               // margin(80) + caption(~70)
            let plot_x1: i32 = (w as i32) - 80;  // right margin

            let swatch_px = 32i32;
            let pad       = 18i32;
            let row_h     = 52i32;
            let max_chars = 32usize;

            // Auto-size box width to the longest label (~17 px/char at 30 pt).
            let char_w_est  = 17i32;
            let max_label_w = legend_entries.iter()
                .map(|(l, _)| l.chars().count().min(max_chars) as i32 * char_w_est)
                .max()
                .unwrap_or(200);
            let box_w = pad + swatch_px + pad + max_label_w + pad;
            let box_h = pad + legend_entries.len() as i32 * row_h + pad / 2;

            let inset  = 24i32;
            let box_x1 = plot_x1 - inset;
            let box_x0 = (box_x1 - box_w).max(plot_x0 + inset);
            let box_y0 = plot_y0 + inset;
            let box_y1 = box_y0 + box_h;

            // White fill + dark border.
            root.draw(&Rectangle::new(
                [(box_x0, box_y0), (box_x1, box_y1)],
                ShapeStyle { color: RGBAColor(255, 255, 255, 0.93),
                             filled: true, stroke_width: 0 },
            ))?;
            root.draw(&Rectangle::new(
                [(box_x0, box_y0), (box_x1, box_y1)],
                ShapeStyle { color: RGBAColor(60, 62, 70, 1.0),
                             filled: false, stroke_width: 2 },
            ))?;

            let font_leg = ("sans-serif", 47).into_font().style(FontStyle::Bold);

            for (row, (label, (r, g, b))) in legend_entries.iter().enumerate() {
                let row_top = box_y0 + pad + row as i32 * row_h;

                let sw_x0 = box_x0 + pad;
                let sw_y0 = row_top + (row_h - swatch_px) / 2;
                let sw_x1 = sw_x0 + swatch_px;
                let sw_y1 = sw_y0 + swatch_px;

                // Colour swatch — solid fill + 1-px darkened border.
                root.draw(&Rectangle::new(
                    [(sw_x0, sw_y0), (sw_x1, sw_y1)],
                    ShapeStyle { color: RGBAColor(*r, *g, *b, 1.0),
                                 filled: true, stroke_width: 0 },
                ))?;
                root.draw(&Rectangle::new(
                    [(sw_x0, sw_y0), (sw_x1, sw_y1)],
                    ShapeStyle {
                        color: RGBAColor(
                            r.saturating_sub(50),
                            g.saturating_sub(50),
                            b.saturating_sub(50),
                            1.0,
                        ),
                        filled: false, stroke_width: 1,
                    },
                ))?;

                // Truncate label to fit the box width.
                let display_label = if label.chars().count() > max_chars {
                    format!("{}…", &label[..label.char_indices()
                        .nth(max_chars)
                        .map(|(i, _)| i)
                        .unwrap_or(label.len())])
                } else {
                    label.clone()
                };

                // Black text — readable on white, no halo needed.
                root.draw(&Text::new(
                    display_label,
                    (sw_x1 + pad, row_top + (row_h - 30) / 2),
                    font_leg.clone().color(&BLACK),
                ))?;
            }
        }

        root.present()?;
    }

    encode_rgb_to_png_300dpi(&pixel_buffer, w, h)
}

// ─────────────────────────────────────────────────────────────────────────────
// Bar / byte-distribution chart
// ─────────────────────────────────────────────────────────────────────────────

pub fn export_bar_chart_png(
    byte_counts: &[usize; BYTE_RANGE],
    title:       &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let w = PUB_W;
    let h = PUB_H;
    let mut pixel_buffer = vec![0u8; (w * h * 3) as usize];

    {
        let root = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        root.fill(&C_BG)?;

        let max_count     = byte_counts.iter().cloned().max().unwrap_or(0).max(1) as f64;
        let total_bytes: usize = byte_counts.iter().sum();
        let uniform_level = total_bytes as f64 / BYTE_RANGE as f64;
        let spike_thresh  = uniform_level * UNIFORM_SPIKE_RATIO;

        let font_caption = ("sans-serif", 52).into_font().style(FontStyle::Bold);
        let font_axis    = ("sans-serif", 65).into_font();
        let font_tick    = ("sans-serif", 55).into_font();
        let font_tick_sz = 55i32;
        let font_axis_sz = 65i32;

        // Same dynamic y-label area calculation as the line chart.
        let max_tick_chars = {
            let n = max_count;
            if      n >= 1_000_000.0 { format!("{:.1}M", n / 1_000_000.0).len() }
            else if n >= 1_000.0     { format!("{:.1}k", n / 1_000.0).len() }
            else                     { format!("{}", n as usize).len() }
        } as i32;
        let char_w_est   = (font_tick_sz as f64 * 0.55) as i32;
        let y_label_area = (max_tick_chars * char_w_est + font_axis_sz + 35).max(193) as u32;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, font_caption)
            .margin(60)
            .x_label_area_size(110)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d(0i32..256i32, 0.0f64..max_count * 1.12)?;

        chart.configure_mesh()
            .x_desc("Byte value")
            .y_desc(if title.to_lowercase().contains("count") { "" } else { "Count" })
            .x_labels(9)
            .y_labels(8)
            .x_label_formatter(&|v| format!("0x{:02X}", *v as u8))
            .y_label_formatter(&|v| {
                let n = *v as usize;
                if      n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
                else if n >= 1_000     { format!("{:.1}k", n as f64 / 1_000.0) }
                else                   { format!("{n}") }
            })
            .axis_style(ShapeStyle::from(&RGBColor(80, 82, 90)).stroke_width(2))
            .bold_line_style(ShapeStyle::from(&C_GRID).stroke_width(1))
            .light_line_style(ShapeStyle::from(&RGBAColor(0, 0, 0, 0.0)).stroke_width(0))
            .x_label_style(font_tick.clone())
            .y_label_style(font_tick.clone())
            .axis_desc_style(font_axis.clone())
            .draw()?;

        // Shaded "expected uniform" band up to the spike threshold.
        let uniform_fill = RGBAColor(C_ANOMALY.0, C_ANOMALY.1, C_ANOMALY.2, 0.06);
        chart.draw_series(std::iter::once(
            Rectangle::new(
                [(0i32, 0.0), (256i32, uniform_level * UNIFORM_SPIKE_RATIO)],
                ShapeStyle { color: uniform_fill, filled: true, stroke_width: 0 },
            ),
        ))?;

        // Bars.
        chart.draw_series(
            (0i32..256i32).map(|idx| {
                let count = byte_counts[idx as usize] as f64;
                let color = if count > spike_thresh { &C_BAR_SPIKE } else { &C_BAR_NORMAL };
                Rectangle::new(
                    [(idx, 0.0), (idx + 1, count)],
                    ShapeStyle::from(color).filled(),
                )
            }),
        )?;

        // Uniform reference line — 2 px so it reads clearly in print.
        chart.draw_series(std::iter::once(PathElement::new(
            vec![(0i32, uniform_level), (256i32, uniform_level)],
            ShapeStyle::from(&C_UNIFORM).stroke_width(2),
        )))?;

        root.present()?;
    }

    encode_rgb_to_png_300dpi(&pixel_buffer, w, h)
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoder — with 300 DPI pHYs chunk
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a raw RGB pixel buffer to PNG and embed a pHYs chunk declaring
/// 300 DPI so that applications (Word, LaTeX, Illustrator, GIMP) import the
/// figure at the correct physical dimensions without manual DPI override.
fn encode_rgb_to_png_300dpi(
    rgb_data: &[u8],
    width:    u32,
    height:   u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut png_bytes = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut png_bytes);
        let mut encoder = png::Encoder::new(cursor, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        // pHYs chunk: pixels per metre, unit = metre (1).
        // 300 DPI = 300 / 0.0254 ≈ 11 811 pixels per metre.
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: DPI_300_PPM,
            yppu: DPI_300_PPM,
            unit: png::Unit::Meter,
        }));
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb_data)?;
    }
    Ok(png_bytes)
}

// ─────────────────────────────────────────────────────────────────────────────
// File-save dialog
// ─────────────────────────────────────────────────────────────────────────────

pub fn save_png_via_dialog(png_data: Vec<u8>, suggested_stem: &str) {
    if let Some(save_path) = FileDialog::new()
        .set_file_name(&format!("{suggested_stem}.png"))
        .add_filter("PNG image", &["png"])
        .save_file()
    {
        let _ = fs::write(save_path, png_data);
    }
}