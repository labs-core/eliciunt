/**
 * @file      constants.rs
 * @brief     Application-wide compile-time constants.
 * @details   Centralises magic numbers used across the analysis, export,
 *            and UI modules so they can be tuned from a single location.
 *
 * @copyright  (C) Core Labs
 *             All rights reserved.
 *
 * @author     Manoel Serafim
 * @email      manoel.serafim@proton.me
 * @github     https://github.com/manoel-serafim
 * SPDX-License-Identifier: GPL-3.0
 */

pub const BYTE_RANGE:          usize = 256;
pub const CHI2_DF:             f64   = 255.0;
pub const HEX_COLUMNS:         usize = 16;
pub const PLOT_HEIGHT_PX:      f32   = 155.0;
pub const PNG_CHART_WIDTH:     u32   = 1100;
pub const PNG_CHART_HEIGHT:    u32   = 450;
pub const UNIFORM_SPIKE_RATIO: f64   = 1.5;
pub const ANOMALY_K_DEFAULT:   f64   = 2.0;
pub const WINDOW_SIZE_DEFAULT: usize = 512;
