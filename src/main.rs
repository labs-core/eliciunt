
use eframe::egui;
use egui::{Color32, FontId, Frame, Margin, RichText, Rounding, Stroke, Vec2};
use egui_plot::{Bar, BarChart, Legend, Line, Plot, PlotPoints};
use rfd::FileDialog;
use std::{collections::HashMap, fs};

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
}


#[derive(Clone)]
struct RegionInsight {
    offset:      usize,
    entropy:     f64,
    chi2:        f64,
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
    bigram_scores:  Vec<[f64; 2]>,
    trigram_scores: Vec<[f64; 2]>,
    regions:        Vec<RegionInsight>,
}

impl Default for AnalysisResult {
    fn default() -> Self {
        Self {
            entropy:        Vec::new(),
            chi2:           Vec::new(),
            serial_corr:    Vec::new(),
            hamming:        Vec::new(),
            byte_dist:      [0.0; MAX_BYTE],
            bigram_scores:  Vec::new(),
            trigram_scores: Vec::new(),
            regions:        Vec::new(),
        }
    }
}

struct BinaryFile {
    name:   String,
    data:   Vec<u8>,
    result: Option<AnalysisResult>,
}


struct App {
    files:       Vec<BinaryFile>,
    window_size: usize,
    sel_file:    usize,
    active_tab:  usize,
}

impl Default for App {
    fn default() -> Self {
        Self {
            files:       Vec::new(),
            window_size: 512,
            sel_file:    0,
            active_tab:  0,
        }
    }
}

impl App {
    fn load_file() -> Option<BinaryFile> {
        let path = FileDialog::new().pick_file()?;
        let data = fs::read(&path).ok()?;
        Some(BinaryFile {
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            data,
            result: None,
        })
    }

    fn analyze(data: &[u8], w: usize) -> AnalysisResult {
        let mut r = AnalysisResult::default();
        if data.is_empty() { return r; }

        let mut hist = [0usize; MAX_BYTE];
        for &b in data { hist[b as usize] += 1; }
        for i in 0..MAX_BYTE {
            r.byte_dist[i] = hist[i] as f64 / data.len() as f64;
        }

        for offset in (0..data.len().saturating_sub(w)).step_by(w) {
            let s       = &data[offset..offset + w];
            let entropy = compute_entropy(s);
            let chi2    = compute_chi2(s);
            let serial  = serial_correlation(s);
            let hamming = hamming_weight(s);

            r.entropy.push([offset as f64, entropy]);
            r.chi2.push([offset as f64, chi2]);
            r.serial_corr.push([offset as f64, serial]);
            r.hamming.push([offset as f64, hamming]);
            r.bigram_scores.push([offset as f64, ngram_uniqueness(s, 2)]);
            r.trigram_scores.push([offset as f64, ngram_uniqueness(s, 3)]);

            let suspicious = entropy > 7.7 || chi2 < 220.0 || serial.abs() < 0.05;
            r.regions.push(RegionInsight {
                offset, entropy, chi2, serial_corr: serial, hamming, suspicious,
            });
        }
        r
    }
}


fn compute_entropy(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let len = data.len() as f64;
    hist.iter().filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / len; -p * p.log2() })
        .sum()
}

fn compute_chi2(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let expected = data.len() as f64 / 256.0;
    hist.iter().map(|&o| { let d = o as f64 - expected; d*d/expected }).sum()
}

