/**
 * @file      ui/statistics.rs
 * @brief     Global randomness-test results and per-metric statistics panel.
 * @details   Displays Kolmogorov-Smirnov, chi-squared, and Wald-Wolfowitz
 *            test outcomes alongside windowed metric summary statistics and
 *            an interpretation guide for all reported values.
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
use egui::RichText;

use crate::models::AnalysisResult;
use crate::palette as pal;
use crate::ui::widgets::{card_frame, truncate_filename};

pub fn render_statistics_tab(
    ui:         &mut egui::Ui,
    result:     &AnalysisResult,
    file_name:  &str,
    file_index: usize,
) {
    let stats = &result.stats;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Global Randomness Tests").strong().color(pal::TEXT).size(13.0),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display_name = truncate_filename(file_name, 40);
            ui.label(RichText::new(display_name).size(11.0).color(pal::MUTED).italics());
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new("H₀: bytes are i.i.d. Uniform{0,…,255}.  Significance level α = 0.05.")
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        let test_rows: &[(&str, String, f64, f64, &str, &str)] = &[
            (
                "Kolmogorov–Smirnov",
                format!("D = {:.5}", stats.ks_statistic),
                stats.ks_pvalue,
                0.05,
                "Non-uniform distribution",
                "Consistent with Uniform[0,255]",
            ),
            (
                "Chi² (global, df=255)",
                format!("χ² = {:.2}", stats.global_chi2),
                stats.global_chi2_p,
                0.05,
                "Non-uniform distribution",
                "Consistent with Uniform[0,255]",
            ),
            (
                "Wald–Wolfowitz runs",
                format!("Z = {:.4}", stats.runs_z_score),
                stats.runs_pvalue,
                0.05,
                "Non-random sequential structure",
                "Consistent with independent draws",
            ),
        ];

        egui::Grid::new(format!("global_tests_grid_{file_index}"))
            .num_columns(5)
            .min_col_width(100.0)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                for header in &["Test", "Statistic", "p-value", "Reject H₀?", "Interpretation"] {
                    ui.label(RichText::new(*header).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for (test_name, stat_str, p_value, alpha, reject_msg, accept_msg) in test_rows {
                    let null_rejected = *p_value < *alpha;
                    let verdict_color = if null_rejected { pal::RED } else { pal::GREEN };

                    ui.label(RichText::new(*test_name).size(12.0));
                    ui.label(RichText::new(stat_str).monospace().size(12.0));
                    ui.label(
                        RichText::new(if *p_value < 0.0001 {
                            "< 0.0001".to_owned()
                        } else {
                            format!("{:.4}", p_value)
                        })
                        .monospace().size(12.0).color(verdict_color),
                    );
                    ui.label(
                        RichText::new(if null_rejected { "Yes" } else { "No" })
                            .size(12.0).color(verdict_color).strong(),
                    );
                    ui.label(
                        RichText::new(if null_rejected { *reject_msg } else { *accept_msg })
                            .size(11.0).color(verdict_color),
                    );
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Mean per-window χ² p-value: {:.4}   (≈ 0.5 expected for uniform random data)",
                stats.mean_window_chi2p,
            ))
            .size(11.0).color(pal::MUTED),
        );
    });

    ui.add_space(8.0);

    card_frame().show(ui, |ui| {
        ui.label(
            RichText::new("Per-metric Statistics (windowed)")
                .strong().color(pal::TEXT).size(13.0),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Window size: {} bytes", result.window_size))
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        let metric_rows = [
            ("Entropy",              "bits / symbol", "[0, 8]",    &stats.entropy_stats),
            ("Reduced χ² (χ²/df)",  "dimensionless", "E = 1.0",   &stats.chi2_stats),
            ("Serial correlation",  "ρ",             "[−1, 1]",   &stats.serial_stats),
            ("Hamming weight",      "bits / byte",   "[0, 8]",    &stats.hamming_stats),
        ];

        egui::Grid::new("metric_stats_grid")
            .num_columns(7)
            .min_col_width(72.0)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                for header in &["Metric", "Unit", "Theoretical range", "Mean", "Std dev", "Min", "Max"] {
                    ui.label(RichText::new(*header).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();
                for (name, unit, theoretical_range, metric_stats) in &metric_rows {
                    ui.label(RichText::new(*name).size(12.0));
                    ui.label(RichText::new(*unit).size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(*theoretical_range).monospace().size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(format!("{:.5}", metric_stats.mean)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", metric_stats.sd)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", metric_stats.min)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", metric_stats.max)).monospace().size(12.0));
                    ui.end_row();
                }
            });
    });

    ui.add_space(8.0);

    card_frame().show(ui, |ui| {
        ui.label(RichText::new("Interpretation Guide").strong().color(pal::TEXT).size(13.0));
        ui.add_space(6.0);
        let guide_entries: &[(&str, &str)] = &[
            ("Entropy ≈ 8 bits/symbol",  "Near-maximal uncertainty — typical of compressed or encrypted data."),
            ("Entropy ≪ 8 bits/symbol",  "Significant redundancy — structured, sparse, or padding regions."),
            ("Reduced χ² ≈ 1.0",     "Byte distribution matches uniform — baseline for random/encrypted data."),
            ("Reduced χ² ≫ 1.0",     "Byte distribution deviates from uniform — likely structured content."),
            ("Serial |ρ| ≫ 0",           "Adjacent bytes are linearly correlated — sequential structure present."),
            ("Hamming weight ≈ 4.0",      "Expected for uniform random bytes (bit probability ≈ 0.5)."),
            ("Hamming weight ≈ 8.0",      "All bits set — consistent with erased flash (0xC3 fill)."),
            ("Hamming weight ≈ 0.0",      "All bits clear — consistent with zero-fill padding."),
            ("KS p-value < 0.05",         "Global CDF diverges from uniform — byte usage is skewed."),
            ("Runs test p-value < 0.05",  "Non-random sequential structure — long runs or alternating patterns."),
        ];
        for (term, explanation) in guide_entries {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(*term).monospace().size(12.0).color(pal::RED));
                ui.label(RichText::new("—").size(12.0).color(pal::MUTED));
                ui.label(RichText::new(*explanation).size(12.0));
            });
            ui.add_space(3.0);
        }
    });
}
