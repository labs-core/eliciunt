use std::collections::HashMap;

use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke, Vec2};

use crate::analysis::{analyze_binary, load_binary_file, reapply_sigma_threshold};
use crate::constants::{ANOMALY_K_DEFAULT, WINDOW_SIZE_DEFAULT};
use crate::models::{BinaryFile, HexBookmark, HexSelectionState};
use crate::padding::{self, flatten_to_hex_bookmarks, MultiRangeBookmark, PaddingRegion};
use crate::palette as pal;
use crate::ui::{
    hex::render_hex_view,
    plots::{render_byte_distribution, render_manual_bookmark_form, render_metric_row},
    regions::render_suspicious_regions,
    statistics::render_statistics_tab,
};

/// Return value from `BookmarkDialog::show`.
enum BookmarkDialogResult {
    Single(HexBookmark),
    Multi(MultiRangeBookmark),
}

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
    (Color32::from_rgb(255, 120, 180), "Pink"),
    (Color32::from_rgb( 40, 200, 160), "Teal"),
    (Color32::from_rgb(160, 100, 240), "Violet"),
    (Color32::from_rgb(255, 200,  80), "Gold"),
    (Color32::from_rgb(100, 200, 255), "Sky"),
    (Color32::from_rgb(255, 140, 100), "Coral"),
    (Color32::from_rgb(140, 220, 100), "Lime"),
    (Color32::from_rgb(200, 160, 120), "Tan"),
];

// ─── bookmark creation dialog state ──────────────────────────────────────────

#[derive(Default)]
struct BookmarkDialog {
    open:           bool,
    pending_start:  usize,
    pending_len:    usize,
    label_buf:      String,
    color_idx:      usize,
    /// Ranges already confirmed via "＋ Keep Selecting"; the pending range is
    /// appended when the dialog is finally submitted.
    extra_ranges:   Vec<(usize, usize)>,
    /// When true, a new hex-view selection should be appended to this dialog
    /// rather than opening a fresh one.
    keep_selecting: bool,
}

impl BookmarkDialog {
    fn open_for(&mut self, start: usize, len: usize) {
        self.pending_start  = start;
        self.pending_len    = len;
        self.label_buf      = format!("0x{:X}", start);
        self.color_idx      = 0;
        self.extra_ranges   = Vec::new();
        self.keep_selecting = false;
        self.open           = true;
    }

    /// Append a new selection while keep_selecting is active.
    fn add_range(&mut self, start: usize, len: usize) {
        self.extra_ranges.push((self.pending_start, self.pending_len));
        self.pending_start = start;
        self.pending_len   = len;
    }

