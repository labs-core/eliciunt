use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke};

use crate::analysis::{analyze_binary, load_binary_file, reapply_sigma_threshold};
use crate::constants::{ANOMALY_K_DEFAULT, WINDOW_SIZE_DEFAULT};
use crate::models::BinaryFile;
use crate::palette as pal;
use crate::ui::{
    hex::render_hex_view,
    plots::{render_byte_distribution, render_metric_row},
    regions::render_suspicious_regions,
    statistics::render_statistics_tab,
};

pub struct App {
    pub loaded_files:        Vec<BinaryFile>,
    pub window_size:         usize,
    pub selected_file_idx:   usize,
    pub active_tab:          usize,
    pub show_hex_panel:      bool,
    pub anomaly_threshold:   f64,
    pub hex_highlight:       Option<(usize, usize)>,
    pub hex_scroll_pending:  Option<usize>,
    // Counts down from 2 after a scroll is requested so the offset survives
    // the frame ordering gap (hex panel renders before the plots panel).
    pub hex_scroll_ttl:      u8,
}

impl Default for App {
    fn default() -> Self {
        Self {
            loaded_files:        Vec::new(),
            window_size:         WINDOW_SIZE_DEFAULT,
            selected_file_idx:   0,
            active_tab:          0,
            show_hex_panel:      false,
            anomaly_threshold:   ANOMALY_K_DEFAULT,
            hex_highlight:       None,
            hex_scroll_pending:  None,
            hex_scroll_ttl:      0,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        self.render_topbar(ctx);
        self.render_file_list_panel(ctx);
        self.render_hex_panel(ctx);
        self.render_central_panel(ctx);
    }
}

impl App {
    fn apply_theme(&self, ctx: &egui::Context) {
        let mut visuals = ctx.style().visuals.clone();
        visuals.override_text_color              = Some(pal::TEXT);
        visuals.panel_fill                       = pal::BG;
        visuals.window_fill                      = pal::PANEL;
        visuals.faint_bg_color                   = pal::RED_FAINT;
        visuals.extreme_bg_color                 = pal::BG;
        visuals.widgets.noninteractive.bg_fill   = pal::PANEL;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal::BORDER);
        visuals.widgets.inactive.bg_fill         = pal::RED_FAINT;
        visuals.widgets.hovered.bg_fill          = pal::RED_LIGHT;
        visuals.widgets.hovered.bg_stroke        = Stroke::new(1.0, pal::RED_MID);
        visuals.widgets.active.bg_fill           = pal::RED;
        visuals.selection.bg_fill                = pal::RED_LIGHT;
        visuals.selection.stroke                 = Stroke::new(1.0, pal::RED);
        ctx.set_visuals(visuals);
    }

    fn render_topbar(&mut self, ctx: &egui::Context) {
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
                        egui::Button::new(
                            RichText::new("+ Load Binary").size(12.0).color(Color32::WHITE),
                        )
                        .fill(pal::RED)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        if let Some(file) = load_binary_file() {
                            self.loaded_files.push(file);
                        }
                    }

                    if ui.add(
                        egui::Button::new(
                            RichText::new("▶ Analyze").size(12.0).color(Color32::WHITE),
                        )
                        .fill(pal::RED_MID)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        let (win, k) = (self.window_size, self.anomaly_threshold);
                        for file in &mut self.loaded_files {
                            file.result = Some(analyze_binary(&file.data, win, k));
                        }
                    }

