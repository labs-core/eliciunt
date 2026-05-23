use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke, Vec2};
use egui_plot::{Bar, BarChart, HLine, Line, Plot, PlotPoints};
use rfd::FileDialog;
use std::{collections::HashSet, fs};

const MAX_BYTE:  usize = 256;
const HEX_WIDTH: usize = 16;

mod pal {
    use egui::Color32;
    pub const BG:        Color32 = Color32::from_rgb(255, 255, 255);
    pub const PANEL:     Color32 = Color32::from_rgb(255, 255, 255);
    pub const BORDER:    Color32 = Color32::from_rgb(230, 210, 210);
    pub const RED:       Color32 = Color32::from_rgb(196,  28,  28);
    pub const RED_MID:   Color32 = Color32::from_rgb(220,  80,  80);
    pub const RED_LIGHT: Color32 = Color32::from_rgb(254, 226, 226);
    pub const RED_FAINT: Color32 = Color32::from_rgb(255, 245, 245);
    pub const TEXT:      Color32 = Color32::from_rgb( 20,  10,  10);
    pub const MUTED:     Color32 = Color32::from_rgb(150, 100, 100);
    pub const HL_BG:     Color32 = Color32::from_rgb(255, 210, 210);
    pub const HL_BORDER: Color32 = Color32::from_rgb(196,  28,  28);
    pub const GREEN:     Color32 = Color32::from_rgb( 22, 120,  60);
}

fn erfc_approx(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let p = t * (0.254829592
        + t * (-0.284496736
        + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    let r = p * (-x * x).exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

fn normal_upper(z: f64) -> f64 {
    0.5 * erfc_approx(z / std::f64::consts::SQRT_2)
}

fn chi2_pvalue(x: f64, df: usize) -> f64 {
    if x <= 0.0 || df == 0 { return 1.0; }
    let d    = df as f64;
    let cbrt = (x / d).powf(1.0 / 3.0);
    let mu   = 1.0 - 2.0 / (9.0 * d);
    let sig  = (2.0 / (9.0 * d)).sqrt();
    normal_upper((cbrt - mu) / sig)
}

fn ks_uniform_test(data: &[u8]) -> (f64, f64) {
    if data.is_empty() { return (0.0, 1.0); }
    let n = data.len();
    let mut counts = [0usize; MAX_BYTE];
    for &b in data { counts[b as usize] += 1; }
    let mut cdf_emp = 0.0f64;
    let mut d = 0.0f64;
    for i in 0..MAX_BYTE {
        cdf_emp += counts[i] as f64 / n as f64;
        let cdf_th = (i + 1) as f64 / MAX_BYTE as f64;
        d = d.max((cdf_emp - cdf_th).abs());
    }
    let dn = d * (n as f64).sqrt();
    let mut p_sum = 0.0f64;
    for k in 1i64..=60 {
        let term = (-2.0 * (k * k) as f64 * dn * dn).exp();
        if k % 2 == 1 { p_sum += term; } else { p_sum -= term; }
        if term < 1e-14 { break; }
    }
    (d, (2.0 * p_sum).clamp(0.0, 1.0))
}

fn runs_test(data: &[u8]) -> (f64, f64) {
    if data.len() < 2 { return (0.0, 1.0); }
    let median = {
        let mut s = data.to_vec();
        s.sort_unstable();
        s[s.len() / 2] as f64
    };
    let above: Vec<bool> = data.iter().map(|&b| (b as f64) >= median).collect();
    let n1 = above.iter().filter(|&&a|  a).count() as f64;
    let n2 = above.iter().filter(|&&a| !a).count() as f64;
    let n  = n1 + n2;
    if n1 == 0.0 || n2 == 0.0 { return (0.0, 1.0); }
    let mut runs = 1u64;
    for i in 1..above.len() {
        if above[i] != above[i - 1] { runs += 1; }
    }
    let r   = runs as f64;
    let mu  = 2.0 * n1 * n2 / n + 1.0;
    let var = 2.0 * n1 * n2 * (2.0 * n1 * n2 - n) / (n * n * (n - 1.0));
    if var <= 0.0 { return (0.0, 1.0); }
    let z = (r - mu) / var.sqrt();
    (z, (2.0 * normal_upper(z.abs())).min(1.0))
}

#[derive(Clone, Default)]
struct MetricStats {
    mean: f64,
    sd:   f64,
    min:  f64,
    max:  f64,
}

impl MetricStats {
    fn from_series(data: &[[f64; 2]]) -> Self {
        if data.is_empty() { return Self::default(); }
        let vals: Vec<f64> = data.iter().map(|p| p[1]).collect();
        let n    = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let sd   = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
        let min  = vals.iter().cloned().fold(f64::INFINITY,     f64::min);
        let max  = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Self { mean, sd, min, max }
    }
}

#[derive(Clone, Default)]
struct FileStatistics {
    entropy_stats: MetricStats,
    chi2_stats:    MetricStats,
    serial_stats:  MetricStats,
    hamming_stats: MetricStats,
    ks_d:          f64,
    ks_p:          f64,
    global_chi2:   f64,
    global_chi2_p: f64,
    runs_z:        f64,
    runs_p:        f64,
    mean_window_p: f64,
}

use plotters::prelude::*;

fn png_line_chart(
    data:          &[[f64; 2]],
    title:         &str,
    x_label:       &str,
    y_label:       &str,
    anomaly_bands: Option<(f64, f64, f64)>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; 900 * 400 * 3];
    {
        let root = BitMapBackend::with_buffer(&mut buf, (900, 400)).into_drawing_area();
        root.fill(&WHITE)?;

        if data.is_empty() {
            root.present()?;
            drop(root);
            return png_from_rgb(&buf, 900, 400);
        }

        let x_min = data.iter().map(|p| p[0]).fold(f64::INFINITY,     f64::min);
        let x_max = data.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y_min = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let y_max = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);

        let (y_lo, y_hi) = if let Some((mean, sd, k)) = anomaly_bands {
            let lo  = y_min.min(mean - k * sd);
            let hi  = y_max.max(mean + k * sd);
            let pad = (hi - lo).max(1e-9) * 0.08;
            (lo - pad, hi + pad)
        } else {
            let pad = (y_max - y_min).max(1e-9) * 0.08;
            (y_min - pad, y_max + pad)
        };

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 14).into_font())
            .margin(10)
            .x_label_area_size(44)
            .y_label_area_size(72)
            .build_cartesian_2d(x_min..x_max, y_lo..y_hi)?;

        chart.configure_mesh()
            .x_desc(x_label)
            .y_desc(y_label)
            .x_label_formatter(&|v| format!("0x{:X}", *v as usize))
            .y_label_formatter(&|v| format!("{:.3}", v))
            .draw()?;

        chart.draw_series(LineSeries::new(
            data.iter().map(|p| (p[0], p[1])),
            &RGBColor(180, 30, 30),
        ))?;

        if let Some((mean, sd, k)) = anomaly_bands {
            chart.draw_series(std::iter::once(PathElement::new(
                vec![(x_min, mean), (x_max, mean)],
                ShapeStyle::from(&BLACK).stroke_width(1),
            )))?;
            for val in [mean + k * sd, mean - k * sd] {
                let seg = (x_max - x_min) / 120.0;
                let mut on = true;
                let mut x  = x_min;
                while x < x_max {
                    let x_end = (x + seg).min(x_max);
                    if on {
                        chart.draw_series(std::iter::once(PathElement::new(
                            vec![(x, val), (x_end, val)],
                            ShapeStyle::from(&RGBColor(100, 100, 100)).stroke_width(1),
                        )))?;
                    }
                    x  = x_end;
                    on = !on;
                }
            }
        }

        root.present()?;
    }
    png_from_rgb(&buf, 900, 400)
}

