use std::collections::HashSet;
use crate::constants::BYTE_RANGE;

pub fn compute_entropy(window: &[u8]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    let mut byte_histogram = [0usize; BYTE_RANGE];
    for &byte_val in window {
        byte_histogram[byte_val as usize] += 1;
    }
    let window_len = window.len() as f64;
    byte_histogram
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let probability = count as f64 / window_len;
            -probability * probability.log2()
        })
        .sum()
}

pub fn compute_chi2(window: &[u8]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    let mut byte_histogram = [0usize; BYTE_RANGE];
    for &byte_val in window {
        byte_histogram[byte_val as usize] += 1;
    }
    let expected_count = window.len() as f64 / BYTE_RANGE as f64;
    byte_histogram
        .iter()
        .map(|&observed| {
            let delta = observed as f64 - expected_count;
            delta * delta / expected_count
        })
        .sum()
}

pub fn serial_correlation(window: &[u8]) -> f64 {
    if window.len() < 2 {
        return 0.0;
    }
    let n = window.len() as f64;
    let (mut sum_x, mut sum_x_sq, mut sum_xy) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..window.len() {
        let x = window[i] as f64;
        let y = window[(i + 1) % window.len()] as f64;
        sum_x    += x;
        sum_x_sq += x * x;
        sum_xy   += x * y;
    }
    // Pearson lag-1 circular autocorrelation:
    //   r = (n·Σxy − Σx·Σy) / sqrt((n·Σx²−(Σx)²)·(n·Σy²−(Σy)²))
    // Because the series wraps around circularly, Σy == Σx and Σy² == Σx²,
    // so both variance legs are equal to `denominator` and the sqrt collapses:
    //   r = numerator / denominator
    // A clamp guards against tiny floating-point overshoots.
    let numerator   = n * sum_xy   - sum_x * sum_x;
    let denominator = n * sum_x_sq - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        0.0
    } else {
        (numerator / denominator).clamp(-1.0, 1.0)
    }
}

pub fn hamming_weight(window: &[u8]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    window.iter().map(|b| b.count_ones()).sum::<u32>() as f64 / window.len() as f64
}

pub fn ngram_uniqueness(window: &[u8], n: usize) -> f64 {
    if window.len() < n {
        return 0.0;
    }
    let total_ngrams  = window.len() - n + 1;
    let unique_ngrams: HashSet<&[u8]> = (0..total_ngrams).map(|i| &window[i..i + n]).collect();
    unique_ngrams.len() as f64 / total_ngrams as f64
}