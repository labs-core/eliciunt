use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke, Vec2};

use crate::analysis::{analyze_binary, load_binary_file, reapply_sigma_threshold};
use crate::constants::{ANOMALY_K_DEFAULT, WINDOW_SIZE_DEFAULT};
use crate::models::{BinaryFile, HexBookmark, HexSelectionState};
use crate::palette as pal;
use crate::ui::{
    hex::render_hex_view,
    plots::{render_byte_distribution, render_metric_row},
    regions::render_suspicious_regions,
    statistics::render_statistics_tab,
};

// ─── preset palette offered in the bookmark dialog ───────────────────────────
const BOOKMARK_COLOR_PRESETS: &[(Color32, &str)] = &[
    (Color32::from_rgb(220,  80,  80), "Red"),
    (Color32::from_rgb( 80, 180,  80), "Green"),
    (Color32::from_rgb( 80, 140, 220), "Blue"),
    (Color32::from_rgb(220, 180,  50), "Yellow"),
    (Color32::from_rgb(200,  90, 200), "Purple"),
    (Color32::from_rgb( 60, 200, 200), "Cyan"),
    (Color32::from_rgb(230, 140,  50), "Orange"),
    (Color32::from_rgb(180, 180, 180), "Grey"),
];

// ─── bookmark creation dialog state ──────────────────────────────────────────

#[derive(Default)]
struct BookmarkDialog {
    open:           bool,
    pending_start:  usize,
    pending_len:    usize,
    label_buf:      String,
    color_idx:      usize,
}

impl BookmarkDialog {
    fn open_for(&mut self, start: usize, len: usize) {
        self.pending_start = start;
        self.pending_len   = len;
        self.label_buf     = format!("0x{:X}", start);
        self.color_idx     = 0;
        self.open          = true;
    }

    /// Draw the dialog window. Returns `Some(bookmark)` when the user confirms.
    fn show(&mut self, ctx: &egui::Context) -> Option<HexBookmark> {
        if !self.open {
            return None;
        }
        let mut confirmed: Option<HexBookmark> = None;
        let mut should_close = false;

        egui::Window::new("📌 Create Bookmark")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(Frame {
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                inner_margin: Margin::same(16.0),
                rounding:     Rounding::same(8.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.set_min_width(340.0);

                // ── range info ────────────────────────────────────────────
                ui.label(
                    RichText::new(format!(
                        "Range:  0x{:08X} – 0x{:08X}  ({} bytes)",
                        self.pending_start,
                        self.pending_start + self.pending_len,
                        self.pending_len,
                    ))
                    .monospace()
                    .size(11.0)
                    .color(pal::MUTED),
                );
                ui.add_space(10.0);

                // ── label input ───────────────────────────────────────────
                ui.label(RichText::new("Label").size(12.0).color(pal::TEXT));
                ui.add_space(4.0);
                let text_edit = egui::TextEdit::singleline(&mut self.label_buf)
                    .desired_width(300.0)
                    .font(egui::TextStyle::Monospace);
                ui.add(text_edit);
                ui.add_space(10.0);

                // ── colour picker ─────────────────────────────────────────
                ui.label(RichText::new("Colour").size(12.0).color(pal::TEXT));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (idx, (color, name)) in BOOKMARK_COLOR_PRESETS.iter().enumerate() {
                        let selected = idx == self.color_idx;
                        let swatch_size = Vec2::splat(22.0);
                        let (rect, response) =
                            ui.allocate_exact_size(swatch_size, egui::Sense::click());

                        let border_color = if selected {
                            Color32::WHITE
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 60)
                        };
                        ui.painter().rect(
                            rect,
                            Rounding::same(if selected { 5.0 } else { 3.0 }),
                            *color,
                            Stroke::new(if selected { 2.0 } else { 1.0 }, border_color),
                        );
                        if response.clicked() {
                            self.color_idx = idx;
                        }
                        response.on_hover_text(*name);
                        ui.add_space(3.0);
                    }
                });
                ui.add_space(14.0);

                // ── preview ───────────────────────────────────────────────
                let chosen_color = BOOKMARK_COLOR_PRESETS[self.color_idx].0;
                let preview_fill = Color32::from_rgba_unmultiplied(
                    chosen_color.r(), chosen_color.g(), chosen_color.b(), 40,
                );
                Frame::none()
                    .fill(preview_fill)
                    .stroke(Stroke::new(1.0, chosen_color))
                    .rounding(Rounding::same(4.0))
                    .inner_margin(Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(
                                if self.label_buf.trim().is_empty() {
                                    "(unnamed)".to_owned()
                                } else {
                                    self.label_buf.clone()
                                }
                            )
                            .size(11.0)
                            .color(chosen_color),
                        );
                    });
                ui.add_space(12.0);

                // ── action buttons ────────────────────────────────────────
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(
                            RichText::new("✓  Add Bookmark").size(12.0).color(Color32::WHITE),
                        )
                        .fill(pal::RED)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        let label = if self.label_buf.trim().is_empty() {
                            format!("0x{:X}", self.pending_start)
                        } else {
                            self.label_buf.trim().to_owned()
                        };
                        confirmed = Some(HexBookmark {
                            start: self.pending_start,
                            len:   self.pending_len,
                            label,
                            color: chosen_color,
                        });
                        should_close = true;
                    }
                    ui.add_space(8.0);
                    if ui.add(
                        egui::Button::new(RichText::new("✕  Cancel").size(12.0).color(pal::MUTED))
                            .fill(pal::RED_FAINT)
                            .stroke(Stroke::new(1.0, pal::BORDER))
                            .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        should_close = true;
                    }
                });
            });

        if should_close {
            self.open = false;
        }
        confirmed
    }
}