fn serial_correlation(data: &[u8]) -> f64 {
    if data.len() < 2 { return 0.0; }
    let n = data.len() as f64;
    let (mut sum, mut sum_sq, mut serial_sum) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..data.len() {
        let x = data[i] as f64;
        let y = data[(i + 1) % data.len()] as f64;
        sum += x; sum_sq += x*x; serial_sum += x*y;
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
    let mut map = HashMap::<Vec<u8>, usize>::new();
    for i in 0..=(data.len() - n) {
        *map.entry(data[i..i+n].to_vec()).or_insert(0) += 1;
    }
    map.len() as f64 / (data.len() - n + 1) as f64
}

fn normalize(data: &[[f64; 2]]) -> (Vec<[f64; 2]>, f64, f64) {
    if data.is_empty() {
        return (Vec::new(), 0.0, 1.0);
    }
    let min = data.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let max = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let range = (max - min).max(f64::EPSILON);
    let pts = data.iter()
        .map(|p| [p[0], (p[1] - min) / range])
        .collect();
    (pts, min, max)
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

fn badge(ui: &mut egui::Ui, label: &str, fg: Color32, bg: Color32) {
    let (rect, _) = ui.allocate_at_least(
        ui.fonts(|f| {
            let w = f.glyph_width(&FontId::proportional(11.0), 'x') * label.len() as f32 + 18.0;
            Vec2::new(w, 20.0)
        }),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label,
        FontId::proportional(11.0), fg);
}


fn plot_metric(
    ui:           &mut egui::Ui,
    title:        &str,
    unit:         &str,
    data:         &[[f64; 2]],
    file_index:   usize,
    do_normalize: bool,
) {
    // Build the points that will actually be plotted.
    let (plot_pts, raw_min, raw_max) = if do_normalize {
        normalize(data)
    } else {
        // Pass through unchanged; min/max still computed for the label.
        let min = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let max = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        (data.to_vec(), min, max)
    };

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong().color(pal::TEXT).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let range_label = if do_normalize {
                    format!("{unit}  [{:.3} – {:.3}]  normalised", raw_min, raw_max)
                } else {
                    format!("{unit}  [{:.3} – {:.3}]", raw_min, raw_max)
                };
                ui.label(RichText::new(range_label).size(11.0).color(pal::MUTED));
            });
        });
        ui.add_space(6.0);

        let line = Line::new(PlotPoints::from(plot_pts))
            .color(pal::RED)
            .width(1.6)
            .name(title);

        Plot::new(format!("{}_{}", title, file_index))
            .height(160.0)
            .legend(Legend::default())
            .show_axes([true, true])
            .show_grid([true, true])
            .auto_bounds([true, true].into())
            .set_margin_fraction(egui::Vec2::new(0.02, 0.1))
            .x_axis_formatter(|mark, _, _| {
                format!("0x{:X}", mark.value as usize)
            })
            .y_axis_formatter(|mark, _, _| {
                format!("{:.3}", mark.value)
            })
             .label_formatter(move |name, point| {
                let norm_y = point.y;   // value as plotted (0–1 when normalised)
                if do_normalize {
                    let real_y = norm_y * (raw_max - raw_min) + raw_min;
                    format!(
                        "offset: 0x{:X}\n{}: {:.4} (norm)\nraw:    {:.4}",
                        point.x as usize, name, norm_y, real_y
                    )
                } else {
                    format!(
                        "offset: 0x{:X}\n{}: {:.4}",
                        point.x as usize, name, norm_y
                    )
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.line(line);
            });
    });
}

fn plot_distribution(ui: &mut egui::Ui, result: &AnalysisResult, file_index: usize) {
    // Normalise the byte distribution too (max bar = 1.0)
    let max_freq = result.byte_dist.iter().cloned().fold(0.0f64, f64::max).max(f64::EPSILON);

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Byte Frequency Distribution")
                    .strong().color(pal::TEXT).size(13.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("0x00 – 0xFF  ·  normalised  [max = {:.4}]", max_freq))
                        .size(11.0).color(pal::MUTED),
                );
            });
        });
        ui.add_space(6.0);

        let bars: Vec<Bar> = (0..MAX_BYTE)
            .map(|i| {
                Bar::new(i as f64, result.byte_dist[i] / max_freq)
                    .width(0.8)
                    .fill(pal::RED_MID)
                    .stroke(Stroke::new(0.0, pal::RED))
                    // per-bar name shows hex byte value in tooltip
                    .name(format!("0x{:02X}", i))
            })
            .collect();

        let chart = BarChart::new(bars)
            .color(pal::RED)
            .name("freq");

        Plot::new(format!("byte_dist_{}", file_index))
            .height(200.0)
            .show_grid([true, true])
            .auto_bounds([true, true].into())
            .x_axis_formatter(|mark, _, _| format!("0x{:02X}", mark.value as u8))
            .y_axis_formatter(|mark, _, _| format!("{:.2}", mark.value))
            .label_formatter(move |name, point| {
                let real = point.y * max_freq;
                format!("byte: {}\nrel. freq: {:.4}", name, real)
            })
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(chart);
            });
    });
}

fn suspicious_regions(ui: &mut egui::Ui, result: &AnalysisResult) {
    let suspicious: Vec<&RegionInsight> =
        result.regions.iter().filter(|r| r.suspicious).take(64).collect();

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Anomalous Regions").strong().color(pal::TEXT).size(13.0));
            ui.add_space(6.0);
            badge(ui, &format!("{} flagged", suspicious.len()), pal::RED, pal::RED_LIGHT);
        });
        ui.add_space(8.0);

        if suspicious.is_empty() {
            ui.label(RichText::new("No anomalies detected.").color(pal::MUTED).size(12.0));
            return;
        }

        let cols = ["Offset", "Entropy", "Chi²", "Serial ρ", "Hamming", "Status"];
        egui::Grid::new("regions_grid")
            .num_columns(cols.len())
            .striped(true)
            .min_col_width(84.0)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for col in &cols {
                    ui.label(RichText::new(*col).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for r in &suspicious {
                    ui.label(RichText::new(format!("0x{:08X}", r.offset)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.4}", r.entropy)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.2}",  r.chi2)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.4}", r.serial_corr)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.4}", r.hamming)).monospace().size(12.0));

                    let (rect, _) = ui.allocate_exact_size(Vec2::new(80.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, Rounding::same(4.0), pal::RED_LIGHT);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER,
                        "⚠ Suspicious", FontId::proportional(11.0), pal::RED);
                    ui.end_row();
                }
            });
    });
}