fn png_bar_chart(counts: &[usize; MAX_BYTE], title: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; 900 * 400 * 3];
    {
        let root      = BitMapBackend::with_buffer(&mut buf, (900, 400)).into_drawing_area();
        root.fill(&WHITE)?;
        let max_count = counts.iter().cloned().max().unwrap_or(0).max(1) as f64;
        let total:    usize = counts.iter().sum();
        let uniform   = total as f64 / MAX_BYTE as f64;

        let mut chart = ChartBuilder::on(&root)
            .caption(title, ("sans-serif", 14).into_font())
            .margin(10)
            .x_label_area_size(44)
            .y_label_area_size(72)
            .build_cartesian_2d(0i32..256i32, 0.0f64..max_count * 1.08)?;

        chart.configure_mesh()
            .x_desc("Byte value (0x00 – 0xFF)")
            .y_desc("Occurrences")
            .x_labels(9)
            .x_label_formatter(&|v| format!("0x{:02X}", *v as u8))
            .y_label_formatter(&|v| format!("{}", *v as usize))
            .draw()?;

        chart.draw_series(
            (0i32..256i32).map(|i| {
                let count     = counts[i as usize] as f64;
                let bar_style = if count > uniform * 1.5 {
                    ShapeStyle::from(&RGBColor(180, 30, 30)).filled()
                } else {
                    ShapeStyle::from(&RGBColor(60, 60, 60)).filled()
                };
                Rectangle::new([(i, 0.0), (i + 1, count)], bar_style)
            }),
        )?;

        chart.draw_series(std::iter::once(PathElement::new(
            vec![(0i32, uniform), (256i32, uniform)],
            ShapeStyle::from(&BLACK).stroke_width(1),
        )))?;

        root.present()?;
    }
    png_from_rgb(&buf, 900, 400)
}

fn png_from_rgb(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(std::io::Cursor::new(&mut out), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header()?;
        writer.write_image_data(rgb)?;
    }
    Ok(out)
}

fn save_png(data: Vec<u8>, stem: &str) {
    if let Some(path) = FileDialog::new()
        .set_file_name(&format!("{stem}.png"))
        .add_filter("PNG image", &["png"])
        .save_file()
    {
        let _ = fs::write(path, data);
    }
}

#[derive(Clone, Default)]
struct AnomalyThresholds {
    entropy_mean: f64, entropy_sd: f64,
    chi2_mean:    f64, chi2_sd:    f64,
    serial_mean:  f64, serial_sd:  f64,
}

impl AnomalyThresholds {
    fn from_metrics(e: &[[f64; 2]], c: &[[f64; 2]], s: &[[f64; 2]]) -> Self {
        fn mean_sd(v: &[[f64; 2]]) -> (f64, f64) {
            if v.is_empty() { return (0.0, 1.0); }
            let n  = v.len() as f64;
            let mu = v.iter().map(|p| p[1]).sum::<f64>() / n;
            let sd = (v.iter().map(|p| (p[1] - mu).powi(2)).sum::<f64>() / n).sqrt();
            (mu, sd.max(f64::EPSILON))
        }
        let (em, es) = mean_sd(e);
        let (cm, cs) = mean_sd(c);
        let (sm, ss) = mean_sd(s);
        Self {
            entropy_mean: em, entropy_sd: es,
            chi2_mean:    cm, chi2_sd:    cs,
            serial_mean:  sm, serial_sd:  ss,
        }
    }

    fn is_suspicious(&self, entropy: f64, chi2: f64, serial: f64, k: f64) -> bool {
        (entropy - self.entropy_mean).abs() / self.entropy_sd > k
            || (chi2   - self.chi2_mean).abs()   / self.chi2_sd   > k
            || (serial - self.serial_mean).abs() / self.serial_sd > k
    }
}

#[derive(Clone)]
struct RegionInsight {
    offset:      usize,
    entropy:     f64,
    chi2:        f64,
    chi2_p:      f64,
    serial_corr: f64,
    hamming:     f64,
    suspicious:  bool,
}

#[derive(Clone)]
struct AnalysisResult {
    entropy:        Vec<[f64; 2]>,
    chi2:           Vec<[f64; 2]>,
    serial_corr:    Vec<[f64; 2]>,
    hamming:        Vec<[f64; 2]>,
    byte_dist:      [f64; MAX_BYTE],
    byte_counts:    [usize; MAX_BYTE],
    bigram_scores:  Vec<[f64; 2]>,
    trigram_scores: Vec<[f64; 2]>,
    regions:        Vec<RegionInsight>,
    thresholds:     AnomalyThresholds,
    window_size:    usize,
    stats:          FileStatistics,
}

impl Default for AnalysisResult {
    fn default() -> Self {
        Self {
            entropy:        Vec::new(),
            chi2:           Vec::new(),
            serial_corr:    Vec::new(),
            hamming:        Vec::new(),
            byte_dist:      [0.0; MAX_BYTE],
            byte_counts:    [0usize; MAX_BYTE],
            bigram_scores:  Vec::new(),
            trigram_scores: Vec::new(),
            regions:        Vec::new(),
            thresholds:     AnomalyThresholds::default(),
            window_size:    0,
            stats:          FileStatistics::default(),
        }
    }
}