    /// Draw the dialog window.
    /// Returns `Some(result)` when the user confirms.
    fn show(&mut self, ctx: &egui::Context, used_colors: &[Color32]) -> Option<BookmarkDialogResult> {
        if !self.open {
            return None;
        }
        let mut confirmed: Option<BookmarkDialogResult> = None;
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

                // ── accumulated ranges list ───────────────────────────────────
                if !self.extra_ranges.is_empty() {
                    ui.label(
                        RichText::new(format!("Ranges ({})", self.extra_ranges.len() + 1))
                            .size(10.5).color(pal::MUTED).strong(),
                    );
                    ui.add_space(3.0);
                    for (i, &(s, l)) in self.extra_ranges.iter().enumerate() {
                        ui.label(
                            RichText::new(format!(
                                "  {}  0x{:08X} – 0x{:08X}  ({} B)",
                                i + 1, s, s + l, l
                            ))
                            .monospace().size(10.0).color(pal::MUTED),
                        );
                    }
                    ui.label(
                        RichText::new(format!(
                            "  {}  0x{:08X} – 0x{:08X}  ({} B)  ◀ current",
                            self.extra_ranges.len() + 1,
                            self.pending_start,
                            self.pending_start + self.pending_len,
                            self.pending_len,
                        ))
                        .monospace().size(10.0).color(pal::TEXT),
                    );
                    ui.add_space(8.0);
                } else {
                    // ── single range info ─────────────────────────────────────
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
                }

                // ── label input ───────────────────────────────────────────────
                ui.label(RichText::new("Label").size(12.0).color(pal::TEXT));
                ui.add_space(4.0);
                let text_edit = egui::TextEdit::singleline(&mut self.label_buf)
                    .desired_width(300.0)
                    .font(egui::TextStyle::Monospace);
                ui.add(text_edit);
                ui.add_space(10.0);

                // ── colour picker ─────────────────────────────────────────────
                ui.label(RichText::new("Colour").size(12.0).color(pal::TEXT));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (idx, (color, name)) in BOOKMARK_COLOR_PRESETS.iter().enumerate() {
                        let selected = idx == self.color_idx;
                        let taken    = used_colors.contains(color);
                        let swatch_size = Vec2::splat(22.0);
                        let (rect, response) =
                            ui.allocate_exact_size(swatch_size, egui::Sense::click());

                        // Dim taken colours; keep selected visible even if
                        // technically "taken" (it's the current bookmark's own).
                        let display_color = if taken && !selected {
                            Color32::from_rgba_unmultiplied(
                                color.r(), color.g(), color.b(), 55,
                            )
                        } else {
                            *color
                        };
                        let border_color = if selected {
                            Color32::WHITE
                        } else if taken {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 25)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 60)
                        };
                        ui.painter().rect(
                            rect,
                            Rounding::same(if selected { 5.0 } else { 3.0 }),
                            display_color,
                            Stroke::new(if selected { 2.0 } else { 1.0 }, border_color),
                        );
                        // Draw a small "×" over taken-but-unselected swatches.
                        if taken && !selected {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "×",
                                egui::FontId::proportional(13.0),
                                Color32::from_rgba_unmultiplied(255, 255, 255, 140),
                            );
                        }
                        if response.clicked() && !taken {
                            self.color_idx = idx;
                        }
                        response.on_hover_text(if taken {
                            format!("{name}  (already in use)")
                        } else {
                            name.to_string()
                        });
                        ui.add_space(3.0);
                    }
                });
                ui.add_space(14.0);

                // ── preview ───────────────────────────────────────────────────
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

                // ── action buttons ────────────────────────────────────────────
                ui.horizontal(|ui| {
                    // ── "✓ Add Bookmark" ──────────────────────────────────────
                    // Commits all accumulated ranges (extra_ranges + pending) as
                    // either a plain single-range HexBookmark (when keep_selecting
                    // was never used) or a MultiRangeBookmark (when ≥ 2 ranges
                    // were gathered).
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
                        let color = chosen_color;

                        if self.extra_ranges.is_empty() {
                            // No extra ranges — plain single-range bookmark.
                            confirmed = Some(BookmarkDialogResult::Single(HexBookmark {
                                start: self.pending_start,
                                len:   self.pending_len,
                                label,
                                color,
                            }));
                        } else {
                            // Build a MultiRangeBookmark from all accumulated ranges.
                            let mut all_ranges = self.extra_ranges.clone();
                            all_ranges.push((self.pending_start, self.pending_len));
                            all_ranges.sort_by_key(|&(s, _)| s);
                            let total: usize = all_ranges.iter().map(|&(_, l)| l).sum();
                            let count = all_ranges.len();
                            let regions = all_ranges
                                .into_iter()
                                .map(|(s, l)| PaddingRegion { start: s, len: l, fill_byte: 0 })
                                .collect();
                            confirmed = Some(BookmarkDialogResult::Multi(MultiRangeBookmark {
                                label: format!("{label}  ({total} B)  ×{count}"),
                                color,
                                regions,
                            }));
                        }
                        should_close = true;
                    }

                    ui.add_space(8.0);

                    // ── "Keep Selecting" button ───────────────────────────────
                    // Saves the current pending range into extra_ranges and
                    // keeps the dialog open so the user can drag-select another
                    // region in the hex view.  The active state is shown via a
                    // coloured border + indicator dot so it is always obvious
                    // whether the mode is armed.
                    let keep_label = if self.keep_selecting {
                        RichText::new("＋ Keep Selecting  ●")
                            .size(11.0).color(Color32::from_rgb(120, 210, 160))
                    } else {
                        RichText::new("＋ Keep Selecting")
                            .size(11.0).color(pal::MUTED)
                    };
                    if ui.add(
                        egui::Button::new(keep_label)
                            .fill(pal::RED_FAINT)
                            .stroke(Stroke::new(
                                1.0,
                                if self.keep_selecting {
                                    Color32::from_rgb(80, 180, 120)
                                } else {
                                    pal::BORDER
                                },
                            ))
                            .rounding(Rounding::same(5.0)),
                    )
                    .on_hover_text(
                        "Save this range and pick another region in the hex view.\n\
                         All saved ranges will become one grouped bookmark.",
                    )
                    .clicked() {
                        // Push the current pending range and arm the flag so the
                        // next hex-view drag appends rather than replacing.
                        self.extra_ranges.push((self.pending_start, self.pending_len));
                        self.pending_len    = 0;
                        self.keep_selecting = true;
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

                if self.keep_selecting {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("⟵  Drag-select another region in the hex view…")
                            .size(10.0).color(pal::MUTED).italics(),
                    );
                }
            });

        if should_close {
            self.open           = false;
            self.keep_selecting = false;
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

    // ── bookmark state ────────────────────────────────────────────────────────
    /// Manual single-range bookmarks (one contiguous byte span each).
    pub bookmarks:              Vec<HexBookmark>,
    /// Manual multi-range bookmarks created via "Keep Selecting" in the hex
    /// view dialog.  These are distinct from auto-padding bookmarks and are
    /// stored separately so they can be rendered as unified groups in the plot
    /// without flattening.
    pub user_multi_bookmarks:   Vec<MultiRangeBookmark>,
    /// Tracks the in-progress drag selection inside the hex view.
    hex_selection:              HexSelectionState,
    /// Modal dialog for naming / colouring a new bookmark.
    bookmark_dialog:            BookmarkDialog,
    /// Whether to show the bookmark list side-panel.
    show_bookmark_panel:        bool,

    // ── auto-bookmark (padding detection) state ───────────────────────────────
    /// Master switch: when false, auto-bookmarks are neither computed nor shown.
    pub auto_bookmarks_enabled: bool,
    /// Minimum contiguous run length (bytes) for a fill region to be flagged.
    pub auto_bookmark_min_run:  usize,
    /// Cached per-file auto-bookmarks stored as grouped MultiRangeBookmarks
    /// (one group per fill-byte class: 0xFF, 0x00).  Recomputed whenever the
    /// file is analysed or min_run changes.
    auto_bookmarks:             HashMap<usize, Vec<MultiRangeBookmark>>,
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
            bookmarks:              Vec::new(),
            user_multi_bookmarks:   Vec::new(),
            hex_selection:          HexSelectionState::default(),
            bookmark_dialog:        BookmarkDialog::default(),
            show_bookmark_panel:    false,
            auto_bookmarks_enabled: true,
            auto_bookmark_min_run:  512,
            auto_bookmarks:         HashMap::new(),
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
        // Dispatch the result into the correct storage bucket based on whether
        // the user created a plain single-range bookmark or a grouped
        // multi-range bookmark via "Keep Selecting".
        let used_colors: Vec<Color32> = {
            let mut colors: Vec<Color32> = self.bookmarks.iter().map(|b| b.color).collect();
            colors.extend(self.user_multi_bookmarks.iter().map(|m| m.color));
            colors
        };
        if let Some(result) = self.bookmark_dialog.show(ctx, &used_colors) {
            match result {
                BookmarkDialogResult::Single(bm)  => self.bookmarks.push(bm),
                BookmarkDialogResult::Multi(mbm)  => self.user_multi_bookmarks.push(mbm),
            }
            self.hex_selection.clear();
        }
    }
}

