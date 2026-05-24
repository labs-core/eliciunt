/**
 * @file      padding.rs
 * @brief     Automatic padding-region detection and bookmark generation.
 * @details   Scans a flat binary image for contiguous runs of a single fill
 *            byte value (0xFF – erased flash, 0x00 – zero-fill) whose length
 *            meets a configurable minimum threshold.
 *
 *            Each fill-byte class is collapsed into a single MultiRangeBookmark
 *            that carries ALL qualifying runs of that class as sub-regions.
 *            This lets the bookmark panel show one "0xFF padding" card (and one
 *            "0x00 padding" card) instead of an unbounded number of individual
 *            entries, while still marking every individual run in the plots and
 *            hex view via `flatten_to_hex_bookmarks()`.
 *
 *            `PaddingRegion` is also re-used by the user-bookmark path in
 *            `app.rs` to represent individual segments of a user-created
 *            multi-range bookmark, keeping the two sub-systems uniform.  In
 *            that context `fill_byte` is set to 0 and carries no meaning.
 *
 *            Algorithm – O(n) single forward pass:
 *              1. Walk the byte slice tracking the current run (fill byte +
 *                 run length).
 *              2. On any byte transition, check whether the finished run
 *                 qualifies (fill byte is 0xFF or 0x00, length ≥ min_run).
 *              3. Emit a PaddingRegion for each qualifying run.
 *              4. Group regions by fill byte → one MultiRangeBookmark per class.
 *
 * @copyright  (C) Core Labs
 *             All rights reserved.
 *
 * @author     Manoel Serafim
 * @email      manoel.serafim@proton.me
 * @github     https://github.com/manoel-serafim
 * SPDX-License-Identifier: GPL-3.0
 */

use egui::Color32;
use crate::models::HexBookmark;

// ── Visual identity for each fill-byte class ────────────────────────────────
//
// 0xFF  – matches the paper's description of erased flash (all bits set).
//         Grey family: neutral, suggests "emptiness" or blank silicon.
// 0x00  – zero-fill / linker-inserted padding.
//         Steel-blue family: cold, structured, not data.

const COLOR_FF: Color32 = Color32::from_rgb(180, 180, 185);   // ash grey
const COLOR_00: Color32 = Color32::from_rgb( 90, 140, 210);   // steel blue

// ── PaddingRegion ────────────────────────────────────────────────────────────

/// A single contiguous byte-range that belongs to a [`MultiRangeBookmark`].
///
/// For auto-detected padding, `fill_byte` is 0xFF or 0x00.
/// For user-created multi-range bookmarks (see `app::BookmarkEditState`),
/// `fill_byte` is 0x00 and carries no semantic meaning – only `start` / `len`
/// are used.
#[derive(Debug, Clone)]
pub struct PaddingRegion {
    /// Byte offset of the first fill byte.
    pub start:     usize,
    /// Number of consecutive fill bytes.
    pub len:       usize,
    /// The repeated byte value (0xFF, 0x00, or 0 for user ranges).
    pub fill_byte: u8,
}

impl PaddingRegion {
    /// Exclusive end offset (first byte after this region).
    #[inline]
    pub fn end(&self) -> usize {
        self.start + self.len
    }

    /// Convenience constructor for user-defined ranges (fill_byte = 0).
    #[inline]
    pub fn user(start: usize, len: usize) -> Self {
        Self { start, len, fill_byte: 0 }
    }
}

// ── MultiRangeBookmark ───────────────────────────────────────────────────────

/// A bookmark that groups multiple non-contiguous byte ranges of the same
/// logical class under a single label and colour.
///
/// Rendering is handled entirely by `plots::draw_multi_range_bookmark`, which
/// emits one translucent fill per sub-region and a single shared label,
/// keeping the plot visually unified regardless of region count.
///
/// Use [`flatten_to_hex_bookmarks`] when you need one `HexBookmark` per
/// sub-region for export, testing, or legacy rendering paths.
#[derive(Debug, Clone)]
pub struct MultiRangeBookmark {
    /// Human-readable name shown in the bookmark panel header and plot legend.
    pub label:   String,
    /// Shared colour for all sub-regions (matches the fill-byte class or user
    /// choice).
    pub color:   Color32,
    /// Every qualifying run that belongs to this bookmark, in offset order.
    pub regions: Vec<PaddingRegion>,
}