struct BinaryFile {
    name:   String,
    data:   Vec<u8>,
    result: Option<AnalysisResult>,
}

struct App {
    files:              Vec<BinaryFile>,
    window_size:        usize,
    sel_file:           usize,
    active_tab:         usize,
    show_hex:           bool,
    anomaly_k:          f64,
    hex_highlight:      Option<(usize, usize)>,
    hex_scroll_pending: Option<usize>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            files:              Vec::new(),
            window_size:        512,
            sel_file:           0,
            active_tab:         0,
            show_hex:           false,
            anomaly_k:          2.0,
            hex_highlight:      None,
            hex_scroll_pending: None,
        }
    }
}

impl App {
    fn load_file() -> Option<BinaryFile> {
        let path = FileDialog::new().pick_file()?;
        let data = fs::read(&path).ok()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Some(BinaryFile { name, data, result: None })
    }

    fn analyze(data: &[u8], w: usize, k: f64) -> AnalysisResult {
        let mut r     = AnalysisResult::default();
        r.window_size = w;
        if data.is_empty() { return r; }

        let len_f    = data.len() as f64;
        let mut hist = [0usize; MAX_BYTE];
        for &b in data { hist[b as usize] += 1; }
        for i in 0..MAX_BYTE {
            r.byte_dist[i]   = hist[i] as f64 / len_f;
            r.byte_counts[i] = hist[i];
        }

        let step = w.max(1);
        for offset in (0..data.len().saturating_sub(w) + 1).step_by(step) {
            let end = (offset + w).min(data.len());
            if end - offset < w { break; }
            let s  = &data[offset..end];
            let c2 = compute_chi2(s);
            r.entropy.push([offset as f64,       compute_entropy(s)]);
            r.chi2.push([offset as f64,           c2]);
            r.serial_corr.push([offset as f64,    serial_correlation(s)]);
            r.hamming.push([offset as f64,         hamming_weight(s)]);
            r.bigram_scores.push([offset as f64,  ngram_uniqueness(s, 2)]);
            r.trigram_scores.push([offset as f64, ngram_uniqueness(s, 3)]);
        }

        let thr = AnomalyThresholds::from_metrics(&r.entropy, &r.chi2, &r.serial_corr);
        for i in 0..r.entropy.len() {
            let c2 = r.chi2[i][1];
            r.regions.push(RegionInsight {
                offset:      r.entropy[i][0] as usize,
                entropy:     r.entropy[i][1],
                chi2:        c2,
                chi2_p:      chi2_pvalue(c2, 255),
                serial_corr: r.serial_corr[i][1],
                hamming:     r.hamming[i][1],
                suspicious:  thr.is_suspicious(r.entropy[i][1], c2, r.serial_corr[i][1], k),
            });
        }
        r.thresholds = thr;

        let global_chi2   = compute_chi2(data);
        let (ks_d, ks_p)  = ks_uniform_test(data);
        let (runs_z, runs_p) = runs_test(data);
        let mean_window_p = if r.regions.is_empty() { 1.0 } else {
            r.regions.iter().map(|reg| reg.chi2_p).sum::<f64>() / r.regions.len() as f64
        };
        r.stats = FileStatistics {
            entropy_stats: MetricStats::from_series(&r.entropy),
            chi2_stats:    MetricStats::from_series(&r.chi2),
            serial_stats:  MetricStats::from_series(&r.serial_corr),
            hamming_stats: MetricStats::from_series(&r.hamming),
            ks_d, ks_p,
            global_chi2,
            global_chi2_p: chi2_pvalue(global_chi2, 255),
            runs_z, runs_p,
            mean_window_p,
        };
        r
    }

    fn reapply_k(result: &mut AnalysisResult, k: f64) {
        let thr = AnomalyThresholds::from_metrics(&result.entropy, &result.chi2, &result.serial_corr);
        for reg in &mut result.regions {
            reg.suspicious = thr.is_suspicious(reg.entropy, reg.chi2, reg.serial_corr, k);
        }
        result.thresholds = thr;
    }
}

fn compute_entropy(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let len = data.len() as f64;
    hist.iter()
        .filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / len; -p * p.log2() })
        .sum()
}

fn compute_chi2(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let exp = data.len() as f64 / 256.0;
    hist.iter().map(|&o| { let d = o as f64 - exp; d * d / exp }).sum()
}

fn serial_correlation(data: &[u8]) -> f64 {
    if data.len() < 2 { return 0.0; }
    let n = data.len() as f64;
    let (mut sum, mut sum_sq, mut serial_sum) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..data.len() {
        let x = data[i] as f64;
        let y = data[(i + 1) % data.len()] as f64;
        sum        += x;
        sum_sq     += x * x;
        serial_sum += x * y;
    }
    let num = n * serial_sum - sum * sum;
    let den = n * sum_sq    - sum * sum;
    if den.abs() < f64::EPSILON { 0.0 } else { num / den }
}

fn hamming_weight(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    data.iter().map(|b| b.count_ones()).sum::<u32>() as f64 / data.len() as f64
}

fn ngram_uniqueness(data: &[u8], n: usize) -> f64 {
    if data.len() < n { return 0.0; }
    let total  = data.len() - n + 1;
    let unique: HashSet<&[u8]> = (0..total).map(|i| &data[i..i + n]).collect();
    unique.len() as f64 / total as f64
}

fn card_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(14.0),
        outer_margin: Margin::symmetric(0.0, 4.0),
        rounding:     Rounding::same(6.0),
        fill:         pal::PANEL,
        stroke:       Stroke::new(1.0, pal::BORDER),
        ..Default::default()
    }
}

fn png_btn(ui: &mut egui::Ui) -> bool {
    ui.add(
        egui::Button::new(RichText::new("⬇ PNG").size(10.0).color(pal::RED))
            .fill(pal::RED_FAINT)
            .stroke(Stroke::new(1.0, pal::RED_MID))
            .rounding(Rounding::same(3.0))
            .min_size(Vec2::new(44.0, 16.0)),
    ).clicked()
}

const PLOT_HEIGHT: f32 = 155.0;