impl App {
    // ── bookmark helpers ──────────────────────────────────────────────────────

    /// Returns a flat `Vec<HexBookmark>` suitable for the hex view and any
    /// renderer that works on individual byte spans.
    ///
    /// Includes:
    ///   • Manual single-range bookmarks (`self.bookmarks`).
    ///   • User multi-range bookmarks flattened to one `HexBookmark` per
    ///     sub-region (needed so the hex view can highlight each span).
    ///   • Auto-detected padding bookmarks, also flattened, when the toggle
    ///     is on.
    fn effective_bookmarks(&self) -> Vec<HexBookmark> {
        let mut out = self.bookmarks.clone();
        // Flatten user multi-range bookmarks so every sub-region is highlighted
        // individually in the hex view.
        out.extend(flatten_to_hex_bookmarks(&self.user_multi_bookmarks));
        // Flatten auto padding bookmarks if enabled.
        if self.auto_bookmarks_enabled {
            if let Some(auto_multi) = self.auto_bookmarks.get(&self.selected_file_idx) {
                out.extend(flatten_to_hex_bookmarks(auto_multi));
            }
        }
        out
    }

    /// Returns only the single-range manual bookmarks for plot rendering.
    ///
    /// Multi-range groups (user and auto) are handled separately via
    /// `effective_multi_bookmarks()` so each group renders as a unified entity
    /// with a single label.  Including their flattened sub-regions here would
    /// cause every sub-region to draw its own label in the plot.
    fn effective_plot_bookmarks(&self) -> Vec<HexBookmark> {
        self.bookmarks.clone()
    }

