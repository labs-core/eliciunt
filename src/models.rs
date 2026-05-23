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