fn plot_one_metric(
    ui:       &mut egui::Ui,
    title:    &str,
    unit:     &str,
    data:     &[[f64; 2]],
    bands:    Option<(f64, f64, f64)>,
    file_idx: usize,
    plot_id:  usize,
) -> Option<usize> {
    let (y_min, y_max) = if data.is_empty() {
        (0.0, 1.0)
    } else {
        let lo = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let hi = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let lo = bands.map(|(m, s, k)| lo.min(m - k * s)).unwrap_or(lo);
        let hi = bands.map(|(m, s, k)| hi.max(m + k * s)).unwrap_or(hi);
        (lo, hi)
    };

    let pr = Plot::new(format!("metric_{file_idx}_{plot_id}"))
        .height(PLOT_HEIGHT)
        .show_axes([true, true])
        .show_grid([true, true])
        .include_y(y_min)
        .include_y(y_max)
        .auto_bounds([true, false].into())
        .set_margin_fraction(Vec2::new(0.02, 0.10))
        .x_axis_formatter(|mark, _, _| format!("0x{:X}", mark.value as usize))
        .y_axis_formatter(|mark, _, _| format!("{:.3}", mark.value))
        .label_formatter(move |name, point| {
            format!("offset: 0x{:X}\n{}: {:.4}", point.x as usize, name, point.y)
        })
        .show(ui, |pui| {
            pui.line(
                Line::new(PlotPoints::from(data.to_vec()))
                    .color(pal::RED)
                    .width(1.5)
                    .name(title),
            );
            if let Some((mean, sd, k)) = bands {
                pui.hline(HLine::new(mean)
                    .color(Color32::from_rgb(80, 80, 80)).width(1.2).name("μ"));
                pui.hline(HLine::new(mean + k * sd)
                    .color(Color32::from_rgb(160, 160, 160)).width(1.0)
                    .style(egui_plot::LineStyle::Dashed { length: 6.0 }).name("μ+kσ"));
                pui.hline(HLine::new(mean - k * sd)
                    .color(Color32::from_rgb(160, 160, 160)).width(1.0)
                    .style(egui_plot::LineStyle::Dashed { length: 6.0 }).name("μ−kσ"));
            }
        });

    if pr.response.hovered() {
        ui.input_mut(|i| {
            i.smooth_scroll_delta = Vec2::ZERO;
            i.raw_scroll_delta    = Vec2::ZERO;
        });
    }

    if pr.response.secondary_clicked() {
        if let Some(pointer) = pr.response.interact_pointer_pos() {
            let plot_pos = pr.transform.value_from_position(pointer);
            return Some(plot_pos.x.max(0.0) as usize);
        }
    }

    None
}

fn plot_metrics_row(
    ui:      &mut egui::Ui,
    title:   &str,
    unit:    &str,
    entries: &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
    col_w:   f32,
    row_id:  usize,
) -> Option<(usize, usize)> {
    let mut clicked: Option<(usize, usize)> = None;
    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{title}  ({unit})"))
                    .strong().color(pal::TEXT).size(12.0),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("right-click → jump to hex")
                    .size(10.0).color(pal::MUTED).italics(),
            );
        });
        ui.add_space(4.0);

        // Use allocate_ui_with_layout with an EXPLICIT height instead of
        // horizontal_top.  horizontal_top hands each child the full
        // available_rect_before_wrap() height; inside a non-shrinking
        // ScrollArea that is the entire viewport.  On the first frame a new
        // plot widget appears its cached size is unknown, so min_rect comes
        // back as the full available rect and the card fills the screen.
        // Pre-committing the rect here prevents that entirely.
        let row_h = PLOT_HEIGHT
            + ui.text_style_height(&egui::TextStyle::Body)   // filename label
            + ui.text_style_height(&egui::TextStyle::Body)   // png button row
            + ui.spacing().item_spacing.y * 4.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui| {
            let n = entries.len();
            for (col_i, (data, bands, file_idx, file_name)) in entries.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.set_max_width(col_w);
                    ui.set_max_height(row_h);

                    if n > 1 {
                        let short = if file_name.len() > 30 {
                            format!("…{}", &file_name[file_name.len().saturating_sub(27)..])
                        } else {
                            (*file_name).to_string()
                        };
                        ui.label(RichText::new(short).size(10.0).color(pal::MUTED).italics());
                    }

                    if let Some(off) = plot_one_metric(
                        ui, title, unit, data, *bands, *file_idx,
                        row_id * 64 + col_i,
                    ) {
                        clicked = Some((*file_idx, off));
                    }

                    ui.horizontal(|ui| {
                        if png_btn(ui) {
                            let label = format!("{title} ({unit})");
                            match png_line_chart(data, title, "File offset (bytes)", &label, *bands) {
                                Ok(png) => save_png(
                                    png,
                                    &format!("{}_{file_idx}", title.to_lowercase().replace(' ', "_")),
                                ),
                                Err(e) => eprintln!("PNG export error: {e}"),
                            }
                        }
                    });
                });

                if col_i + 1 < n {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                }
            }
        });
    });
    clicked
}

fn plot_distribution(ui: &mut egui::Ui, result: &AnalysisResult, file_name: &str, file_idx: usize) {
    let counts        = &result.byte_counts;
    let total: usize  = counts.iter().sum();
    let uniform_count = total as f64 / MAX_BYTE as f64;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Byte Frequency Distribution")
                    .strong().color(pal::TEXT).size(13.0),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display = if file_name.len() > 40 {
                format!("…{}", &file_name[file_name.len() - 37..])
            } else {
                file_name.to_owned()
            };
            ui.label(RichText::new(display).size(11.0).color(pal::MUTED).italics());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if png_btn(ui) {
                    let title = format!("Byte Frequency Distribution — {file_name}");
                    match png_bar_chart(counts, &title) {
                        Ok(png) => save_png(png, &format!("byte_dist_{file_idx}")),
                        Err(e)  => eprintln!("PNG export error: {e}"),
                    }
                }
                ui.label(
                    RichText::new(format!("max occurrences = {}", counts.iter().cloned().max().unwrap_or(0)))
                        .size(11.0).color(pal::MUTED),
                );
            });
        });
        ui.add_space(6.0);

        let bars: Vec<Bar> = (0..MAX_BYTE)
            .map(|i| {
                let count = counts[i] as f64;
                Bar::new(i as f64, count)
                    .width(0.9)
                    .fill(if count > uniform_count * 1.5 { pal::RED } else { pal::RED_MID })
                    .stroke(Stroke::NONE)
                    .name(format!("0x{:02X}", i))
            })
            .collect();

        Plot::new(format!("byte_dist_{file_idx}"))
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
            .label_formatter(move |name, point| {
                format!("byte: {}\noccurrences: {}", name, point.y as usize)
            })
            .show(ui, |pui| {
                pui.bar_chart(BarChart::new(bars).color(pal::RED).name("count"));
                pui.hline(
                    HLine::new(uniform_count)
                        .color(Color32::from_rgb(80, 80, 80))
                        .width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name("uniform"),
                );
            });
    });
}