impl MultiRangeBookmark {
    /// Total number of bytes covered across all sub-regions.
    pub fn total_bytes(&self) -> usize {
        self.regions.iter().map(|r| r.len).sum()
    }

    /// Number of sub-regions in this group.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// `true` if this group contains exactly one sub-region (functionally
    /// equivalent to a plain `HexBookmark`).
    pub fn is_single_range(&self) -> bool {
        self.regions.len() == 1
    }
}

// ── Detection ────────────────────────────────────────────────────────────────

/// Scan `data` for contiguous runs of 0xFF or 0x00 that are at least
/// `min_run` bytes long.
///
/// Returns one [`PaddingRegion`] per qualifying run, in offset order.
pub fn detect_padding_regions(data: &[u8], min_run: usize) -> Vec<PaddingRegion> {
    let mut regions: Vec<PaddingRegion> = Vec::new();

    if data.is_empty() || min_run == 0 {
        return regions;
    }

    let mut run_start = 0usize;
    let mut run_byte  = data[0];
    let mut run_len   = 1usize;

    for i in 1..data.len() {
        if data[i] == run_byte {
            run_len += 1;
        } else {
            emit_if_qualifies(&mut regions, run_start, run_len, run_byte, min_run);
            run_start = i;
            run_byte  = data[i];
            run_len   = 1;
        }
    }
    // Flush the final run.
    emit_if_qualifies(&mut regions, run_start, run_len, run_byte, min_run);

    regions
}

#[inline]
fn emit_if_qualifies(
    out:       &mut Vec<PaddingRegion>,
    start:     usize,
    len:       usize,
    fill_byte: u8,
    min_run:   usize,
) {
    if len >= min_run && (fill_byte == 0xFF || fill_byte == 0x00) {
        out.push(PaddingRegion { start, len, fill_byte });
    }
}

// ── MultiRangeBookmark construction ─────────────────────────────────────────

/// Group a flat list of [`PaddingRegion`]s into at most two
/// [`MultiRangeBookmark`]s – one per fill-byte class (0xFF, 0x00).
///
/// Classes that have no qualifying regions are omitted from the output.
/// Regions within each group retain their original offset order.
pub fn build_multi_range_bookmarks(regions: &[PaddingRegion]) -> Vec<MultiRangeBookmark> {
    let mut ff_regions:   Vec<PaddingRegion> = Vec::new();
    let mut zero_regions: Vec<PaddingRegion> = Vec::new();

    for r in regions {
        if r.fill_byte == 0xFF {
            ff_regions.push(r.clone());
        } else {
            zero_regions.push(r.clone());
        }
    }

    let mut out = Vec::with_capacity(2);

    if !ff_regions.is_empty() {
        let total = ff_regions.iter().map(|r| r.len).sum::<usize>();
        out.push(MultiRangeBookmark {
            label:   format!(
                "0xFF padding  {}  ×{}",
                fmt_byte_count(total),
                ff_regions.len()
            ),
            color:   COLOR_FF,
            regions: ff_regions,
        });
    }

    if !zero_regions.is_empty() {
        let total = zero_regions.iter().map(|r| r.len).sum::<usize>();
        out.push(MultiRangeBookmark {
            label:   format!(
                "0x00 padding  {}  ×{}",
                fmt_byte_count(total),
                zero_regions.len()
            ),
            color:   COLOR_00,
            regions: zero_regions,
        });
    }

    out
}

/// Expand a slice of [`MultiRangeBookmark`]s into individual [`HexBookmark`]s,
/// one per sub-region, ready for the plot renderer and hex view.
///
/// Each `HexBookmark` inherits the parent's colour and gets a label of the
/// form `"0xFF pad  (4.0 KiB)"` so it is self-describing in plot tooltips.
///
/// Note: prefer `plots::draw_multi_range_bookmarks` for rendering; this
/// function is retained for export, tests, and legacy callers.
pub fn flatten_to_hex_bookmarks(multi: &[MultiRangeBookmark]) -> Vec<HexBookmark> {
    let mut out = Vec::new();
    for mbm in multi {
        // Determine the label prefix from the colour, falling back to a generic
        // marker for user-created groups whose fill_byte is 0.
        let kind = if mbm.color == COLOR_FF {
            "0xFF"
        } else if mbm.color == COLOR_00 {
            "0x00"
        } else {
            "user"
        };

        for r in &mbm.regions {
            out.push(HexBookmark {
                start: r.start,
                len:   r.len,
                label: format!("{kind} pad  {}", fmt_byte_count(r.len)),
                color: mbm.color,
            });
        }
    }
    out
}