// ─── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    pub loaded_files:       Vec<BinaryFile>,
    pub window_size:        usize,
    pub selected_file_idx:  usize,
    pub active_tab:         usize,
    pub show_hex_panel:     bool,
    pub anomaly_threshold:  f64,
    pub hex_highlight:      Option<(usize, usize)>,
    pub hex_scroll_pending: Option<usize>,
    #[allow(dead_code)]
    pub hex_scroll_ttl:     u8,

    // ── bookmark state ────────────────────────────────────────────────────
    /// All confirmed bookmarks.
    pub bookmarks:          Vec<HexBookmark>,
    /// Tracks the in-progress drag selection inside the hex view.
    hex_selection:          HexSelectionState,
    /// Modal dialog for naming / colouring a new bookmark.
    bookmark_dialog:        BookmarkDialog,
    /// Whether to show the bookmark list side-panel.
    show_bookmark_panel:    bool,
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
            bookmarks:           Vec::new(),
            hex_selection:       HexSelectionState::default(),
            bookmark_dialog:     BookmarkDialog::default(),
            show_bookmark_panel: false,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        self.render_topbar(ctx);
        self.render_file_list_panel(ctx);
        self.render_hex_panel(ctx);
        self.render_bookmark_panel(ctx);
        self.render_central_panel(ctx);

        // Show the modal bookmark creation dialog (floats above everything).
        if let Some(new_bm) = self.bookmark_dialog.show(ctx) {
            self.bookmarks.push(new_bm);
            self.hex_selection.clear();
        }
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

                    // ── bookmarks toggle button ───────────────────────────
                    ui.add_space(4.0);
                    let bm_label = if self.show_bookmark_panel {
                        format!("✕ Bookmarks ({})", self.bookmarks.len())
                    } else {
                        format!("📌 Bookmarks ({})", self.bookmarks.len())
                    };
                    if ui.add(
                        egui::Button::new(
                            RichText::new(&bm_label).size(12.0).color(
                                if self.show_bookmark_panel { Color32::WHITE } else { pal::RED },
                            ),
                        )
                        .fill(if self.show_bookmark_panel { pal::RED } else { pal::RED_FAINT })
                        .stroke(Stroke::new(1.0, pal::RED))
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        self.show_bookmark_panel = !self.show_bookmark_panel;
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

    // ── bookmark list panel ───────────────────────────────────────────────────

    fn render_bookmark_panel(&mut self, ctx: &egui::Context) {
        if !self.show_bookmark_panel {
            return;
        }

        // Collect indices to remove (can't mutate while iterating).
        let mut remove_idx: Option<usize> = None;
        // Collect jump requests.
        let mut jump_to: Option<usize> = None;

        egui::SidePanel::left("bookmark_panel")
            .resizable(true)
            .default_width(220.0)
            .frame(Frame {
                inner_margin: Margin::same(12.0),
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(RichText::new("BOOKMARKS").size(10.0).color(pal::MUTED).strong());
                ui.add_space(8.0);

                if self.bookmarks.is_empty() {
                    ui.label(
                        RichText::new("No bookmarks yet.\nDrag-select bytes in the Hex panel.")
                            .size(11.0)
                            .color(pal::MUTED),
                    );
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_source("bookmark_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (idx, bm) in self.bookmarks.iter().enumerate() {
                            let fill = Color32::from_rgba_unmultiplied(
                                bm.color.r(), bm.color.g(), bm.color.b(), 30,
                            );
                            Frame::none()
                                .fill(fill)
                                .stroke(Stroke::new(1.0, bm.color))
                                .rounding(Rounding::same(5.0))
                                .inner_margin(Margin::same(8.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Colour dot
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(10.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(), 5.0, bm.color,
                                        );
                                        ui.label(
                                            RichText::new(&bm.label)
                                                .size(12.0)
                                                .strong()
                                                .color(bm.color),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "0x{:08X} – 0x{:08X}\n{} bytes",
                                            bm.start,
                                            bm.end(),
                                            bm.len,
                                        ))
                                        .monospace()
                                        .size(10.0)
                                        .color(pal::MUTED),
                                    );
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        if ui.add(
                                            egui::Button::new(
                                                RichText::new("⤴ Jump").size(10.0).color(pal::TEXT),
                                            )
                                            .fill(pal::RED_FAINT)
                                            .stroke(Stroke::new(1.0, pal::BORDER))
                                            .rounding(Rounding::same(3.0)),
                                        ).clicked() {
                                            jump_to = Some(bm.start);
                                        }
                                        ui.add_space(4.0);
                                        if ui.add(
                                            egui::Button::new(
                                                RichText::new("✕").size(10.0).color(pal::MUTED),
                                            )
                                            .fill(pal::RED_FAINT)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(3.0)),
                                        )
                                        .on_hover_text("Remove bookmark")
                                        .clicked()
                                        {
                                            remove_idx = Some(idx);
                                        }
                                    });
                                });
                            ui.add_space(6.0);
                        }
                    });
            });

        // Apply deferred mutations.
        if let Some(idx) = remove_idx {
            self.bookmarks.remove(idx);
        }
        if let Some(offset) = jump_to {
            self.show_hex_panel     = true;
            self.hex_scroll_pending = Some(offset);
            self.hex_highlight      = Some((offset, self.bookmarks
                .iter()
                .find(|b| b.start == offset)
                .map(|b| b.len)
                .unwrap_or(1)));
        }
    }

    // ── hex panel ─────────────────────────────────────────────────────────────

    fn render_hex_panel(&mut self, ctx: &egui::Context) {
        let was_hex_visible = self.show_hex_panel;
        if self.hex_scroll_pending.is_some() {
            self.show_hex_panel = true;
        }
        let pending_scroll   = self.hex_scroll_pending;
        let active_highlight = self.hex_highlight;

        if self.show_hex_panel {
            egui::SidePanel::right("hexdump")
                .resizable(true)
                .default_width(520.0)
                .min_width(520.0)
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
                        // Show current drag selection size.
                        if let Some((sel_start, sel_len)) = self.hex_selection.normalised() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "┊ selecting  0x{:08X} + {} B",
                                    sel_start, sel_len
                                ))
                                .size(11.0)
                                .color(Color32::from_rgb(80, 140, 220))
                                .monospace(),
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
                            // Hint text
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new("drag to select  ·  release to bookmark")
                                    .size(10.0).color(pal::MUTED).italics(),
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

                        if let Some((sel_start, sel_len)) = render_hex_view(
                            ui,
                            &file.data,
                            pending_scroll,
                            active_highlight,
                            &self.bookmarks,
                            &mut self.hex_selection,
                            self.bookmark_dialog.open
                        ) {
                            // Only open the dialog when it is not already open.
                            if !self.bookmark_dialog.open {
                                self.bookmark_dialog.open_for(sel_start, sel_len);
                            }
                        }
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

    // ── central panel ─────────────────────────────────────────────────────────

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
        let bookmarks    = &self.bookmarks;

        let mut apply_jump = |ui: &mut egui::Ui,
                               metric_name: &str,
                               metric_unit: &str,
                               entries: &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
                               row_id: usize| {
            if let Some((file_idx, byte_offset)) =
                render_metric_row(ui, metric_name, metric_unit, entries, column_width, row_id, bookmarks)
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