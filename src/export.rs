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

use crate::constants::{BYTE_RANGE, PNG_CHART_HEIGHT, PNG_CHART_WIDTH, UNIFORM_SPIKE_RATIO};

const C_LINE:       RGBColor = RGBColor(41,  98,  255);
const C_FILL:       RGBColor = RGBColor(41,  98,  255);
const C_MEAN:       RGBColor = RGBColor(30,  30,  30);
const C_BAND_EDGE:  RGBColor = RGBColor(220, 80,  40);
const C_ANOMALY:    RGBColor = RGBColor(220, 80,  40);
const C_BAR_NORMAL: RGBColor = RGBColor(60,  100, 180);
const C_BAR_SPIKE:  RGBColor = RGBColor(200, 50,  30);
const C_UNIFORM:    RGBColor = RGBColor(20,  20,  20);
const C_BG:         RGBColor = RGBColor(252, 252, 253);
const C_GRID:       RGBColor = RGBColor(220, 222, 228);

pub fn export_line_chart_png(
    series:       &[[f64; 2]],
    title:        &str,
    x_axis_label: &str,
    y_axis_label: &str,
    anomaly_band: Option<(f64, f64, f64)>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let w = PNG_CHART_WIDTH;
    let h = PNG_CHART_HEIGHT;
    let mut pixel_buffer = vec![0u8; (w * h * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        root.fill(&RGBColor(C_BG.0, C_BG.1, C_BG.2))?;

        if series.is_empty() {
            root.present()?;
            drop(root);
            return encode_rgb_to_png(&pixel_buffer, w, h);
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
            let pad = (y_max - y_min).max(1e-9) * 0.12;
            (y_min - pad, y_max + pad)
        };

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 16).into_font())
            .margin(18)
            .x_label_area_size(50)
            .y_label_area_size(80)
            .build_cartesian_2d(x_min..x_max, y_lo..y_hi)?;

        chart.configure_mesh()
            .x_desc(x_axis_label)
            .y_desc(y_axis_label)
            .x_labels(10)
            .y_labels(8)
            .x_label_formatter(&|v| format!("0x{:X}", *v as usize))
            .y_label_formatter(&|v| format!("{:.3}", v))
            .axis_style(ShapeStyle::from(&RGBColor(160, 163, 175)).stroke_width(1))
            .bold_line_style(ShapeStyle::from(&RGBColor(C_GRID.0, C_GRID.1, C_GRID.2)).stroke_width(1))
            .light_line_style(ShapeStyle::from(&RGBColor(235, 236, 240)).stroke_width(1))
            .label_style(("sans-serif", 11).into_font())
            .draw()?;

        if let Some((mean, sd, k)) = anomaly_band {
            let lo_band = mean - k * sd;
            let hi_band = mean + k * sd;

            let band_fill = RGBAColor(C_ANOMALY.0, C_ANOMALY.1, C_ANOMALY.2, 0.08);
            chart.draw_series(std::iter::once(
                Rectangle::new(
                    [(x_min, lo_band), (x_max, hi_band)],
                    ShapeStyle { color: band_fill, filled: true, stroke_width: 0 },
                ),
            ))?;

            for &edge_y in &[lo_band, hi_band] {
                chart.draw_series(std::iter::once(PathElement::new(
                    vec![(x_min, edge_y), (x_max, edge_y)],
                    ShapeStyle::from(&C_BAND_EDGE).stroke_width(1),
                )))?;
            }

            chart.draw_series(std::iter::once(PathElement::new(
                vec![(x_min, mean), (x_max, mean)],
                ShapeStyle::from(&C_MEAN).stroke_width(1),
            )))?;
        }

        let area_fill = RGBAColor(C_FILL.0, C_FILL.1, C_FILL.2, 0.15);
        let mut area_pts: Vec<(f64, f64)> = Vec::with_capacity(series.len() + 2);
        area_pts.push((series[0][0], y_lo.max(0.0)));
        for p in series { area_pts.push((p[0], p[1])); }
        area_pts.push((series[series.len() - 1][0], y_lo.max(0.0)));
        chart.draw_series(std::iter::once(
            Polygon::new(
                area_pts,
                ShapeStyle { color: area_fill, filled: true, stroke_width: 0 },
            ),
        ))?;

        chart.draw_series(LineSeries::new(
            series.iter().map(|p| (p[0], p[1])),
            ShapeStyle::from(&C_LINE).stroke_width(2),
        ))?;

        root.present()?;
    }
    encode_rgb_to_png(&pixel_buffer, w, h)
}