// ── Convenience wrappers ─────────────────────────────────────────────────────

/// Detect padding regions and return grouped [`MultiRangeBookmark`]s.
///
/// This is the primary entry point for the auto-bookmark feature.
pub fn detect_and_build_multi(data: &[u8], min_run: usize) -> Vec<MultiRangeBookmark> {
    let regions = detect_padding_regions(data, min_run);
    build_multi_range_bookmarks(&regions)
}

/// Detect + group + flatten: returns one [`HexBookmark`] per qualifying run.
///
/// Kept for callers that only need the flat list (e.g. export, tests).
pub fn detect_and_build(data: &[u8], min_run: usize) -> Vec<HexBookmark> {
    flatten_to_hex_bookmarks(&detect_and_build_multi(data, min_run))
}

// ── Formatting helpers ───────────────────────────────────────────────────────

/// Format a byte count as a human-readable string with units.
///
/// ```
/// assert_eq!(fmt_byte_count(512),       "(512 B)");
/// assert_eq!(fmt_byte_count(2048),      "(2.0 KiB)");
/// assert_eq!(fmt_byte_count(1_572_864), "(1.5 MiB)");
/// ```
pub fn fmt_byte_count(n: usize) -> String {
    if n >= 1_048_576 {
        format!("({:.1} MiB)", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("({:.1} KiB)", n as f64 / 1024.0)
    } else {
        format!("({n} B)")
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_padding_regions ───────────────────────────────────────────────

    #[test]
    fn detects_ff_run() {
        let data = vec![0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0x02];
        let regions = detect_padding_regions(&data, 4);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start,     1);
        assert_eq!(regions[0].len,       4);
        assert_eq!(regions[0].fill_byte, 0xFF);
    }

    #[test]
    fn detects_zero_run() {
        let data = vec![0x00, 0x00, 0x00, 0x01];
        let regions = detect_padding_regions(&data, 3);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].fill_byte, 0x00);
    }

    #[test]
    fn below_min_run_is_excluded() {
        let data = vec![0xFF; 3];
        let regions = detect_padding_regions(&data, 4);
        assert!(regions.is_empty());
    }

    #[test]
    fn non_fill_bytes_are_excluded() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let regions = detect_padding_regions(&data, 1);
        assert!(regions.is_empty());
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(detect_padding_regions(&[], 4).is_empty());
    }

    #[test]
    fn zero_min_run_returns_empty() {
        let data = vec![0xFF; 100];
        assert!(detect_padding_regions(&data, 0).is_empty());
    }

    // ── build_multi_range_bookmarks ──────────────────────────────────────────

    #[test]
    fn groups_by_fill_byte() {
        let regions = vec![
            PaddingRegion { start: 0,  len: 10, fill_byte: 0xFF },
            PaddingRegion { start: 20, len: 5,  fill_byte: 0x00 },
            PaddingRegion { start: 30, len: 8,  fill_byte: 0xFF },
        ];
        let groups = build_multi_range_bookmarks(&regions);
        assert_eq!(groups.len(), 2);

        let ff = groups.iter().find(|g| g.label.starts_with("0xFF")).unwrap();
        assert_eq!(ff.region_count(), 2);
        assert_eq!(ff.total_bytes(), 18);

        let zz = groups.iter().find(|g| g.label.starts_with("0x00")).unwrap();
        assert_eq!(zz.region_count(), 1);
        assert_eq!(zz.total_bytes(), 5);
    }

    // ── fmt_byte_count ───────────────────────────────────────────────────────

    #[test]
    fn format_bytes() {
        assert_eq!(fmt_byte_count(512),       "(512 B)");
        assert_eq!(fmt_byte_count(1024),      "(1.0 KiB)");
        assert_eq!(fmt_byte_count(1_048_576), "(1.0 MiB)");
    }

    // ── PaddingRegion helpers ────────────────────────────────────────────────

    #[test]
    fn end_offset() {
        let r = PaddingRegion { start: 10, len: 5, fill_byte: 0xFF };
        assert_eq!(r.end(), 15);
    }

    #[test]
    fn is_single_range() {
        let mbm = MultiRangeBookmark {
            label:   "test".into(),
            color:   Color32::WHITE,
            regions: vec![PaddingRegion::user(0, 10)],
        };
        assert!(mbm.is_single_range());
    }
}
