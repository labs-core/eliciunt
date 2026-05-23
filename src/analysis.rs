use std::fs;
use rfd::FileDialog;

use crate::constants::BYTE_RANGE;
use crate::math::{chi2_pvalue, ks_uniform_test, runs_test};
use crate::metrics::{
    compute_chi2, compute_entropy, hamming_weight, ngram_uniqueness, serial_correlation,
};
use crate::models::{
    AnalysisResult, AnomalyThresholds, BinaryFile, FileStatistics, MetricStats, RegionInsight,
};

pub fn load_binary_file() -> Option<BinaryFile> {
    let path = FileDialog::new().pick_file()?;
    let data = fs::read(&path).ok()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(BinaryFile { name, data, result: None })
}

pub fn analyze_binary(data: &[u8], window_size: usize, sigma_threshold: f64) -> AnalysisResult {
    let mut result     = AnalysisResult::default();
    result.window_size = window_size;
    if data.is_empty() {
        return result;
    }

    let file_len = data.len() as f64;
    let mut global_histogram = [0usize; BYTE_RANGE];
    for &byte_val in data {
        global_histogram[byte_val as usize] += 1;
    }
    for i in 0..BYTE_RANGE {
        result.byte_distribution[i] = global_histogram[i] as f64 / file_len;
        result.byte_counts[i]       = global_histogram[i];
    }

    let step = window_size.max(1);
    for window_offset in (0..data.len().saturating_sub(window_size) + 1).step_by(step) {
        let window_end = (window_offset + window_size).min(data.len());
        if window_end - window_offset < window_size {
            break;
        }
        let window_slice = &data[window_offset..window_end];
        let window_chi2  = compute_chi2(window_slice);
        let offset_f64   = window_offset as f64;

        result.entropy.push([offset_f64,       compute_entropy(window_slice)]);
        result.chi2.push([offset_f64,           window_chi2]);
        result.serial_corr.push([offset_f64,    serial_correlation(window_slice)]);
        result.hamming.push([offset_f64,         hamming_weight(window_slice)]);
        result.bigram_scores.push([offset_f64,  ngram_uniqueness(window_slice, 2)]);
        result.trigram_scores.push([offset_f64, ngram_uniqueness(window_slice, 3)]);
    }

    let thresholds = AnomalyThresholds::from_metric_series(
        &result.entropy,
        &result.chi2,
        &result.serial_corr,
    );
    for i in 0..result.entropy.len() {
        let window_chi2    = result.chi2[i][1];
        let window_entropy = result.entropy[i][1];
        let window_serial  = result.serial_corr[i][1];
        result.regions.push(RegionInsight {
            offset:      result.entropy[i][0] as usize,
            entropy:     window_entropy,
            chi2:        window_chi2,
            chi2_pvalue: chi2_pvalue(window_chi2, 255),
            serial_corr: window_serial,
            hamming:     result.hamming[i][1],
            suspicious:  thresholds.is_suspicious(window_entropy, window_chi2, window_serial, sigma_threshold),
        });
    }
    result.thresholds = thresholds;

    let global_chi2_stat     = compute_chi2(data);
    let (ks_d, ks_p)         = ks_uniform_test(data);
    let (runs_z, runs_p)     = runs_test(data);
    let mean_window_chi2p    = if result.regions.is_empty() {
        1.0
    } else {
        result.regions.iter().map(|r| r.chi2_pvalue).sum::<f64>() / result.regions.len() as f64
    };
    result.stats = FileStatistics {
        entropy_stats:     MetricStats::from_series(&result.entropy),
        chi2_stats:        MetricStats::from_series(&result.chi2),
        serial_stats:      MetricStats::from_series(&result.serial_corr),
        hamming_stats:     MetricStats::from_series(&result.hamming),
        ks_statistic:      ks_d,
        ks_pvalue:         ks_p,
        global_chi2:       global_chi2_stat,
        global_chi2_p:     chi2_pvalue(global_chi2_stat, 255),
        runs_z_score:      runs_z,
        runs_pvalue:       runs_p,
        mean_window_chi2p,
    };
    result
}

pub fn reapply_sigma_threshold(result: &mut AnalysisResult, sigma_threshold: f64) {
    let thresholds = AnomalyThresholds::from_metric_series(
        &result.entropy,
        &result.chi2,
        &result.serial_corr,
    );
    for region in &mut result.regions {
        region.suspicious = thresholds.is_suspicious(
            region.entropy,
            region.chi2,
            region.serial_corr,
            sigma_threshold,
        );
    }
    result.thresholds = thresholds;
}