pub fn export_bar_chart_png(
    byte_counts: &[usize; BYTE_RANGE],
    title:       &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let w = PNG_CHART_WIDTH;
    let h = PNG_CHART_HEIGHT;
    let mut pixel_buffer = vec![0u8; (w * h * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        root.fill(&RGBColor(C_BG.0, C_BG.1, C_BG.2))?;

        let max_count     = byte_counts.iter().cloned().max().unwrap_or(0).max(1) as f64;
        let total_bytes: usize = byte_counts.iter().sum();
        let uniform_level = total_bytes as f64 / BYTE_RANGE as f64;
        let spike_thresh  = uniform_level * UNIFORM_SPIKE_RATIO;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 16).into_font())
            .margin(18)
            .x_label_area_size(50)
            .y_label_area_size(80)
            .build_cartesian_2d(0i32..256i32, 0.0f64..max_count * 1.10)?;

        chart.configure_mesh()
            .x_desc("Byte value")
            .y_desc("Count")
            .x_labels(9)
            .y_labels(8)
            .x_label_formatter(&|v| format!("0x{:02X}", *v as u8))
            .y_label_formatter(&|v| format!("{}", *v as usize))
            .axis_style(ShapeStyle::from(&RGBColor(160, 163, 175)).stroke_width(1))
            .bold_line_style(ShapeStyle::from(&RGBColor(C_GRID.0, C_GRID.1, C_GRID.2)).stroke_width(1))
            .light_line_style(ShapeStyle::from(&RGBColor(235, 236, 240)).stroke_width(1))
            .label_style(("sans-serif", 11).into_font())
            .draw()?;

        let uniform_fill  = RGBAColor(C_ANOMALY.0, C_ANOMALY.1, C_ANOMALY.2, 0.07);
        chart.draw_series(std::iter::once(
            Rectangle::new(
                [(0i32, 0.0), (256i32, uniform_level * UNIFORM_SPIKE_RATIO)],
                ShapeStyle { color: uniform_fill, filled: true, stroke_width: 0 },
            ),
        ))?;

        chart.draw_series(
            (0i32..256i32).map(|idx| {
                let count = byte_counts[idx as usize] as f64;
                let style = if count > spike_thresh {
                    ShapeStyle::from(&C_BAR_SPIKE).filled()
                } else {
                    ShapeStyle::from(&C_BAR_NORMAL).filled()
                };
                Rectangle::new([(idx, 0.0), (idx + 1, count)], style)
            }),
        )?;

        chart.draw_series(std::iter::once(PathElement::new(
            vec![(0i32, uniform_level), (256i32, uniform_level)],
            ShapeStyle::from(&C_UNIFORM).stroke_width(2),
        )))?;

        root.present()?;
    }
    encode_rgb_to_png(&pixel_buffer, w, h)
}

fn encode_rgb_to_png(
    rgb_data: &[u8],
    width:    u32,
    height:   u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(std::io::Cursor::new(&mut png_bytes), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgb_data)?;
    }
    Ok(png_bytes)
}

pub fn save_png_via_dialog(png_data: Vec<u8>, suggested_stem: &str) {
    if let Some(save_path) = FileDialog::new()
        .set_file_name(&format!("{suggested_stem}.png"))
        .add_filter("PNG image", &["png"])
        .save_file()
    {
        let _ = fs::write(save_path, png_data);
    }
}
