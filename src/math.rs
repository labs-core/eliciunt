/**
 * @file      math.rs
 * @brief     Statistical distribution approximations and hypothesis tests.
 * @details   Implements a polynomial erfc approximation, chi-squared p-value
 *            via the Wilson-Hilferty cube-root transform, the Kolmogorov-Smirnov
 *            uniform-distribution test, and the Wald-Wolfowitz runs test.
 *
 * @copyright  (C) Core Labs
 *             All rights reserved.
 *
 * @author     Manoel Serafim
 * @email      manoel.serafim@proton.me
 * @github     https://github.com/manoel-serafim
 * SPDX-License-Identifier: GPL-3.0
 */

use crate::constants::BYTE_RANGE;

pub fn erfc_approx(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let horner = t * (0.254829592
        + t * (-0.284496736
        + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    let tail = horner * (-x * x).exp();
    if x >= 0.0 { tail } else { 2.0 - tail }
}

pub fn normal_upper_tail(z: f64) -> f64 {
    0.5 * erfc_approx(z / std::f64::consts::SQRT_2)
}

pub fn chi2_pvalue(chi2_stat: f64, degrees_of_freedom: usize) -> f64 {
    if chi2_stat <= 0.0 || degrees_of_freedom == 0 {
        return 1.0;
    }
    let df_f64       = degrees_of_freedom as f64;
    let cbrt_arg     = (chi2_stat / df_f64).powf(1.0 / 3.0);
    let normal_mu    = 1.0 - 2.0 / (9.0 * df_f64);
    let normal_sigma = (2.0 / (9.0 * df_f64)).sqrt();
    normal_upper_tail((cbrt_arg - normal_mu) / normal_sigma)
}

pub fn ks_uniform_test(data: &[u8]) -> (f64, f64) {
    if data.is_empty() {
        return (0.0, 1.0);
    }
    let n = data.len();
    let mut byte_counts = [0usize; BYTE_RANGE];
    for &byte_val in data {
        byte_counts[byte_val as usize] += 1;
    }

    let mut empirical_cdf = 0.0f64;
    let mut max_deviation = 0.0f64;
    for i in 0..BYTE_RANGE {
        empirical_cdf += byte_counts[i] as f64 / n as f64;
        let theoretical_cdf = (i + 1) as f64 / BYTE_RANGE as f64;
        max_deviation = max_deviation.max((empirical_cdf - theoretical_cdf).abs());
    }

    let scaled_deviation = max_deviation * (n as f64).sqrt();
    let mut kolmogorov_sum = 0.0f64;
    for k in 1i64..=60 {
        let term = (-2.0 * (k * k) as f64 * scaled_deviation * scaled_deviation).exp();
        if k % 2 == 1 { kolmogorov_sum += term; } else { kolmogorov_sum -= term; }
        if term < 1e-14 { break; }
    }
    (max_deviation, (2.0 * kolmogorov_sum).clamp(0.0, 1.0))
}

pub fn runs_test(data: &[u8]) -> (f64, f64) {
    if data.len() < 2 {
        return (0.0, 1.0);
    }
    let median = {
        let mut sorted = data.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2] as f64
    };
    let above_median: Vec<bool> = data.iter().map(|&b| (b as f64) >= median).collect();
    let count_above = above_median.iter().filter(|&&a|  a).count() as f64;
    let count_below = above_median.iter().filter(|&&a| !a).count() as f64;
    let total_n = count_above + count_below;

    if count_above == 0.0 || count_below == 0.0 {
        return (0.0, 1.0);
    }

    let mut run_count = 1u64;
    for i in 1..above_median.len() {
        if above_median[i] != above_median[i - 1] {
            run_count += 1;
        }
    }

    let runs_f64      = run_count as f64;
    let expected_runs = 2.0 * count_above * count_below / total_n + 1.0;
    let runs_variance = 2.0 * count_above * count_below
        * (2.0 * count_above * count_below - total_n)
        / (total_n * total_n * (total_n - 1.0));

    if runs_variance <= 0.0 {
        return (0.0, 1.0);
    }
    let z_score = (runs_f64 - expected_runs) / runs_variance.sqrt();
    (z_score, (2.0 * normal_upper_tail(z_score.abs())).min(1.0))
}
