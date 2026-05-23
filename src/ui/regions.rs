use eframe::egui;
use egui::{RichText, Rounding, Stroke, Vec2};

use crate::models::AnalysisResult;
use crate::palette as pal;
use crate::ui::widgets::{card_frame, truncate_filename};

pub fn render_suspicious_regions(
    ui:         &mut egui::Ui,
    result:     &AnalysisResult,
    file_name:  &str,
    file_index: usize,
) -> Option<usize> {
    let flagged_regions: Vec<_> = result.regions.iter().filter(|r| r.suspicious).collect();
    let mut jump_to_offset: Option<usize> = None;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Anomalous Regions").strong().color(pal::TEXT).size(13.0),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("·").size(12.0).color(pal::BORDER));
            ui.add_space(2.0);
            let display_name = truncate_filename(file_name, 40);
            ui.label(RichText::new(display_name).size(11.0).color(pal::MUTED).italics());
            ui.add_space(6.0);

            let flagged_count    = flagged_regions.len();
            let (badge_fg, badge_bg) = if flagged_count > 0 {
                (pal::RED, pal::RED_LIGHT)
            } else {
                (pal::MUTED, pal::PANEL)
            };
            let badge_text  = format!("{flagged_count} flagged");
            let badge_font  = egui::FontId::proportional(11.0);
            let badge_width = ui.fonts(|f| {
                f.layout_no_wrap(badge_text.clone(), badge_font.clone(), badge_fg).size().x
            });
            let (badge_rect, _) =
                ui.allocate_at_least(Vec2::new(badge_width + 18.0, 20.0), egui::Sense::hover());
            ui.painter().rect_filled(badge_rect, Rounding::same(4.0), badge_bg);
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                &badge_text,
                badge_font,
                badge_fg,
            );
        });

        ui.add_space(4.0);
        let thr = &result.thresholds;
        ui.label(
            RichText::new(format!(
                "entropy μ={:.3} σ={:.3}  ·  χ² μ={:.1} σ={:.1}  ·  serial μ={:.4} σ={:.4}",
                thr.entropy_mean, thr.entropy_sd,
                thr.chi2_mean,    thr.chi2_sd,
                thr.serial_mean,  thr.serial_sd,
            ))
            .size(10.0).color(pal::MUTED),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Click a row to jump to that offset in the Hex Dump.")
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        if flagged_regions.is_empty() {
            ui.label(RichText::new("No anomalies detected.").color(pal::MUTED).size(12.0));
            return;
        }

        egui::Grid::new(format!("regions_grid_{file_index}"))
            .num_columns(7)
            .striped(true)
            .min_col_width(72.0)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for header in &["Offset", "Entropy", "Chi²", "p(χ²)", "Serial ρ", "Hamming", ""] {
                    ui.label(RichText::new(*header).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for region in &flagged_regions {
                    let chi2p_color = if region.chi2_pvalue < 0.05 { pal::RED } else { pal::GREEN };

                    let resp_offset  = ui.add(egui::SelectableLabel::new(
                        false,
                        RichText::new(format!("0x{:08X}", region.offset)).monospace().size(12.0),
                    ));
                    let resp_entropy = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:.4}", region.entropy)).monospace().size(12.0),
                        ).sense(egui::Sense::click()),
                    );
                    let resp_chi2    = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:.2}", region.chi2)).monospace().size(12.0),
                        ).sense(egui::Sense::click()),
                    );
                    let resp_chi2p   = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:.4}", region.chi2_pvalue))
                                .monospace().size(12.0).color(chi2p_color),
                        ).sense(egui::Sense::click()),
                    );
                    let resp_serial  = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:.4}", region.serial_corr)).monospace().size(12.0),
                        ).sense(egui::Sense::click()),
                    );
                    let resp_hamming = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:.4}", region.hamming)).monospace().size(12.0),
                        ).sense(egui::Sense::click()),
                    );
                    let jump_btn = ui.add(
                        egui::Button::new(RichText::new("⟶ Hex").size(11.0).color(pal::RED))
                            .fill(pal::RED_FAINT)
                            .stroke(Stroke::new(1.0, pal::RED_MID))
                            .rounding(Rounding::same(4.0))
                            .min_size(Vec2::new(56.0, 18.0)),
                    );

                    let any_secondary_click = resp_offset.secondary_clicked()
                        || resp_entropy.secondary_clicked()
                        || resp_chi2.secondary_clicked()
                        || resp_chi2p.secondary_clicked()
                        || resp_serial.secondary_clicked()
                        || resp_hamming.secondary_clicked();

                    if resp_offset.clicked() || jump_btn.clicked() || any_secondary_click {
                        jump_to_offset = Some(region.offset);
                    }
                    ui.end_row();
                }
            });
    });

    jump_to_offset
}