fn suspicious_regions(ui: &mut egui::Ui, result: &AnalysisResult, file_name: &str, file_idx: usize) -> Option<usize> {
    let suspicious: Vec<&RegionInsight> = result.regions.iter().filter(|r| r.suspicious).collect();
    let mut jump_to: Option<usize> = None;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Anomalous Regions").strong().color(pal::TEXT).size(13.0));
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display = if file_name.len() > 40 {
                format!("…{}", &file_name[file_name.len() - 37..])
            } else {
                file_name.to_owned()
            };
            ui.label(RichText::new(display).size(11.0).color(pal::MUTED).italics());
            ui.add_space(6.0);

            let n       = suspicious.len();
            let (fg, bg) = if n > 0 { (pal::RED, pal::RED_LIGHT) } else { (pal::MUTED, pal::PANEL) };
            let lbl     = format!("{n} flagged");
            let font    = egui::FontId::proportional(11.0);
            let tw      = ui.fonts(|f| f.layout_no_wrap(lbl.clone(), font.clone(), fg).size().x);
            let (rect, _) = ui.allocate_at_least(Vec2::new(tw + 18.0, 20.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, &lbl, font, fg);
        });

        ui.add_space(4.0);
        let t = &result.thresholds;
        ui.label(
            RichText::new(format!(
                "entropy μ={:.3} σ={:.3}  ·  χ² μ={:.1} σ={:.1}  ·  serial μ={:.4} σ={:.4}",
                t.entropy_mean, t.entropy_sd, t.chi2_mean, t.chi2_sd, t.serial_mean, t.serial_sd,
            ))
            .size(10.0).color(pal::MUTED),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Click a row to jump to that offset in the Hex Dump.")
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        if suspicious.is_empty() {
            ui.label(RichText::new("No anomalies detected.").color(pal::MUTED).size(12.0));
            return;
        }

        egui::Grid::new(format!("regions_grid_{file_idx}"))
            .num_columns(7)
            .striped(true)
            .min_col_width(72.0)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for h in &["Offset", "Entropy", "Chi²", "p(χ²)", "Serial ρ", "Hamming", ""] {
                    ui.label(RichText::new(*h).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for reg in &suspicious {
                    let resp  = ui.add(egui::SelectableLabel::new(
                        false,
                        RichText::new(format!("0x{:08X}", reg.offset)).monospace().size(12.0),
                    ));
                    let r_ent = ui.add(egui::Label::new(RichText::new(format!("{:.4}", reg.entropy)).monospace().size(12.0)).sense(egui::Sense::click()));
                    let r_c2  = ui.add(egui::Label::new(RichText::new(format!("{:.2}",  reg.chi2)).monospace().size(12.0)).sense(egui::Sense::click()));
                    let pc    = if reg.chi2_p < 0.05 { pal::RED } else { pal::GREEN };
                    let r_c2p = ui.add(egui::Label::new(RichText::new(format!("{:.4}", reg.chi2_p)).monospace().size(12.0).color(pc)).sense(egui::Sense::click()));
                    let r_ser = ui.add(egui::Label::new(RichText::new(format!("{:.4}", reg.serial_corr)).monospace().size(12.0)).sense(egui::Sense::click()));
                    let r_ham = ui.add(egui::Label::new(RichText::new(format!("{:.4}", reg.hamming)).monospace().size(12.0)).sense(egui::Sense::click()));
                    let go    = ui.add(
                        egui::Button::new(RichText::new("⟶ Hex").size(11.0).color(pal::RED))
                            .fill(pal::RED_FAINT).stroke(Stroke::new(1.0, pal::RED_MID))
                            .rounding(Rounding::same(4.0)).min_size(Vec2::new(56.0, 18.0)),
                    );
                    let any_right = resp.secondary_clicked()
                        || r_ent.secondary_clicked()
                        || r_c2.secondary_clicked()
                        || r_c2p.secondary_clicked()
                        || r_ser.secondary_clicked()
                        || r_ham.secondary_clicked();
                    if resp.clicked() || go.clicked() || any_right {
                        jump_to = Some(reg.offset);
                    }
                    ui.end_row();
                }
            });
    });

    jump_to
}

