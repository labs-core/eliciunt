use crate::constants::BYTE_RANGE;

#[derive(Clone, Default)]
pub struct MetricStats {
    pub mean: f64,
    pub sd:   f64,
    pub min:  f64,
    pub max:  f64,
}

impl MetricStats {
    pub fn from_series(series: &[[f64; 2]]) -> Self {
        if series.is_empty() {
            return Self::default();
        }
        let values: Vec<f64> = series.iter().map(|point| point[1]).collect();
        let count   = values.len() as f64;
        let mean    = values.iter().sum::<f64>() / count;
        let sd      = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count).sqrt();
        let minimum = values.iter().cloned().fold(f64::INFINITY,     f64::min);
        let maximum = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Self { mean, sd, min: minimum, max: maximum }
    }
}

#[derive(Clone, Default)]
pub struct FileStatistics {
    pub entropy_stats:    MetricStats,
    pub chi2_stats:       MetricStats,
    pub serial_stats:     MetricStats,
    pub hamming_stats:    MetricStats,
    pub ks_statistic:     f64,
    pub ks_pvalue:        f64,
    pub global_chi2:      f64,
    pub global_chi2_p:    f64,
    pub runs_z_score:     f64,
    pub runs_pvalue:      f64,
    pub mean_window_chi2p: f64,
}

#[derive(Clone, Default)]
pub struct AnomalyThresholds {
    pub entropy_mean: f64,
    pub entropy_sd:   f64,
    pub chi2_mean:    f64,
    pub chi2_sd:      f64,
    pub serial_mean:  f64,
    pub serial_sd:    f64,
}

impl AnomalyThresholds {
    pub fn from_metric_series(
        entropy_series: &[[f64; 2]],
        chi2_series:    &[[f64; 2]],
        serial_series:  &[[f64; 2]],
    ) -> Self {
        fn mean_and_sd(series: &[[f64; 2]]) -> (f64, f64) {
            if series.is_empty() {
                return (0.0, 1.0);
            }
            let count = series.len() as f64;
            let mean  = series.iter().map(|p| p[1]).sum::<f64>() / count;
            let sd    = (series.iter().map(|p| (p[1] - mean).powi(2)).sum::<f64>() / count)
                .sqrt()
                .max(f64::EPSILON);
            (mean, sd)
        }
        let (entropy_mean, entropy_sd) = mean_and_sd(entropy_series);
        let (chi2_mean,    chi2_sd)    = mean_and_sd(chi2_series);
        let (serial_mean,  serial_sd)  = mean_and_sd(serial_series);
        Self { entropy_mean, entropy_sd, chi2_mean, chi2_sd, serial_mean, serial_sd }
    }

    pub fn is_suspicious(&self, entropy: f64, chi2: f64, serial: f64, sigma_threshold: f64) -> bool {
        (entropy - self.entropy_mean).abs() / self.entropy_sd > sigma_threshold
            || (chi2   - self.chi2_mean).abs()   / self.chi2_sd   > sigma_threshold
            || (serial - self.serial_mean).abs() / self.serial_sd > sigma_threshold
    }
}

#[derive(Clone)]
pub struct RegionInsight {
    pub offset:      usize,
    pub entropy:     f64,
    pub chi2:        f64,
    pub chi2_pvalue: f64,
    pub serial_corr: f64,
    pub hamming:     f64,
    pub suspicious:  bool,
}

#[derive(Clone)]
pub struct AnalysisResult {
    pub entropy:          Vec<[f64; 2]>,
    pub chi2:             Vec<[f64; 2]>,
    pub serial_corr:      Vec<[f64; 2]>,
    pub hamming:          Vec<[f64; 2]>,
    pub byte_distribution: [f64; BYTE_RANGE],
    pub byte_counts:      [usize; BYTE_RANGE],
    pub bigram_scores:    Vec<[f64; 2]>,
    pub trigram_scores:   Vec<[f64; 2]>,
    pub regions:          Vec<RegionInsight>,
    pub thresholds:       AnomalyThresholds,
    pub window_size:      usize,
    pub stats:            FileStatistics,
}

impl Default for AnalysisResult {
    fn default() -> Self {
        Self {
            entropy:           Vec::new(),
            chi2:              Vec::new(),
            serial_corr:       Vec::new(),
            hamming:           Vec::new(),
            byte_distribution: [0.0; BYTE_RANGE],
            byte_counts:       [0usize; BYTE_RANGE],
            bigram_scores:     Vec::new(),
            trigram_scores:    Vec::new(),
            regions:           Vec::new(),
            thresholds:        AnomalyThresholds::default(),
            window_size:       0,
            stats:             FileStatistics::default(),
        }
    }
}

pub struct BinaryFile {
    pub name:   String,
    pub data:   Vec<u8>,
    pub result: Option<AnalysisResult>,
}


// ─────────────────────────────────────────────────────────────────────────────
// Paste these two structs anywhere inside models.rs (e.g. after BinaryFile).
// ─────────────────────────────────────────────────────────────────────────────

use eframe::egui::Color32;

/// A named, coloured byte-range bookmark created by the user in the hex view.
/// It is stored globally on `App` and fed into every metric plot as a shaded
/// background band so the region is always visible across all graphs.
#[derive(Clone)]
pub struct HexBookmark {
    /// Byte offset where the selection starts.
    pub start:  usize,
    /// Length of the selection in bytes.
    pub len:    usize,
    /// Display label chosen by the user.
    pub label:  String,
    /// Colour chosen by the user (same colour used in hex rows AND plot bands).
    pub color:  Color32,
}

impl HexBookmark {
    /// Inclusive end offset (last byte of the bookmarked region).
    pub fn end(&self) -> usize {
        self.start.saturating_add(self.len).saturating_sub(1)
    }

    /// The x-axis extent as plot coordinates (byte offsets).
    pub fn plot_x_range(&self) -> (f64, f64) {
        (self.start as f64, (self.start + self.len) as f64)
    }
}

/// Tracks the in-progress drag selection in the hex panel.
/// Cleared as soon as the user confirms the bookmark via the creation dialog.
#[derive(Clone, Default)]
pub struct HexSelectionState {
    /// Byte index where the mouse button was pressed.
    pub drag_start: Option<usize>,
    /// Byte index currently under the cursor (updated every frame while dragging).
    pub drag_end:   Option<usize>,
}

impl HexSelectionState {
    /// Returns `(start, len)` normalised so start ≤ end, or `None` while no
    /// drag is in progress.
    pub fn normalised(&self) -> Option<(usize, usize)> {
        match (self.drag_start, self.drag_end) {
            (Some(a), Some(b)) => {
                let lo = a.min(b);
                let hi = a.max(b);
                Some((lo, hi - lo + 1))
            }
            _ => None,
        }
    }

    pub fn clear(&mut self) {
        self.drag_start = None;
        self.drag_end   = None;
    }
}