fn hex_view(ui: &mut egui::Ui, data: &[u8]) {
    egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let end = data.len().min(HEX_WIDTH * 512);
            for line_offset in (0..end).step_by(HEX_WIDTH) {
                let slice_end = (line_offset + HEX_WIDTH).min(data.len());
                let slice     = &data[line_offset..slice_end];
                let mut hex   = format!("{:08X}  ", line_offset);
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
                ui.label(RichText::new(&hex).monospace().size(12.0).color(pal::TEXT));
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

        // ── Top bar ──────────────────────────────────────────
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
                    ui.label(RichText::new("").size(12.0).color(pal::MUTED));
                    ui.separator();

                    if ui.add(
                        egui::Button::new(RichText::new("+ Load Binary").size(12.0).color(Color32::WHITE))
                            .fill(pal::RED).stroke(Stroke::NONE).rounding(Rounding::same(5.0))
                    ).clicked() {
                        if let Some(file) = Self::load_file() { self.files.push(file); }
                    }

                    if ui.add(
                        egui::Button::new(RichText::new("▶ Analyze").size(12.0).color(Color32::WHITE))
                            .fill(pal::RED_MID).stroke(Stroke::NONE).rounding(Rounding::same(5.0))
                    ).clicked() {
                        let w = self.window_size;
                        for file in &mut self.files {
                            file.result = Some(Self::analyze(&file.data, w));
                        }
                    }

                    ui.separator();
                    ui.label(RichText::new("Window:").size(12.0).color(pal::MUTED));
                    ui.add(egui::Slider::new(&mut self.window_size, 128..=8192).show_value(true).suffix(" B"));
                });
            });

        // ── Left: file list ──────────────────────────────────
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
                            )).clicked() { self.sel_file = idx; }

                            if let Some(ref r) = file.result {
                                let n = r.regions.iter().filter(|r| r.suspicious).count();
                                if n > 0 {
                                    ui.label(RichText::new(format!("  ⚠ {} regions", n)).size(10.0).color(pal::RED));
                                }
                            } else {
                                ui.label(RichText::new("  not analyzed").size(10.0).color(pal::MUTED));
                            }
                        }
                    });
            });

        // ── Right: hex dump ──────────────────────────────────
        egui::SidePanel::right("hexdump")
            .resizable(true)
            .default_width(520.0)
            .frame(Frame {
                inner_margin: Margin::same(12.0),
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(RichText::new("HEX DUMP").size(10.0).color(pal::MUTED).strong());
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
                    hex_view(ui, &file.data);
                } else {
                    ui.add_space(24.0);
                    ui.label(RichText::new("Load a binary file to inspect.").color(pal::MUTED).size(12.0));
                }
            });

        // ── Central: tabs ────────────────────────────────────
        egui::CentralPanel::default()
            .frame(Frame { fill: pal::BG, inner_margin: Margin::same(16.0), ..Default::default() })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in ["Metrics", "Distribution", "Anomalies"].iter().enumerate() {
                        let active = self.active_tab == i;
                        if ui.add(
                            egui::Button::new(
                                RichText::new(*tab).size(12.0)
                                    .color(if active { pal::RED } else { pal::MUTED })
                                    .strong(),
                            )
                            .fill(if active { pal::RED_LIGHT } else { pal::PANEL })
                            .stroke(Stroke::new(1.0, if active { pal::RED } else { pal::BORDER }))
                            .rounding(Rounding::same(5.0))
                        ).clicked() { self.active_tab = i; }
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
                                    0 => {
                                        // (title, data, unit, normalize)
                                        // Serial correlation spans [-1, 1] and its sign is
                                        // meaningful — do NOT normalise it.
                                        let metrics: &[(&str, &[[f64; 2]], &str, bool)] = &[
                                            ("Entropy",            &result.entropy,        "bits / symbol", true),
                                            ("Chi²",               &result.chi2,           "statistic",     true),
                                            ("Serial Correlation", &result.serial_corr,    "ρ",             false),
                                            ("Hamming Weight",     &result.hamming,        "bits / byte",   true),
                                            ("Bigram Uniqueness",  &result.bigram_scores,  "ratio",         true),
                                            ("Trigram Uniqueness", &result.trigram_scores, "ratio",         true),
                                        ];
                                        for (title, data, unit, norm) in metrics {
                                            plot_metric(ui, title, unit, data, idx, *norm);
                                            ui.add_space(4.0);
                                        }
                                    }
                                    1 => { plot_distribution(ui, &result, idx); }
                                    2 => { suspicious_regions(ui, &result); }
                                    _ => {}
                                }
                            });
                            ui.add_space(8.0);
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
        Box::new(|_| Box::new(App::default())),
    )
}