    /// Returns a `Vec<MultiRangeBookmark>` for the plot renderer, preserving
    /// group identity so each group is drawn as a single logical entity.
    ///
    /// Includes:
    ///   • User multi-range bookmarks (created via "Keep Selecting").
    ///   • Auto-detected padding bookmark groups when the toggle is on.
    fn effective_multi_bookmarks(&self) -> Vec<MultiRangeBookmark> {
        let mut out = self.user_multi_bookmarks.clone();
        if self.auto_bookmarks_enabled {
            if let Some(auto_multi) = self.auto_bookmarks.get(&self.selected_file_idx) {
                out.extend(auto_multi.iter().cloned());
            }
        }
        out
    }

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
                    ui.label(RichText::new("ELICIO").size(18.0).strong().color(pal::RED));
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
                        let min_run  = self.auto_bookmark_min_run;
                        self.auto_bookmarks.clear();
                        for (idx, file) in self.loaded_files.iter_mut().enumerate() {
                            file.result = Some(analyze_binary(&file.data, win, k));
                            // detect_and_build_multi returns one MultiRangeBookmark
                            // per fill-byte class (up to two total), preserving the
                            // group structure for the plot renderer.
                            self.auto_bookmarks.insert(
                                idx,
                                padding::detect_and_build_multi(&file.data, min_run),
                            );
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

                    // ── bookmarks toggle button ───────────────────────────────
                    // Count shows total user-created bookmarks (single + multi).
                    ui.add_space(4.0);
                    let total_bm_count = self.bookmarks.len() + self.user_multi_bookmarks.len();
                    let bm_label = if self.show_bookmark_panel {
                        format!("✕ Bookmarks ({})", total_bm_count)
                    } else {
                        format!("📌 Bookmarks ({})", total_bm_count)
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

    fn render_bookmark_panel(&mut self, ctx: &egui::Context) {
        if !self.show_bookmark_panel {
            return;
        }

        // Collect deferred mutations so we don't borrow `self` inside the closure.
        let mut remove_single_idx: Option<usize> = None;
        let mut remove_multi_idx:  Option<usize> = None;
        let mut jump_to:           Option<usize> = None;
        let mut new_bookmark:      Option<HexBookmark> = None;

        // File length for the manual-range form's bounds check.
        let file_len = self
            .loaded_files
            .get(self.selected_file_idx)
            .map(|f| f.data.len())
            .unwrap_or(0);

        // Pre-compute auto-bookmark stats once outside the closure.
        // AFTER
        let auto_groups: Vec<MultiRangeBookmark> = self
            .auto_bookmarks
            .get(&self.selected_file_idx)
            .cloned()
            .unwrap_or_default();
        let auto_group_count  = auto_groups.len();
        let auto_region_count: usize = auto_groups.iter().map(|g| g.region_count()).sum();

        egui::SidePanel::left("bookmark_panel")
            .resizable(true)
            .default_width(240.0)
            .frame(Frame {
                inner_margin: Margin::same(12.0),
                fill:         pal::PANEL,
                stroke:       Stroke::new(1.0, pal::BORDER),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(RichText::new("BOOKMARKS").size(10.0).color(pal::MUTED).strong());
                ui.add_space(8.0);

                // ── manual range form ─────────────────────────────────────────
                if let Some(bm) = render_manual_bookmark_form(ui, file_len) {
                    new_bookmark = Some(bm);
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // ── auto-bookmark controls ────────────────────────────────────
                ui.label(RichText::new("AUTO PADDING BOOKMARKS").size(10.0).color(pal::MUTED).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let toggle_label = if self.auto_bookmarks_enabled {
                        RichText::new("⏄  Enabled").size(11.0).color(pal::GREEN)
                    } else {
                        RichText::new("○  Disabled").size(11.0).color(pal::MUTED)
                    };
                    if ui.add(
                        egui::Button::new(toggle_label)
                            .fill(if self.auto_bookmarks_enabled { pal::RED_FAINT } else { pal::PANEL })
                            .stroke(Stroke::new(1.0, if self.auto_bookmarks_enabled { pal::GREEN } else { pal::BORDER }))
                            .rounding(Rounding::same(4.0)),
                    )
                    .on_hover_text("Toggle automatic padding-boundary bookmarks")
                    .clicked()
                    {
                        self.auto_bookmarks_enabled = !self.auto_bookmarks_enabled;
                    }
                });
                ui.add_space(4.0);

                // Min-run slider; recompute auto-bookmarks as grouped
                // MultiRangeBookmarks when the user changes it.
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Min run:").size(11.0).color(pal::MUTED));
                    let drag = ui.add(
                        egui::DragValue::new(&mut self.auto_bookmark_min_run)
                            .clamp_range(64..=65536usize)
                            .speed(16.0)
                            .suffix(" B"),
                    );
                    if drag.changed() {
                        let min_run = self.auto_bookmark_min_run;
                        self.auto_bookmarks.clear();
                        for (idx, file) in self.loaded_files.iter().enumerate() {
                            self.auto_bookmarks.insert(
                                idx,
                                padding::detect_and_build_multi(&file.data, min_run),
                            );
                        }
                    }
                });
                ui.add_space(2.0);
                // Summary: N group(s), M region(s) total.
                ui.label(
                    RichText::new(format!(
                        "{auto_group_count} group(s)  ·  {auto_region_count} region(s)\
                         \n(grey = 0xFF erased flash  ·  blue = 0x00 zero-fill)"
                    ))
                    .size(10.0)
                    .color(pal::MUTED)
                    .italics(),
                );
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // ── no bookmarks at all yet ───────────────────────────────────
                let no_manual = self.bookmarks.is_empty() && self.user_multi_bookmarks.is_empty();
                let no_auto   = !self.auto_bookmarks_enabled || auto_group_count == 0;
                if no_manual && no_auto {
                    ui.label(
                        RichText::new(
                            "No bookmarks yet.\nDrag-select bytes in the Hex panel\nor enter a range above."
                        )
                        .size(11.0)
                        .color(pal::MUTED),
                    );
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_source("bookmark_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {

                        // ── Single-range manual bookmarks ─────────────────────
                        if !self.bookmarks.is_empty() {
                            ui.label(
                                RichText::new("Manual").size(10.0).color(pal::MUTED).strong(),
                            );
                            ui.add_space(4.0);
                        }
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
                                            remove_single_idx = Some(idx);
                                        }
                                    });
                                });
                            ui.add_space(6.0);
                        }

                        // ── Multi-range manual bookmarks ──────────────────────
                        // One card per group.  Sub-regions are listed compactly
                        // below the header so the user can see the full extent of
                        // the grouped bookmark without it being split across
                        // multiple unrelated cards.
                        if !self.user_multi_bookmarks.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new("Multi-range").size(10.0).color(pal::MUTED).strong(),
                            );
                            ui.add_space(4.0);
                        }
                        for (idx, mbm) in self.user_multi_bookmarks.iter().enumerate() {
                            let fill = Color32::from_rgba_unmultiplied(
                                mbm.color.r(), mbm.color.g(), mbm.color.b(), 30,
                            );
                            Frame::none()
                                .fill(fill)
                                .stroke(Stroke::new(1.0, mbm.color))
                                .rounding(Rounding::same(5.0))
                                .inner_margin(Margin::same(8.0))
                                .show(ui, |ui| {
                                    // Header row: colour dot + group label.
                                    ui.horizontal(|ui| {
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(10.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(), 5.0, mbm.color,
                                        );
                                        ui.painter().circle_stroke(
                                            dot_rect.center(),
                                            3.0,
                                            Stroke::new(1.0, mbm.color),
                                        );
                                        ui.label(
                                            RichText::new(&mbm.label)
                                                .size(12.0)
                                                .strong()
                                                .color(mbm.color),
                                        );
                                    });
                                    ui.add_space(3.0);
                                    // Sub-region list — compact mono text.
                                    for (i, region) in mbm.regions.iter().enumerate() {
                                        ui.label(
                                            RichText::new(format!(
                                                "  {}  0x{:08X} – 0x{:08X}  ({} B)",
                                                i + 1,
                                                region.start,
                                                region.end(),
                                                region.len,
                                            ))
                                            .monospace()
                                            .size(9.5)
                                            .color(pal::MUTED),
                                        );
                                    }
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        // Jump to the first sub-region.
                                        if let Some(first) = mbm.regions.first() {
                                            if ui.add(
                                                egui::Button::new(
                                                    RichText::new("⤴ Jump to first").size(10.0).color(pal::TEXT),
                                                )
                                                .fill(pal::RED_FAINT)
                                                .stroke(Stroke::new(1.0, pal::BORDER))
                                                .rounding(Rounding::same(3.0)),
                                            ).clicked() {
                                                jump_to = Some(first.start);
                                            }
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
                                        .on_hover_text("Remove group bookmark")
                                        .clicked()
                                        {
                                            remove_multi_idx = Some(idx);
                                        }
                                    });
                                });
                            ui.add_space(6.0);
                        }

                        // ── Auto (padding) bookmarks ──────────────────────────
                        // Rendered as grouped cards — one card per fill-byte
                        // class, with all sub-regions listed inside the card.
                        // There is no remove button: auto bookmarks are
                        // re-derived from the binary data and cannot be manually
                        // deleted (disable the toggle or increase min_run instead).
                        if self.auto_bookmarks_enabled && !auto_groups.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new("Auto — Padding Boundaries")
                                    .size(10.0).color(pal::MUTED).strong(),
                            );
                            ui.add_space(4.0);
                            for group in auto_groups {
                                let fill = Color32::from_rgba_unmultiplied(
                                    group.color.r(), group.color.g(), group.color.b(), 18,
                                );
                                Frame::none()
                                    .fill(fill)
                                    .stroke(Stroke::new(1.0,
                                        Color32::from_rgba_unmultiplied(
                                            group.color.r(), group.color.g(), group.color.b(), 120,
                                        ),
                                    ))
                                    .rounding(Rounding::same(5.0))
                                    .inner_margin(Margin::same(7.0))
                                    .show(ui, |ui| {
                                        // Group header.
                                        ui.horizontal(|ui| {
                                            let (dot_rect, _) = ui.allocate_exact_size(
                                                Vec2::splat(8.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                dot_rect.center(), 4.0, group.color,
                                            );
                                            ui.label(
                                                RichText::new(&group.label)
                                                    .size(11.0)
                                                    .color(group.color),
                                            );
                                        });
                                        ui.add_space(3.0);
                                        // List up to 5 sub-regions; fold the rest.
                                        let show_n = group.regions.len().min(5);
                                        for region in &group.regions[..show_n] {
                                            ui.label(
                                                RichText::new(format!(
                                                    "  0x{:08X} – 0x{:08X}",
                                                    region.start,
                                                    region.end(),
                                                ))
                                                .monospace()
                                                .size(9.5)
                                                .color(pal::MUTED),
                                            );
                                        }
                                        if group.regions.len() > 5 {
                                            ui.label(
                                                RichText::new(format!(
                                                    "  … and {} more",
                                                    group.regions.len() - 5
                                                ))
                                                .size(9.5)
                                                .color(pal::MUTED)
                                                .italics(),
                                            );
                                        }
                                        ui.add_space(3.0);
                                        // Jump to the first sub-region of the group.
                                        if let Some(first) = group.regions.first() {
                                            if ui.add(
                                                egui::Button::new(
                                                    RichText::new("⤴ Jump").size(10.0).color(pal::TEXT),
                                                )
                                                .fill(pal::RED_FAINT)
                                                .stroke(Stroke::new(1.0, pal::BORDER))
                                                .rounding(Rounding::same(3.0)),
                                            ).clicked() {
                                                jump_to = Some(first.start);
                                            }
                                        }
                                    });
                                ui.add_space(5.0);
                            }
                        }
                    });
            });

        // ── apply deferred mutations ──────────────────────────────────────────
        if let Some(bm) = new_bookmark {
            self.bookmarks.push(bm);
        }
        if let Some(idx) = remove_single_idx {
            self.bookmarks.remove(idx);
        }
        if let Some(idx) = remove_multi_idx {
            self.user_multi_bookmarks.remove(idx);
        }
        if let Some(offset) = jump_to {
            self.show_hex_panel     = true;
            self.hex_scroll_pending = Some(offset);

            // Search single-range bookmarks first.
            let bm_len_single = self.bookmarks
                .iter()
                .find(|b| b.start == offset)
                .map(|b| b.len);

            // Then search user multi-range bookmarks (match on first sub-region).
            let bm_len_multi = self.user_multi_bookmarks.iter()
                .flat_map(|mbm| mbm.regions.iter())
                .find(|r| r.start == offset)
                .map(|r| r.len);

            // Then search auto bookmark groups.
            let bm_len_auto = self.auto_bookmarks
                .get(&self.selected_file_idx)
                .and_then(|groups| {
                    groups.iter()
                        .flat_map(|g| g.regions.iter())
                        .find(|r| r.start == offset)
                        .map(|r| r.len)
                });

            let bm_len = bm_len_single
                .or(bm_len_multi)
                .or(bm_len_auto)
                .unwrap_or(1);

            self.hex_highlight = Some((offset, bm_len));
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

                        let eff_bm = self.effective_bookmarks();
                        if let Some((sel_start, sel_len)) = render_hex_view(
                            ui,
                            &file.data,
                            pending_scroll,
                            active_highlight,
                            &eff_bm,
                            &mut self.hex_selection,
                            self.bookmark_dialog.open
                        ) {
                            // When keep_selecting is armed, append the new selection
                            // to the open dialog instead of opening a new one.
                            if self.bookmark_dialog.open && self.bookmark_dialog.keep_selecting {
                                self.bookmark_dialog.add_range(sel_start, sel_len);
                            } else if !self.bookmark_dialog.open {
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
        let sigma_k = self.anomaly_threshold;

        // Both snapshot functions return owned Vecs so the closure below can
        // borrow them by reference without conflicting with the mutable self
        // borrow inside the closure body.
        let bookmarks_for_plot    = self.effective_plot_bookmarks();
        let multi_for_plot        = self.effective_multi_bookmarks();
        let bookmarks             = &bookmarks_for_plot;
        let multi_bookmarks       = &multi_for_plot;

        let mut apply_jump = |ui: &mut egui::Ui,
                               metric_name: &str,
                               metric_unit: &str,
                               entries: &[(&[[f64; 2]], Option<(f64, f64, f64)>, usize, &str)],
                               row_id: usize| {
            if let Some((file_idx, byte_offset)) =
                render_metric_row(
                    ui,
                    metric_name,
                    metric_unit,
                    entries,
                    column_width,
                    row_id,
                    bookmarks,
                    multi_bookmarks,   // ← was missing before refactor
                )
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