fn statistics_tab(ui: &mut egui::Ui, result: &AnalysisResult, file_name: &str, file_idx: usize) {
    let s = &result.stats;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Global Randomness Tests").strong().color(pal::TEXT).size(13.0));
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display = if file_name.len() > 40 {
                format!("…{}", &file_name[file_name.len() - 37..])
            } else {
                file_name.to_owned()
            };
            ui.label(RichText::new(display).size(11.0).color(pal::MUTED).italics());
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new("H₀: bytes are i.i.d. Uniform{0,…,255}.  Significance level α = 0.05.")
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        let rows: &[(&str, &str, f64, f64, &str, &str)] = &[
            ("Kolmogorov–Smirnov",    &format!("D = {:.5}",   s.ks_d),         s.ks_p,          0.05, "Non-uniform distribution",        "Consistent with Uniform[0,255]"),
            ("Chi² (global, df=255)", &format!("χ² = {:.2}",  s.global_chi2),  s.global_chi2_p, 0.05, "Non-uniform distribution",        "Consistent with Uniform[0,255]"),
            ("Wald–Wolfowitz runs",   &format!("Z = {:.4}",   s.runs_z),        s.runs_p,        0.05, "Non-random sequential structure", "Consistent with independent draws"),
        ];

        egui::Grid::new(format!("global_tests_grid_{file_idx}"))
            .num_columns(5).min_col_width(100.0).spacing([12.0, 7.0])
            .show(ui, |ui| {
                for h in &["Test", "Statistic", "p-value", "Reject H₀?", "Interpretation"] {
                    ui.label(RichText::new(*h).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();
                for &(test, stat_str, p, alpha, rej, acc) in rows {
                    let reject = p < alpha;
                    let col    = if reject { pal::RED } else { pal::GREEN };
                    ui.label(RichText::new(test).size(12.0));
                    ui.label(RichText::new(stat_str).monospace().size(12.0));
                    ui.label(
                        RichText::new(if p < 0.0001 { "< 0.0001".to_owned() } else { format!("{:.4}", p) })
                            .monospace().size(12.0).color(col),
                    );
                    ui.label(RichText::new(if reject { "Yes" } else { "No" }).size(12.0).color(col).strong());
                    ui.label(RichText::new(if reject { rej } else { acc }).size(11.0).color(col));
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Mean per-window χ² p-value: {:.4}   (≈ 0.5 expected for uniform random data)",
                s.mean_window_p
            ))
            .size(11.0).color(pal::MUTED),
        );
    });

    ui.add_space(8.0);

    card_frame().show(ui, |ui| {
        ui.label(RichText::new("Per-metric Statistics (windowed)").strong().color(pal::TEXT).size(13.0));
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Window size: {} bytes", result.window_size))
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        let rows: &[(&str, &str, &str, &MetricStats)] = &[
            ("Entropy",            "bits / symbol", "[0, 8]",          &s.entropy_stats),
            ("Chi² statistic",     "dimensionless", "df=255, E=w/256", &s.chi2_stats),
            ("Serial correlation", "ρ",             "[−1, 1]",         &s.serial_stats),
            ("Hamming weight",     "bits / byte",   "[0, 8]",          &s.hamming_stats),
        ];

        egui::Grid::new("metric_stats_grid")
            .num_columns(7).min_col_width(72.0).spacing([12.0, 7.0])
            .show(ui, |ui| {
                for h in &["Metric", "Unit", "Theoretical range", "Mean", "Std dev", "Min", "Max"] {
                    ui.label(RichText::new(*h).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();
                for &(name, unit, theory, ms) in rows {
                    ui.label(RichText::new(name).size(12.0));
                    ui.label(RichText::new(unit).size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(theory).monospace().size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(format!("{:.5}", ms.mean)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.sd)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.min)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.max)).monospace().size(12.0));
                    ui.end_row();
                }
            });
    });

    ui.add_space(8.0);

    card_frame().show(ui, |ui| {
        ui.label(RichText::new("Interpretation Guide").strong().color(pal::TEXT).size(13.0));
        ui.add_space(6.0);
        let items: &[(&str, &str)] = &[
            ("Entropy ≈ 8 bits/symbol",  "Near-maximal uncertainty — typical of compressed or encrypted data."),
            ("Entropy ≪ 8 bits/symbol",  "Significant redundancy — structured, sparse, or padding regions."),
            ("Chi² p-value ≪ 0.05",      "Byte distribution deviates from uniform — likely structured content."),
            ("Serial |ρ| ≫ 0",           "Adjacent bytes are linearly correlated — sequential structure present."),
            ("Hamming weight ≈ 4.0",      "Expected for uniform random bytes (bit probability ≈ 0.5)."),
            ("Hamming weight ≈ 8.0",      "All bits set — consistent with erased flash (0xFF fill)."),
            ("Hamming weight ≈ 0.0",      "All bits clear — consistent with zero-fill padding."),
            ("KS p-value < 0.05",         "Global CDF diverges from uniform — byte usage is skewed."),
            ("Runs test p-value < 0.05",  "Non-random sequential structure — long runs or alternating patterns."),
        ];
        for (term, explanation) in items {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(*term).monospace().size(12.0).color(pal::RED));
                ui.label(RichText::new("—").size(12.0).color(pal::MUTED));
                ui.label(RichText::new(*explanation).size(12.0));
            });
            ui.add_space(3.0);
        }
    });
}

