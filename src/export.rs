use std::fs;
use plotters::prelude::*;
use rfd::FileDialog;

use crate::constants::{BYTE_RANGE, PNG_CHART_HEIGHT, PNG_CHART_WIDTH, UNIFORM_SPIKE_RATIO};

pub fn export_line_chart_png(
    series:          &[[f64; 2]],
    title:           &str,
    x_axis_label:    &str,
    y_axis_label:    &str,
    anomaly_band:    Option<(f64, f64, f64)>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let w = PNG_CHART_WIDTH;
    let h = PNG_CHART_HEIGHT;
    let mut pixel_buffer = vec![0u8; (w * h * 3) as usize];
    {
        let drawing_area = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        drawing_area.fill(&WHITE)?;

        if series.is_empty() {
            drawing_area.present()?;
            drop(drawing_area);
            return encode_rgb_to_png(&pixel_buffer, w, h);
        }

        let x_min = series.iter().map(|p| p[0]).fold(f64::INFINITY,     f64::min);
        let x_max = series.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y_min = series.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let y_max = series.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);

        let (y_range_lo, y_range_hi) = if let Some((band_mean, band_sd, band_k)) = anomaly_band {
            let lo  = y_min.min(band_mean - band_k * band_sd);
            let hi  = y_max.max(band_mean + band_k * band_sd);
            let pad = (hi - lo).max(1e-9) * 0.08;
            (lo - pad, hi + pad)
        } else {
            let pad = (y_max - y_min).max(1e-9) * 0.08;
            (y_min - pad, y_max + pad)
        };

        let mut chart = ChartBuilder::on(&drawing_area)
            .caption(title, ("sans-serif", 14).into_font())
            .margin(10)
            .x_label_area_size(44)
            .y_label_area_size(72)
            .build_cartesian_2d(x_min..x_max, y_range_lo..y_range_hi)?;

        chart.configure_mesh()
            .x_desc(x_axis_label)
            .y_desc(y_axis_label)
            .x_label_formatter(&|v| format!("0x{:X}", *v as usize))
            .y_label_formatter(&|v| format!("{:.3}", v))
            .draw()?;

        chart.draw_series(LineSeries::new(
            series.iter().map(|p| (p[0], p[1])),
            &RGBColor(180, 30, 30),
        ))?;

        if let Some((band_mean, band_sd, band_k)) = anomaly_band {
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(x_min, band_mean), (x_max, band_mean)],
                ShapeStyle::from(&BLACK).stroke_width(1),
            )))?;
            for threshold_line in [band_mean + band_k * band_sd, band_mean - band_k * band_sd] {
                let dash_segment_len = (x_max - x_min) / 120.0;
                let mut draw_segment = true;
                let mut x_cursor     = x_min;
                while x_cursor < x_max {
                    let x_segment_end = (x_cursor + dash_segment_len).min(x_max);
                    if draw_segment {
                        chart.draw_series(std::iter::once(PathElement::new(
                            vec![(x_cursor, threshold_line), (x_segment_end, threshold_line)],
                            ShapeStyle::from(&RGBColor(100, 100, 100)).stroke_width(1),
                        )))?;
                    }
                    x_cursor     = x_segment_end;
                    draw_segment = !draw_segment;
                }
            }
        }
        drawing_area.present()?;
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
        let drawing_area   = BitMapBackend::with_buffer(&mut pixel_buffer, (w, h)).into_drawing_area();
        drawing_area.fill(&WHITE)?;

        let max_bar_height = byte_counts.iter().cloned().max().unwrap_or(0).max(1) as f64;
        let total_bytes: usize = byte_counts.iter().sum();
        let uniform_level      = total_bytes as f64 / BYTE_RANGE as f64;

        let mut chart = ChartBuilder::on(&drawing_area)
            .caption(title, ("sans-serif", 14).into_font())
            .margin(10)
            .x_label_area_size(44)
            .y_label_area_size(72)
            .build_cartesian_2d(0i32..256i32, 0.0f64..max_bar_height * 1.08)?;

        chart.configure_mesh()
            .x_desc("Byte value (0x00 – 0xFF)")
            .y_desc("Occurrences")
            .x_labels(9)
            .x_label_formatter(&|v| format!("0x{:02X}", *v as u8))
            .y_label_formatter(&|v| format!("{}", *v as usize))
            .draw()?;

        chart.draw_series(
            (0i32..256i32).map(|byte_idx| {
                let count     = byte_counts[byte_idx as usize] as f64;
                let bar_color = if count > uniform_level * UNIFORM_SPIKE_RATIO {
                    ShapeStyle::from(&RGBColor(180, 30, 30)).filled()
                } else {
                    ShapeStyle::from(&RGBColor(60, 60, 60)).filled()
                };
                Rectangle::new([(byte_idx, 0.0), (byte_idx + 1, count)], bar_color)
            }),
        )?;

        chart.draw_series(std::iter::once(PathElement::new(
            vec![(0i32, uniform_level), (256i32, uniform_level)],
            ShapeStyle::from(&BLACK).stroke_width(1),
        )))?;

        drawing_area.present()?;
    }
    encode_rgb_to_png(&pixel_buffer, w, h)
}

fn encode_rgb_to_png(rgb_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
