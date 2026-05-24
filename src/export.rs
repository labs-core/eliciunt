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
const C_LINE:       RGBColor = RGBColor(210,  40,  40);   // strong blue line
const C_FILL:       RGBColor = RGBColor( 80, 120, 200);   // muted blue area fill
const C_MEAN:       RGBColor = RGBColor( 20,  20,  20);   // near-black mean
const C_BAND_EDGE:  RGBColor = RGBColor(200,  60,  30);   // red band edge
const C_ANOMALY:    RGBColor = RGBColor(200,  60,  30);   // red anomaly tint
const C_BAR_NORMAL: RGBColor = RGBColor( 70, 110, 190);   // bar – normal
const C_BAR_SPIKE:  RGBColor = RGBColor(190,  45,  25);   // bar – spike
const C_UNIFORM:    RGBColor = RGBColor( 20,  20,  20);   // uniform reference
const C_BG:         RGBColor = RGBColor(255, 255, 255);   // white background
const C_GRID:       RGBColor = RGBColor(190, 192, 200);   // grid bold lines

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
            let pad = (hi - lo).max(1e-9) * 0.12;
            (lo - pad, hi + pad)
        } else {
            let pad = (y_max - y_min).max(1e-9) * 0.14;
            (y_min - pad, y_max + pad)
        };

        // ── font sizes scaled to 3200-wide canvas ─────────────────────────
        // At 300 DPI and a 88-mm column the canvas is reduced ~11×, so a
        // 22-pt caption becomes ~2 pt on paper — that's still readable.
        // Using proportional ("sans-serif") font throughout for clean print.
        let font_caption = ("sans-serif", 52).into_font().style(FontStyle::Bold);
        let font_axis    = ("sans-serif", 38).into_font();
        let font_tick    = ("sans-serif", 32).into_font();

        let mut chart = ChartBuilder::on(&root)
            .caption(title, font_caption)
            .margin_top(60)
            .margin_bottom(60)
            .margin_left(60)
            .margin_right(520)          // reserved for the side legend panel
            .x_label_area_size(130)
            .y_label_area_size(200)
            .build_cartesian_2d(x_min..x_max, y_lo..y_hi)?;

        chart.configure_mesh()
            .x_desc(x_axis_label)
            .y_desc(y_axis_label)
            .x_labels(10)
            .y_labels(8)
            .x_label_formatter(&|v| format!("0x{:X}", *v as usize))
            .y_label_formatter(&|v| format!("{:.3}", v))
            // Axis spine: dark, 2 px
            .axis_style(ShapeStyle::from(&RGBColor(80, 82, 90)).stroke_width(2))
            // Bold grid lines only — no light sub-grid (reduces visual noise
            // that is especially distracting in print).
            .bold_line_style(ShapeStyle::from(&C_GRID).stroke_width(1))
            .light_line_style(ShapeStyle::from(&RGBAColor(0, 0, 0, 0.0)).stroke_width(0))
            .x_label_style(font_tick.clone())
            .y_label_style(font_tick.clone())
            .axis_desc_style(font_axis.clone())
            .draw()?;

        // ── anomaly band ──────────────────────────────────────────────────
        if let Some((mean, sd, k)) = anomaly_band {
            let lo_band = mean - k * sd;
            let hi_band = mean + k * sd;

            // Translucent fill — light enough not to obscure the data line.
            let band_fill = RGBAColor(C_ANOMALY.0, C_ANOMALY.1, C_ANOMALY.2, 0.07);
            chart.draw_series(std::iter::once(
                Rectangle::new(
                    [(x_min, lo_band), (x_max, hi_band)],
                    ShapeStyle { color: band_fill, filled: true, stroke_width: 0 },
                ),
            ))?;

            // Solid band-edge lines.
            for &edge_y in &[lo_band, hi_band] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(x_min, edge_y), (x_max, edge_y)],
                    ShapeStyle::from(&C_BAND_EDGE).stroke_width(2),
                )))?;
            }

            // Mean reference line.
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(x_min, mean), (x_max, mean)],
                ShapeStyle::from(&C_MEAN).stroke_width(2),
            )))?;
        }

        // ── bookmark regions ──────────────────────────────────────────────
        // Rendering order: fill → accent strips → border lines → label.
        for bm in bookmarks {
            let (r, g, b) = bm.color;
            let bx0 = bm.x0.max(x_min);
            let bx1 = bm.x1.min(x_max);
            if bx0 >= bx1 { continue; }

            // 1. Main fill.
            let fill = RGBAColor(r, g, b, 0.18);
            chart.draw_series(std::iter::once(
                Rectangle::new(
                    [(bx0, y_lo), (bx1, y_hi)],
                    ShapeStyle { color: fill, filled: true, stroke_width: 0 },
                ),
            ))?;

            // 2. Inner accent strips (~4 % of span).
            let span    = (bx1 - bx0).abs();
            let strip_w = (span * 0.04).min(span * 0.25).max(1.0);
            let accent  = RGBAColor(r, g, b, 0.38);
            for (left, right) in [(bx0, bx0 + strip_w), (bx1 - strip_w, bx1)] {
                chart.draw_series(std::iter::once(
                    Rectangle::new(
                        [(left, y_lo), (right, y_hi)],
                        ShapeStyle { color: accent, filled: true, stroke_width: 0 },
                    ),
                ))?;
            }

            // 3. Solid 2-px border lines.
            let border = RGBColor(r, g, b);
            for &edge_x in &[bx0, bx1] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(edge_x, y_lo), (edge_x, y_hi)],
                    ShapeStyle::from(&border).stroke_width(3),
                )))?;
            }

            // 4. Label moved to side legend — not drawn inside the chart.
        }

        // ── downsample for dense series ───────────────────────────────────
        // When the window size is small the series can have hundreds of
        // thousands of points.  Plotters cannot handle that many segments
        // reliably at any canvas size — rendering breaks down visually.
        //
        // Fix: split the series into consecutive index-based chunks (NOT
        // x-value buckets).  Because the input series is already sorted by
        // byte offset, every chunk covers a contiguous x range.  Within each
        // chunk we keep the min-y and max-y samples *in their original index
        // order*, so the emitted x values never go backwards.  This preserves
        // all spikes and troughs faithfully while capping the rendered point
        // count to ~2× the canvas pixel width.
        let target_pts  = (PUB_W as usize) * 2;      // 6400 for a 3200-px canvas
        let display_series: Vec<[f64; 2]> = if series.len() > target_pts {
            let chunk_size = (series.len() + target_pts - 1) / (target_pts / 2);
            let mut out = Vec::with_capacity(target_pts);
            for chunk in series.chunks(chunk_size) {
                // Find min and max by y-value within this chunk.
                let (min_i, _) = chunk.iter().enumerate()
                    .min_by(|(_, a), (_, b)| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap();
                let (max_i, _) = chunk.iter().enumerate()
                    .max_by(|(_, a), (_, b)| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap();
                // Emit in original index order so x stays monotonically increasing.
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
        // AreaSeries is used instead of Polygon::new because it never
        // self-intersects: it goes forward along the data then straight back
        // along the baseline, regardless of how the data oscillates.
        let area_fill     = RGBAColor(C_FILL.0, C_FILL.1, C_FILL.2, 0.12);
        let area_baseline = y_lo.max(0.0);
        chart.draw_series(AreaSeries::new(
            display_series.iter().map(|p| (p[0], p[1])),
            area_baseline,
            area_fill,
        ))?;

        // ── data line ─────────────────────────────────────────────────────
        chart.draw_series(LineSeries::new(
            display_series.iter().map(|p| (p[0], p[1])),
            ShapeStyle::from(&C_LINE).stroke_width(3),
        ))?;

        // ── side legend panel ─────────────────────────────────────────────
        // Collect unique (label, color) entries — skip entries whose label is
        // empty (sub-regions of a multi-range group beyond the first) and
        // deduplicate by label so each group appears once.
        let mut legend_entries: Vec<(String, (u8, u8, u8))> = Vec::new();
        for bm in bookmarks {
            if !bm.label.is_empty() {
                // Deduplicate: if the same label already appears (e.g. two
                // single-range bookmarks with the same name), skip.
                if !legend_entries.iter().any(|(l, _)| l == &bm.label) {
                    legend_entries.push((bm.label.clone(), bm.color));
                }
            }
        }

        if !legend_entries.is_empty() {
            // Legend box geometry — anchored in the right margin reserved above.
            // Left edge = canvas_width − right_margin + padding.
            let legend_x      = (w as i32) - 500;   // left edge of legend column
            let legend_top    = 120i32;              // top of first entry
            let swatch_size   = 28i32;               // colour square side
            let row_h         = 52i32;               // height per entry row
            let text_x_off    = swatch_size + 16;    // text starts after swatch
            let font_legend_h = ("sans-serif", 28).into_font().style(FontStyle::Bold);
            let font_legend   = ("sans-serif", 28).into_font();

            // Panel background — very light grey so it reads as a distinct box.
            let panel_w  = 480i32;
            let panel_h  = legend_entries.len() as i32 * row_h + 24;
            root.draw(&Rectangle::new(
                [
                    (legend_x - 12, legend_top - 12),
                    (legend_x + panel_w, legend_top + panel_h),
                ],
                ShapeStyle {
                    color:        RGBAColor(230, 232, 238, 1.0),
                    filled:       true,
                    stroke_width: 0,
                },
            ))?;
            // Panel border.
            root.draw(&Rectangle::new(
                [
                    (legend_x - 12, legend_top - 12),
                    (legend_x + panel_w, legend_top + panel_h),
                ],
                ShapeStyle {
                    color:        RGBAColor(150, 152, 160, 1.0),
                    filled:       false,
                    stroke_width: 2,
                },
            ))?;

            // "BOOKMARKS" header.
            root.draw(&Text::new(
                "BOOKMARKS",
                (legend_x, legend_top - 54),
                ("sans-serif", 30).into_font()
                    .style(FontStyle::Bold)
                    .color(&RGBColor(80, 82, 90)),
            ))?;

            for (row, (label, (r, g, b))) in legend_entries.iter().enumerate() {
                let row_y = legend_top + row as i32 * row_h + 6;

                // Colour swatch.
                root.draw(&Rectangle::new(
                    [(legend_x, row_y), (legend_x + swatch_size, row_y + swatch_size)],
                    ShapeStyle {
                        color:        RGBAColor(*r, *g, *b, 0.88),
                        filled:       true,
                        stroke_width: 0,
                    },
                ))?;
                // Swatch border.
                root.draw(&Rectangle::new(
                    [(legend_x, row_y), (legend_x + swatch_size, row_y + swatch_size)],
                    ShapeStyle {
                        color:        RGBAColor(*r, *g, *b, 1.0),
                        filled:       false,
                        stroke_width: 2,
                    },
                ))?;

                // Truncate very long labels so they fit in the panel.
                let max_chars = 28usize;
                let display_label = if label.chars().count() > max_chars {
                    format!("{}…", &label[..label.char_indices()
                        .nth(max_chars)
                        .map(|(i, _)| i)
                        .unwrap_or(label.len())])
                } else {
                    label.clone()
                };

                // Label text — white shadow for legibility, then coloured text.
                let text_x  = legend_x + text_x_off;
                let text_y  = row_y + 4;
                let text_col = RGBColor(*r, *g, *b);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    root.draw(&Text::new(
                        display_label.clone(),
                        (text_x + dx, text_y + dy),
                        font_legend_h.clone().color(&WHITE),
                    ))?;
                }
                root.draw(&Text::new(
                    display_label.clone(),
                    (text_x, text_y),
                    font_legend.clone().color(&text_col),
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
        let font_axis    = ("sans-serif", 38).into_font();
        let font_tick    = ("sans-serif", 32).into_font();

        let mut chart = ChartBuilder::on(&root)
            .caption(title, font_caption)
            .margin(60)
            .x_label_area_size(130)
            .y_label_area_size(200)
            .build_cartesian_2d(0i32..256i32, 0.0f64..max_count * 1.12)?;

        chart.configure_mesh()
            .x_desc("Byte value")
            .y_desc("Count")
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