fn hex_view(ui: &mut egui::Ui, data: &[u8], scroll_to: Option<usize>, highlight: Option<(usize, usize)>) {
    let total_lines = (data.len() + HEX_WIDTH - 1) / HEX_WIDTH;
    let row_height  = ui.text_style_height(&egui::TextStyle::Monospace)
        + ui.spacing().item_spacing.y;

    let mut scroll = egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false]);

    if let Some(target_byte) = scroll_to {
        let target_line = target_byte / HEX_WIDTH;
        let y = (target_line.saturating_sub(3)) as f32 * row_height;
        scroll = scroll.vertical_scroll_offset(y);
    }

    let hl_range: Option<std::ops::RangeInclusive<usize>> = highlight.map(|(start, len)| {
        let first = start / HEX_WIDTH;
        let last  = start.saturating_add(len).saturating_sub(1) / HEX_WIDTH;
        first..=last
    });

    scroll.show_rows(ui, row_height, total_lines, |ui, row_range| {
        for line in row_range {
            let start = line * HEX_WIDTH;
            let end   = (start + HEX_WIDTH).min(data.len());
            let slice = &data[start..end];

            let mut hex   = format!("{:08X}  ", start);
            let mut ascii = String::with_capacity(HEX_WIDTH);
            for (i, b) in slice.iter().enumerate() {
                if i == 8 { hex.push(' '); }
                hex.push_str(&format!("{:02X} ", b));
                ascii.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '·' });
            }
            let missing = HEX_WIDTH - slice.len();
            for i in 0..missing {
                if slice.len() + i == 8 { hex.push(' '); }
                hex.push_str("   ");
            }
            hex.push_str(&format!(" │ {}", ascii));
            const HEX_ROW_WIDTH: f32 = 720.0;
            let highlighted = hl_range.as_ref().map_or(false, |r| r.contains(&line));
            if highlighted {
                let desired = Vec2::new(HEX_ROW_WIDTH, row_height);
                let (rect, _)  = ui.allocate_exact_size(desired, egui::Sense::hover());
                ui.painter().rect(rect.expand(1.0), Rounding::same(2.0), pal::HL_BG, Stroke::new(1.0, pal::HL_BORDER));
                ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, &hex, egui::FontId::monospace(12.0), pal::HL_BORDER);
            } else {
                ui.label(RichText::new(&hex).monospace().size(12.0).color(pal::TEXT));
            }
        }
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut vis = ctx.style().visuals.clone();
        vis.override_text_color              = Some(pal::TEXT);
        vis.panel_fill                       = pal::BG;
        vis.window_fill                      = pal::PANEL;
        vis.faint_bg_color                   = pal::RED_FAINT;
        vis.extreme_bg_color                 = pal::PANEL;
        vis.widgets.noninteractive.bg_fill   = pal::PANEL;
        vis.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal::BORDER);
        vis.widgets.inactive.bg_fill         = pal::RED_FAINT;
        vis.widgets.hovered.bg_fill          = pal::RED_LIGHT;
        vis.widgets.hovered.bg_stroke        = Stroke::new(1.0, pal::RED_MID);
        vis.widgets.active.bg_fill           = pal::RED;
        vis.selection.bg_fill                = pal::RED_LIGHT;
        vis.selection.stroke                 = Stroke::new(1.0, pal::RED);
        ctx.set_visuals(vis);

        egui::TopBottomPanel::top("topbar")
            .frame(Frame {
                inner_margin: Margin::symmetric(16.0, 10.0),
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ELICIUNT").size(18.0).strong().color(pal::RED));
                    ui.separator();

                    if ui.add(
                        egui::Button::new(RichText::new("+ Load Binary").size(12.0).color(Color32::WHITE))
                            .fill(pal::RED).stroke(Stroke::NONE).rounding(Rounding::same(5.0)),
                    ).clicked() {
                        if let Some(file) = Self::load_file() {
                            self.files.push(file);
                        }
                    }

                    if ui.add(
                        egui::Button::new(RichText::new("▶ Analyze").size(12.0).color(Color32::WHITE))
                            .fill(pal::RED_MID).stroke(Stroke::NONE).rounding(Rounding::same(5.0)),
                    ).clicked() {
                        let (w, k) = (self.window_size, self.anomaly_k);
                        for file in &mut self.files {
                            file.result = Some(Self::analyze(&file.data, w, k));
                        }
                    }

                    ui.separator();
                    ui.label(RichText::new("Window:").size(12.0).color(pal::MUTED));
                    ui.scope(|ui| {
                        let vis = ui.visuals_mut();
                        vis.widgets.inactive.bg_fill   = pal::RED_FAINT;
                        vis.widgets.inactive.fg_stroke = Stroke::new(1.0, pal::RED);
                        vis.widgets.hovered.bg_fill    = pal::RED_LIGHT;
                        vis.widgets.active.bg_fill     = pal::RED_LIGHT;
                        vis.widgets.active.fg_stroke   = Stroke::new(1.0, pal::RED);
                        ui.add(
                            egui::DragValue::new(&mut self.window_size)
                                .clamp_range(64..=100_000_000usize)
                                .speed(16.0)
                                .suffix(" B"),
                        );
                    });

                    ui.separator();
                    ui.label(RichText::new("Sensitivity (σ):").size(12.0).color(pal::MUTED));
                    let k_resp = ui.add(
                        egui::DragValue::new(&mut self.anomaly_k)
                            .clamp_range(0.5..=5.0_f64)
                            .speed(0.05)
                            .max_decimals(2),
                    );
                    if k_resp.changed() {
                        let k = self.anomaly_k;
                        for file in &mut self.files {
                            if let Some(ref mut result) = file.result {
                                App::reapply_k(result, k);
                            }
                        }
                    }

                    ui.separator();
                    let hex_label = if self.show_hex { "✕ Hex" } else { "⟨/⟩ Hex" };
                    if ui.add(
                        egui::Button::new(
                            RichText::new(hex_label).size(12.0)
                                .color(if self.show_hex { Color32::WHITE } else { pal::RED }),
                        )
                        .fill(if self.show_hex { pal::RED } else { pal::RED_FAINT })
                        .stroke(Stroke::new(1.0, pal::RED))
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        self.show_hex = !self.show_hex;
                    }

                    if let Some((off, len)) = self.hex_highlight {
                        ui.separator();
                        ui.label(
                            RichText::new(format!("⚑ 0x{:08X} + {}B", off, len))
                                .size(11.0).color(pal::RED).monospace(),
                        );
                        if ui.add(
                            egui::Button::new(RichText::new("✕").size(11.0).color(pal::MUTED))
                                .fill(pal::RED_FAINT).stroke(Stroke::NONE).rounding(Rounding::same(3.0)),
                        ).on_hover_text("Clear highlight").clicked() {
                            self.hex_highlight = None;
                        }
                    }
                });
            });

        egui::SidePanel::left("filelist")
            .resizable(true)
            .default_width(180.0)
            .frame(Frame {
                inner_margin: Margin::same(12.0),
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(RichText::new("FILES").size(10.0).color(pal::MUTED).strong());
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_source("file_list_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.files.is_empty() {
                            ui.label(RichText::new("No files loaded.").size(12.0).color(pal::MUTED));
                        }
                        for (idx, file) in self.files.iter().enumerate() {
                            let selected = idx == self.sel_file;
                            if ui.add(egui::SelectableLabel::new(
                                selected, RichText::new(&file.name).size(12.0),
                            )).clicked() {
                                self.sel_file = idx;
                            }
                            if let Some(ref r) = file.result {
                                let n           = r.regions.iter().filter(|r| r.suspicious).count();
                                let summary_col = if r.stats.ks_p < 0.05 { pal::RED } else { pal::GREEN };
                                ui.label(
                                    RichText::new(format!("  KS p={:.3}  χ²p={:.3}", r.stats.ks_p, r.stats.global_chi2_p))
                                        .size(10.0).color(summary_col),
                                );
                                if n > 0 {
                                    ui.label(RichText::new(format!("  ⚠ {} regions", n)).size(10.0).color(pal::RED));
                                }
                            } else {
                                ui.label(RichText::new("  not analyzed").size(10.0).color(pal::MUTED));
                            }
                        }
                    });
            });

        let was_showing_hex = self.show_hex;
        if self.hex_scroll_pending.is_some() { self.show_hex = true; }
        let scroll_target  = self.hex_scroll_pending;
        let highlight_copy = self.hex_highlight;

        if self.show_hex {
            egui::SidePanel::right("hexdump")
                .resizable(true)
                .default_width(760.0)
                .min_width(620.0)
                .frame(Frame {
                    inner_margin: Margin::same(30.0),
                    outer_margin: Margin::ZERO,
                    fill:         pal::PANEL,
                    stroke:       Stroke::new(1.0, pal::BORDER),
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("HEX DUMP").size(10.0).color(pal::MUTED).strong());
                        if let Some((off, len)) = highlight_copy {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("⚑ 0x{:08X} – 0x{:08X}", off, off + len))
                                    .size(11.0).color(pal::RED).monospace(),
                            );
                        }
                    });
                    if let Some(file) = self.files.get(self.sel_file) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&file.name).size(12.0).strong());
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("{} bytes", file.data.len())).size(11.0).color(pal::MUTED));
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("OFFSET    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  │ ASCII")
                                .monospace().size(11.0).color(pal::MUTED),
                        );
                        ui.add_space(2.0);
                        hex_view(ui, &file.data, scroll_target, highlight_copy);
                    } else {
                        ui.add_space(24.0);
                        ui.label(RichText::new("Load a binary file to inspect.").color(pal::MUTED).size(12.0));
                    }
                });
        }

        if was_showing_hex {
            self.hex_scroll_pending = None;
        }

        egui::CentralPanel::default()
            .frame(Frame {
                fill:         pal::BG,
                inner_margin: Margin::same(16.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in ["Metrics", "Distribution", "Anomalies", "Statistics"].iter().enumerate() {
                        let active = self.active_tab == i;
                        if ui.add(
                            egui::Button::new(
                                RichText::new(*tab).size(12.0)
                                    .color(if active { pal::RED } else { pal::MUTED })
                                    .strong(),
                            )
                            .fill(if active { pal::RED_LIGHT } else { pal::PANEL })
                            .stroke(Stroke::new(1.0, if active { pal::RED } else { pal::BORDER }))
                            .rounding(Rounding::same(5.0)),
                        ).clicked() {
                            self.active_tab = i;
                        }
                    }
                });
                ui.add_space(10.0);

                if self.files.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Load a binary and click ▶ Analyze").size(16.0).color(pal::MUTED));
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_source("central_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        if self.active_tab == 0 {
                            let analyzed: Vec<(usize, String, AnalysisResult)> = (0..self.files.len())
                                .filter_map(|idx| {
                                    let result = self.files[idx].result.clone()?;
                                    Some((idx, self.files[idx].name.clone(), result))
                                })
                                .collect();

                            if analyzed.is_empty() {
                                ui.centered_and_justified(|ui| {
                                    ui.label(RichText::new("Load a binary and click ▶ Analyze").size(16.0).color(pal::MUTED));
                                });
                            } else {
                                let n      = analyzed.len() as f32;
                                let avail  = ui.available_width();
                                let col_w  = ((avail - (n - 1.0) * 16.0) / n).max(180.0);
                                let k      = self.anomaly_k;

                                macro_rules! apply_jump {
                                    ($jump:expr) => {
                                        if let Some((fidx, off)) = $jump {
                                            let ws = analyzed.iter()
                                                .find(|(i, _, _)| *i == fidx)
                                                .map(|(_, _, r)| r.window_size.max(1))
                                                .unwrap_or(1);
                                            self.sel_file           = fidx;
                                            self.hex_scroll_pending = Some(off);
                                            self.hex_highlight      = Some((off, ws));
                                        }
                                    };
                                }

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            let t = &res.thresholds;
                                            (res.entropy.as_slice(), Some((t.entropy_mean, t.entropy_sd, k)), *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Entropy", "bits / symbol", &entries, col_w, 0)
                                };
                                apply_jump!(jump);
                                ui.add_space(4.0);

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            let t = &res.thresholds;
                                            (res.chi2.as_slice(), Some((t.chi2_mean, t.chi2_sd, k)), *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Chi²", "statistic", &entries, col_w, 1)
                                };
                                apply_jump!(jump);
                                ui.add_space(4.0);

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            let t = &res.thresholds;
                                            (res.serial_corr.as_slice(), Some((t.serial_mean, t.serial_sd, k)), *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Serial Correlation", "ρ", &entries, col_w, 2)
                                };
                                apply_jump!(jump);
                                ui.add_space(4.0);

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            (res.hamming.as_slice(), None, *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Hamming Weight", "bits / byte", &entries, col_w, 3)
                                };
                                apply_jump!(jump);
                                ui.add_space(4.0);

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            (res.bigram_scores.as_slice(), None, *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Bigram Uniqueness", "ratio", &entries, col_w, 4)
                                };
                                apply_jump!(jump);
                                ui.add_space(4.0);

                                let jump = {
                                    let entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> =
                                        analyzed.iter().map(|(idx, name, res)| {
                                            (res.trigram_scores.as_slice(), None, *idx, name.as_str())
                                        }).collect();
                                    plot_metrics_row(ui, "Trigram Uniqueness", "ratio", &entries, col_w, 5)
                                };
                                apply_jump!(jump);
                            }

                        } else if self.active_tab == 1 {
                            let analyzed: Vec<(usize, String, AnalysisResult)> = (0..self.files.len())
                                .filter_map(|idx| {
                                    let result = self.files[idx].result.clone()?;
                                    Some((idx, self.files[idx].name.clone(), result))
                                })
                                .collect();
                            if analyzed.is_empty() {
                                ui.centered_and_justified(|ui| {
                                    ui.label(RichText::new("Load a binary and click ▶ Analyze").size(16.0).color(pal::MUTED));
                                });
                            } else {
                                let n      = analyzed.len() as f32;
                                let avail  = ui.available_width();
                                let item_w = ((avail - (n - 1.0) * 8.0) / n).max(200.0);
                                ui.horizontal_top(|ui| {
                                    for (idx, file_name, result) in &analyzed {
                                        ui.vertical(|ui| {
                                            ui.set_max_width(item_w);
                                            plot_distribution(ui, result, file_name, *idx);
                                        });
                                        ui.add_space(8.0);
                                    }
                                });
                            }
                        } else {
                            for idx in 0..self.files.len() {
                                let file_name = self.files[idx].name.clone();
                                let result    = self.files[idx].result.clone();
                                let Some(result) = result else { continue };

                                egui::CollapsingHeader::new(
                                    RichText::new(&file_name).size(13.0).strong(),
                                )
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.add_space(4.0);
                                    match self.active_tab {
                                        2 => {
                                            if let Some(off) = suspicious_regions(ui, &result, &file_name, idx) {
                                                let ws = result.window_size.max(1);
                                                self.hex_highlight      = Some((off, ws));
                                                self.hex_scroll_pending = Some(off);
                                            }
                                        }
                                        3 => {
                                            statistics_tab(ui, &result, &file_name, idx);
                                        }
                                        _ => {}
                                    }
                                });
                                ui.add_space(8.0);
                            }
                        }
                    });
            });
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ELICIUNT")
            .with_inner_size([1440.0, 880.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ELICIUNT",
        opts,
        Box::new(|_cc| Box::new(App::default())),
    )
}