                    ui.separator();
                    ui.label(RichText::new("Window:").size(12.0).color(pal::MUTED));
                    ui.scope(|ui| {
                        let vis = ui.visuals_mut();
                        vis.extreme_bg_color           = pal::BG;
                        vis.widgets.inactive.bg_fill   = pal::RED_FAINT;
                        vis.widgets.inactive.fg_stroke = Stroke::new(1.0, pal::RED);
                        vis.widgets.hovered.bg_fill    = pal::RED_LIGHT;
                        vis.widgets.active.bg_fill     = pal::BG;
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
                    let sigma_drag = ui.add(
                        egui::DragValue::new(&mut self.anomaly_threshold)
                            .clamp_range(0.5..=5.0_f64)
                            .speed(0.05)
                            .max_decimals(2),
                    );
                    if sigma_drag.changed() {
                        let k = self.anomaly_threshold;
                        for file in &mut self.loaded_files {
                            if let Some(ref mut result) = file.result {
                                reapply_sigma_threshold(result, k);
                            }
                        }
                    }

                    ui.separator();
                    let hex_toggle_label = if self.show_hex_panel { "✕ Hex" } else { "⟨/⟩ Hex" };
                    if ui.add(
                        egui::Button::new(
                            RichText::new(hex_toggle_label).size(12.0).color(
                                if self.show_hex_panel { Color32::WHITE } else { pal::RED },
                            ),
                        )
                        .fill(if self.show_hex_panel { pal::RED } else { pal::RED_FAINT })
                        .stroke(Stroke::new(1.0, pal::RED))
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        self.show_hex_panel = !self.show_hex_panel;
                    }

                    if let Some((highlight_offset, highlight_len)) = self.hex_highlight {
                        ui.separator();
                        ui.label(
                            RichText::new(format!("⚑ 0x{:08X} + {}B", highlight_offset, highlight_len))
                                .size(11.0).color(pal::RED).monospace(),
                        );
                        if ui.add(
                            egui::Button::new(RichText::new("✕").size(11.0).color(pal::MUTED))
                                .fill(pal::RED_FAINT)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(3.0)),
                        )
                        .on_hover_text("Clear highlight")
                        .clicked()
                        {
                            self.hex_highlight = None;
                        }
                    }
                });
            });
    }

    fn render_file_list_panel(&mut self, ctx: &egui::Context) {
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
                        if self.loaded_files.is_empty() {
                            ui.label(
                                RichText::new("No files loaded.").size(12.0).color(pal::MUTED),
                            );
                        }
                        for (idx, file) in self.loaded_files.iter().enumerate() {
                            let is_selected = idx == self.selected_file_idx;
                            if ui.add(egui::SelectableLabel::new(
                                is_selected,
                                RichText::new(&file.name).size(12.0),
                            )).clicked() {
                                self.selected_file_idx = idx;
                            }
                            if let Some(ref result) = file.result {
                                let suspicious_count = result.regions.iter().filter(|r| r.suspicious).count();
                                let summary_color    = if result.stats.ks_pvalue < 0.05 { pal::RED } else { pal::GREEN };
                                ui.label(
                                    RichText::new(format!(
                                        "  KS p={:.3}  χ²p={:.3}",
                                        result.stats.ks_pvalue,
                                        result.stats.global_chi2_p,
                                    ))
                                    .size(10.0).color(summary_color),
                                );
                                if suspicious_count > 0 {
                                    ui.label(
                                        RichText::new(format!("  ⚠ {} regions", suspicious_count))
                                            .size(10.0).color(pal::RED),
                                    );
                                }
                            } else {
                                ui.label(
                                    RichText::new("  not analyzed").size(10.0).color(pal::MUTED),
                                );
                            }
                        }
                    });
            });
    }

    fn render_hex_panel(&mut self, ctx: &egui::Context) {
        let was_hex_visible = self.show_hex_panel;
        if self.hex_scroll_pending.is_some() {
            self.show_hex_panel = true;
        }
        let pending_scroll  = self.hex_scroll_pending;
        let active_highlight = self.hex_highlight;

        if self.show_hex_panel {
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
                        if let Some((off, len)) = active_highlight {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("⚑ 0x{:08X} – 0x{:08X}", off, off + len))
                                    .size(11.0).color(pal::RED).monospace(),
                            );
                        }
                    });
                    if let Some(file) = self.loaded_files.get(self.selected_file_idx) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&file.name).size(12.0).strong());
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("{} bytes", file.data.len()))
                                    .size(11.0).color(pal::MUTED),
                            );
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "OFFSET    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  │ ASCII",
                            )
                            .monospace().size(11.0).color(pal::MUTED),
                        );
                        ui.add_space(2.0);
                        render_hex_view(ui, &file.data, pending_scroll, active_highlight);
                    } else {
                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("Load a binary file to inspect.")
                                .color(pal::MUTED).size(12.0),
                        );
                    }
                });
        }

        if was_hex_visible {
            self.hex_scroll_pending = None;
        }
    }

    fn render_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame {
                fill:         pal::BG,
                inner_margin: Margin::same(16.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab_idx, tab_label) in
                        ["Metrics", "Distribution", "Anomalies", "Statistics"].iter().enumerate()
                    {
                        let is_active = self.active_tab == tab_idx;
                        if ui.add(
                            egui::Button::new(
                                RichText::new(*tab_label)
                                    .size(12.0)
                                    .color(if is_active { pal::RED } else { pal::MUTED })
                                    .strong(),
                            )
                            .fill(if is_active { pal::RED_LIGHT } else { pal::PANEL })
                            .stroke(Stroke::new(
                                1.0,
                                if is_active { pal::RED } else { pal::BORDER },
                            ))
                            .rounding(Rounding::same(5.0)),
                        ).clicked() {
                            self.active_tab = tab_idx;
                        }
                    }
                });
                ui.add_space(10.0);

                if self.loaded_files.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Load a binary and click ▶ Analyze")
                                .size(16.0).color(pal::MUTED),
                        );
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_source("central_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        self.render_active_tab(ui);
                    });
            });
    }

    fn render_active_tab(&mut self, ui: &mut egui::Ui) {
        let analyzed: Vec<(usize, String, crate::models::AnalysisResult)> = (0..self.loaded_files.len())
            .filter_map(|idx| {
                let result = self.loaded_files[idx].result.clone()?;
                Some((idx, self.loaded_files[idx].name.clone(), result))
            })
            .collect();

        if analyzed.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Load a binary and click ▶ Analyze")
                        .size(16.0).color(pal::MUTED),
                );
            });
            return;
        }

        match self.active_tab {
            0 => self.render_metrics_tab(ui, &analyzed),
            1 => self.render_distribution_tab(ui, &analyzed),
            _ => self.render_per_file_tab(ui),
        }
    }

    fn render_metrics_tab(
        &mut self,
        ui: &mut egui::Ui,
        analyzed: &[(usize, String, crate::models::AnalysisResult)],
    ) {
        let file_count   = analyzed.len() as f32;
        let available_w  = ui.available_width();
        let column_width = ((available_w - (file_count - 1.0) * 16.0) / file_count).max(180.0);
        let sigma_k      = self.anomaly_threshold;

        let mut apply_jump = |ui: &mut egui::Ui,
                               metric_name: &str,
                               metric_unit: &str,
                               entries: &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
                               row_id: usize| {
            if let Some((file_idx, byte_offset)) =
                render_metric_row(ui, metric_name, metric_unit, entries, column_width, row_id)
            {
                let window_size = analyzed
                    .iter()
                    .find(|(i, _, _)| *i == file_idx)
                    .map(|(_, _, r)| r.window_size.max(1))
                    .unwrap_or(1);
                self.selected_file_idx  = file_idx;
                self.hex_scroll_pending = Some(byte_offset);
                self.hex_highlight      = Some((byte_offset, window_size));
            }
            ui.add_space(4.0);
        };

        let entropy_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| {
                let band = Some((res.thresholds.entropy_mean, res.thresholds.entropy_sd, sigma_k));
                (res.entropy.as_slice(), band, *idx, name.as_str())
            })
            .collect();
        apply_jump(ui, "Entropy", "bits / symbol", &entropy_entries, 0);

        let chi2_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| {
                let band = Some((res.thresholds.chi2_mean, res.thresholds.chi2_sd, sigma_k));
                (res.chi2.as_slice(), band, *idx, name.as_str())
            })
            .collect();
        apply_jump(ui, "Chi²", "statistic", &chi2_entries, 1);

        let serial_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| {
                let band = Some((res.thresholds.serial_mean, res.thresholds.serial_sd, sigma_k));
                (res.serial_corr.as_slice(), band, *idx, name.as_str())
            })
            .collect();
        apply_jump(ui, "Serial Correlation", "ρ", &serial_entries, 2);

        let hamming_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| (res.hamming.as_slice(), None, *idx, name.as_str()))
            .collect();
        apply_jump(ui, "Hamming Weight", "bits / byte", &hamming_entries, 3);

        let bigram_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| (res.bigram_scores.as_slice(), None, *idx, name.as_str()))
            .collect();
        apply_jump(ui, "Bigram Uniqueness", "ratio", &bigram_entries, 4);

        let trigram_entries: Vec<(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)> = analyzed
            .iter()
            .map(|(idx, name, res)| (res.trigram_scores.as_slice(), None, *idx, name.as_str()))
            .collect();
        apply_jump(ui, "Trigram Uniqueness", "ratio", &trigram_entries, 5);
    }

    fn render_distribution_tab(
        &self,
        ui: &mut egui::Ui,
        analyzed: &[(usize, String, crate::models::AnalysisResult)],
    ) {
        let file_count   = analyzed.len() as f32;
        let available_w  = ui.available_width();
        let column_width = ((available_w - (file_count - 1.0) * 8.0) / file_count).max(200.0);

        ui.horizontal_top(|ui| {
            for (file_idx, file_name, result) in analyzed {
                ui.vertical(|ui| {
                    ui.set_max_width(column_width);
                    render_byte_distribution(ui, result, file_name, *file_idx);
                });
                ui.add_space(8.0);
            }
        });
    }

    fn render_per_file_tab(&mut self, ui: &mut egui::Ui) {
        for idx in 0..self.loaded_files.len() {
            let file_name = self.loaded_files[idx].name.clone();
            let Some(result) = self.loaded_files[idx].result.clone() else { continue };

            egui::CollapsingHeader::new(RichText::new(&file_name).size(13.0).strong())
                .default_open(true)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    match self.active_tab {
                        2 => {
                            if let Some(byte_offset) =
                                render_suspicious_regions(ui, &result, &file_name, idx)
                            {
                                let window_size          = result.window_size.max(1);
                                self.hex_highlight       = Some((byte_offset, window_size));
                                self.hex_scroll_pending  = Some(byte_offset);
                            }
                        }
                        3 => {
                            render_statistics_tab(ui, &result, &file_name, idx);
                        }
                        _ => {}
                    }
                });
            ui.add_space(8.0);
        }
    }
}