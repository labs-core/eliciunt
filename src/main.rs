use eframe::egui;
use egui::{Color32, FontId, Frame, Margin, RichText, Rounding, Stroke, Vec2};
use egui_plot::{Bar, BarChart, HLine, Legend, Line, Plot, PlotPoints};
use rfd::FileDialog;
use std::{collections::HashSet, fs};

const MAX_BYTE: usize = 256;
const HEX_WIDTH: usize = 16;

mod pal {
    use egui::Color32;
    pub const BG:        Color32 = Color32::from_rgb(255, 255, 255);
    pub const PANEL:     Color32 = Color32::from_rgb(255, 255, 255);
    pub const BORDER:    Color32 = Color32::from_rgb(230, 210, 210);
    pub const RED:       Color32 = Color32::from_rgb(196,  28,  28);
    pub const RED_MID:   Color32 = Color32::from_rgb(220,  80,  80);
    pub const RED_LIGHT: Color32 = Color32::from_rgb(254, 226, 226);
    pub const RED_FAINT: Color32 = Color32::from_rgb(255, 245, 245);
    pub const TEXT:      Color32 = Color32::from_rgb( 20,  10,  10);
    pub const MUTED:     Color32 = Color32::from_rgb(150, 100, 100);
    pub const HL_BG:     Color32 = Color32::from_rgb(255, 210, 210);
    pub const HL_BORDER: Color32 = Color32::from_rgb(196,  28,  28);
    pub const GREEN:     Color32 = Color32::from_rgb( 22, 120,  60);
}

// ── Statistical helpers ───────────────────────────────────────────────────────

/// Abramowitz & Stegun erfc approximation, max error ≈ 1.5×10⁻⁷.
fn erfc_approx(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let p = t * (0.254829592
        + t * (-0.284496736
        + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    let r = p * (-x * x).exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

/// P(Z > z) for the standard normal distribution.
fn normal_upper(z: f64) -> f64 {
    0.5 * erfc_approx(z / std::f64::consts::SQRT_2)
}

/// Chi² p-value (upper tail, `df` degrees of freedom) via Wilson–Hilferty
/// normal approximation.  Returns P(χ²_{df} > x).
fn chi2_pvalue(x: f64, df: usize) -> f64 {
    if x <= 0.0 || df == 0 { return 1.0; }
    let d    = df as f64;
    let cbrt = (x / d).powf(1.0 / 3.0);
    let mu   = 1.0 - 2.0 / (9.0 * d);
    let sig  = (2.0 / (9.0 * d)).sqrt();
    normal_upper((cbrt - mu) / sig)
}

/// Kolmogorov–Smirnov test of the byte-value empirical CDF against
/// Uniform{0, …, 255}.  Returns (D statistic, asymptotic p-value).
fn ks_uniform_test(data: &[u8]) -> (f64, f64) {
    if data.is_empty() { return (0.0, 1.0); }
    let n = data.len();
    let mut counts = [0usize; MAX_BYTE];
    for &b in data { counts[b as usize] += 1; }
    let mut cdf_emp = 0.0f64;
    let mut d = 0.0f64;
    for i in 0..MAX_BYTE {
        cdf_emp += counts[i] as f64 / n as f64;
        let cdf_th = (i + 1) as f64 / MAX_BYTE as f64;
        d = d.max((cdf_emp - cdf_th).abs());
    }
    let dn = d * (n as f64).sqrt();
    let mut p_sum = 0.0f64;
    for k in 1i64..=60 {
        let term = (-2.0 * (k * k) as f64 * dn * dn).exp();
        if k % 2 == 1 { p_sum += term; } else { p_sum -= term; }
        if term < 1e-14 { break; }
    }
    (d, (2.0 * p_sum).clamp(0.0, 1.0))
}

/// Wald–Wolfowitz runs test (above / below median).
/// Returns (Z statistic, two-tailed p-value).
fn runs_test(data: &[u8]) -> (f64, f64) {
    if data.len() < 2 { return (0.0, 1.0); }
    let median = {
        let mut s = data.to_vec();
        s.sort_unstable();
        s[s.len() / 2] as f64
    };
    let above: Vec<bool> = data.iter().map(|&b| (b as f64) >= median).collect();
    let n1 = above.iter().filter(|&&a|  a).count() as f64;
    let n2 = above.iter().filter(|&&a| !a).count() as f64;
    let n  = n1 + n2;
    if n1 == 0.0 || n2 == 0.0 { return (0.0, 1.0); }
    let mut runs = 1u64;
    for i in 1..above.len() {
        if above[i] != above[i - 1] { runs += 1; }
    }
    let r   = runs as f64;
    let mu  = 2.0 * n1 * n2 / n + 1.0;
    let var = 2.0 * n1 * n2 * (2.0 * n1 * n2 - n) / (n * n * (n - 1.0));
    if var <= 0.0 { return (0.0, 1.0); }
    let z = (r - mu) / var.sqrt();
    (z, (2.0 * normal_upper(z.abs())).min(1.0))
}

// ── Aggregate metric statistics ───────────────────────────────────────────────

#[derive(Clone, Default)]
struct MetricStats {
    mean: f64,
    sd:   f64,
    min:  f64,
    max:  f64,
}

impl MetricStats {
    fn from_series(data: &[[f64; 2]]) -> Self {
        if data.is_empty() { return Self::default(); }
        let vals: Vec<f64> = data.iter().map(|p| p[1]).collect();
        let n    = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        let sd   = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
        let min  = vals.iter().cloned().fold(f64::INFINITY,     f64::min);
        let max  = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Self { mean, sd, min, max }
    }
}

#[derive(Clone, Default)]
struct FileStatistics {
    entropy_stats: MetricStats,
    chi2_stats:    MetricStats,
    serial_stats:  MetricStats,
    hamming_stats: MetricStats,
    /// Kolmogorov–Smirnov statistic vs Uniform[0,255].
    ks_d:          f64,
    ks_p:          f64,
    /// Global chi-square over the whole file (df = 255).
    global_chi2:   f64,
    global_chi2_p: f64,
    /// Wald–Wolfowitz runs test.
    runs_z:        f64,
    runs_p:        f64,
    /// Mean chi-square p-value across windows (higher → more uniform).
    mean_window_p: f64,
}

// ── SVG export ────────────────────────────────────────────────────────────────

/// Generate a black-and-white scientific line chart as an SVG string.
/// `refs`: optional horizontal reference lines `(y_value, label)`.
fn svg_line_chart(
    data:    &[[f64; 2]],
    title:   &str,
    x_label: &str,
    y_label: &str,
    refs:    &[(f64, &str)],
) -> String {
    const VW: f64 = 900.0;
    const VH: f64 = 440.0;
    const ML: f64 = 82.0;
    const MR: f64 = 36.0;
    const MT: f64 = 54.0;
    const MB: f64 = 60.0;
    let pw = VW - ML - MR;
    let ph = VH - MT - MB;

    if data.is_empty() {
        return format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {VW} {VH}">
                <rect width="{VW}" height="{VH}" fill="white"/>
                <text x="{cx}" y="{cy}" text-anchor="middle"
                    font-family='Georgia, serif'
                    font-size="14" fill='#555'>
                    No data
                </text>
            </svg>"#,
            cx = VW / 2.0, cy = VH / 2.0
        );

    }

    let x_min = data.iter().map(|p| p[0]).fold(f64::INFINITY,     f64::min);
    let x_max = data.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let y_min_d = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
    let y_max_d = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);

    // Extend y-range to include reference lines.
    let ref_min = refs.iter().map(|&(v, _)| v).fold(f64::INFINITY,     f64::min);
    let ref_max = refs.iter().map(|&(v, _)| v).fold(f64::NEG_INFINITY, f64::max);
    let y_min = y_min_d.min(ref_min);
    let y_max = y_max_d.max(ref_max);

    let x_rng = (x_max - x_min).max(f64::EPSILON);
    let y_pad = (y_max - y_min).max(f64::EPSILON) * 0.08;
    let y_lo  = y_min - y_pad;
    let y_hi  = y_max + y_pad;
    let y_rng = y_hi - y_lo;

    let to_sx = |v: f64| ML + (v - x_min) / x_rng * pw;
    let to_sy = |v: f64| MT + ph - (v - y_lo)  / y_rng * ph;

    // Polyline.
    let pts: String = data.iter()
        .map(|p| format!("{:.2},{:.2}", to_sx(p[0]), to_sy(p[1])))
        .collect::<Vec<_>>()
        .join(" ");

    // Grid + tick labels.
    let mut grid = String::new();
    let ny = 6usize;
    for i in 0..=ny {
        let frac = i as f64 / ny as f64;
        let val  = y_lo + frac * y_rng;
        let sy   = MT + ph * (1.0 - frac);
        grid.push_str(&format!(
            "  <line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#e4e4e4' stroke-width='0.8'/>\n",
            ML, sy, ML + pw, sy
        ));
        grid.push_str(&format!(
            "  <text x='{:.1}' y='{:.1}' text-anchor='end' dominant-baseline='middle' font-family='Georgia,serif' font-size='11' fill='#333'>{:.4}</text>\n",
            ML - 5.0, sy, val
        ));
    }
    let nx = 6usize;
    for i in 0..=nx {
        let frac = i as f64 / nx as f64;
        let val  = x_min + frac * x_rng;
        let sx   = ML + frac * pw;
        grid.push_str(&format!(
            "  <line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#e4e4e4' stroke-width='0.8'/>\n",
            sx, MT, sx, MT + ph
        ));
        grid.push_str(&format!(
            "  <text x='{:.1}' y='{:.1}' text-anchor='middle' dominant-baseline='hanging' font-family='Georgia,serif' font-size='11' fill='#333'>0x{:X}</text>\n",
            sx, MT + ph + 6.0, val as usize
        ));
    }

    // Horizontal reference lines (dashed).
    let dash_styles = ["6,3", "3,3", "8,4"];
    let mut ref_svg = String::new();
    for (ri, &(val, lbl)) in refs.iter().enumerate() {
        let sy  = to_sy(val);
        let ds  = dash_styles.get(ri).unwrap_or(&"4,3");
        ref_svg.push_str(&format!(
            "  <line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#666' stroke-width='1.0' stroke-dasharray='{}'/>\n",
            ML, sy, ML + pw, sy, ds
        ));
        ref_svg.push_str(&format!(
            "  <text x='{:.1}' y='{:.1}' font-family='Georgia,serif' font-size='10' fill='#555' dominant-baseline='middle'>{}</text>\n",
            ML + pw + 3.0, sy, lbl
        ));
    }

    // Rotated y-axis label.
    let ylx = -(MT + ph / 2.0);
    let y_label_svg = format!(
        r#"  <text transform="rotate(-90)" x="{:.1}" y="18" text-anchor="middle" font-family='Georgia, serif' font-size="12" fill='#222">{}</text>"#,
        ylx, y_label
    );

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {VW} {VH}" width="{VW}" height="{VH}">
  <rect width="{VW}" height="{VH}" fill="white"/>
  <text x="{tx:.1}" y="32" text-anchor="middle" font-family='Georgia, serif' font-size="15" font-weight="bold" fill="black">{title}</text>
{grid}
{ref_svg}
  <rect x="{ML:.1}" y="{MT:.1}" width="{pw:.1}" height="{ph:.1}" fill="none" stroke="black" stroke-width="1.2"/>
  <polyline points="{pts}" fill="none" stroke="black" stroke-width="1.8" stroke-linejoin="round" stroke-linecap="round"/>
  <text x="{ax:.1}" y="{ay:.1}" text-anchor="middle" font-family='Georgia, serif' font-size="12" fill='#222">{x_label}</text>
{y_label_svg}
</svg>"#,
        tx = ML + pw / 2.0,
        title = title,
        ax = ML + pw / 2.0,
        ay = VH - 8.0,
        x_label = x_label,
    )
}

/// Generate a black-and-white scientific bar chart for the byte distribution.
/// Draws a dashed horizontal line at the expected uniform frequency.
fn svg_bar_chart(dist: &[f64; MAX_BYTE], title: &str) -> String {
    const VW: f64 = 900.0;
    const VH: f64 = 440.0;
    const ML: f64 = 78.0;
    const MR: f64 = 36.0;
    const MT: f64 = 54.0;
    const MB: f64 = 60.0;
    let pw = VW - ML - MR;
    let ph = VH - MT - MB;
    let bw = pw / MAX_BYTE as f64;

    let max_f = dist.iter().cloned().fold(0.0f64, f64::max).max(f64::EPSILON);
    let uniform_rel = (1.0 / MAX_BYTE as f64) / max_f;
    let ref_y = MT + ph * (1.0 - uniform_rel);

    // Bars.
    let mut bars_svg = String::new();
    for (i, &f) in dist.iter().enumerate() {
        let h = (f / max_f * ph).max(0.0);
        let x = ML + i as f64 * bw;
        let y = MT + ph - h;
        bars_svg.push_str(&format!(
            "  <rect x='{:.2}' y='{:.2}' width='{:.2}' height='{:.2}' fill='#222' stroke='none'/>\n",
            x + 0.5, y, (bw - 1.0).max(0.5), h
        ));
    }

    // Y ticks.
    let mut ticks = String::new();
    for i in 0..=5 {
        let frac = i as f64 / 5.0;
        let val  = frac * max_f;
        let sy   = MT + ph * (1.0 - frac);
        ticks.push_str(&format!(
            "  <line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='#e4e4e4' stroke-width='0.8'/>\n",
            ML, sy, ML + pw, sy
        ));
        ticks.push_str(&format!(
            "  <text x='{:.1}' y='{:.1}' text-anchor='end' dominant-baseline='middle' font-family='Georgia,serif' font-size='10' fill='#333'>{:.5}</text>\n",
            ML - 4.0, sy, val
        ));
    }
    // X tick labels every 32 bytes.
    for i in 0..=8 {
        let bv = i * 32usize;
        let sx = ML + bv as f64 * bw;
        ticks.push_str(&format!(
            "  <text x='{:.1}' y='{:.1}' text-anchor='middle' dominant-baseline='hanging' font-family='Georgia,serif' font-size='10' fill='#333'>0x{:02X}</text>\n",
            sx, MT + ph + 5.0, bv
        ));
    }

    let ylx = -(MT + ph / 2.0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {VW} {VH}" width="{VW}" height="{VH}">
  <rect width="{VW}" height="{VH}" fill="white"/>
  <text x="{tx:.1}" y="32" text-anchor="middle" font-family='Georgia, serif' font-size="15" font-weight="bold" fill="black">{title}</text>
{ticks}
{bars_svg}
  <line x1='{ML:.1}' y1='{ref_y:.1}' x2='{x2:.1}' y2='{ref_y:.1}' stroke='black' stroke-width='1.2' stroke-dasharray='7,4'/>
  <text x='{lx:.1}' y='{ly:.1}' font-family='Georgia,serif' font-size='10' fill='#222' dominant-baseline='middle'>uniform</text>
  <rect x="{ML:.1}" y="{MT:.1}" width="{pw:.1}" height="{ph:.1}" fill="none" stroke="black" stroke-width="1.2"/>
  <text x="{ax:.1}" y="{ay:.1}" text-anchor="middle" font-family='Georgia, serif' font-size="12" fill='#222">Byte value</text>
  <text transform="rotate(-90)" x="{ylx:.1}" y="18" text-anchor="middle" font-family='Georgia, serif' font-size="12" fill='#222">Relative frequency</text>
</svg>"#,
        tx = ML + pw / 2.0,
        title = title,
        x2 = ML + pw,
        lx = ML + pw + 3.0,
        ly = ref_y,
        ax = ML + pw / 2.0,
        ay = VH - 8.0,
        ylx = ylx,
    )
}

fn save_svg(svg: String, stem: &str) {
    if let Some(path) = FileDialog::new()
        .set_file_name(&format!("{stem}.svg"))
        .add_filter("SVG image", &["svg"])
        .save_file()
    {
        let _ = fs::write(path, svg.as_bytes());
    }
}

fn export_csv(result: &AnalysisResult, file_name: &str) {
    let Some(path) = FileDialog::new()
        .set_file_name(&format!("{file_name}_metrics.csv"))
        .add_filter("CSV", &["csv"])
        .save_file()
    else { return; };

    let mut out = String::from(
        "offset_hex,entropy_bits,chi2_stat,chi2_pvalue,serial_corr,hamming_bits_per_byte,bigram_uniq,trigram_uniq,suspicious\n"
    );
    let get = |series: &[[f64; 2]], i: usize| {
        series.get(i).map(|p| p[1]).unwrap_or(f64::NAN)
    };
    for (i, reg) in result.regions.iter().enumerate() {
        out.push_str(&format!(
            "0x{:08X},{:.6},{:.4},{:.6},{:.6},{:.6},{:.6},{:.6},{}\n",
            reg.offset,
            reg.entropy,
            reg.chi2,
            reg.chi2_p,
            reg.serial_corr,
            reg.hamming,
            get(&result.bigram_scores,  i),
            get(&result.trigram_scores, i),
            if reg.suspicious { 1 } else { 0 },
        ));
    }
    let _ = fs::write(path, out.as_bytes());
}

// ── Dynamic anomaly thresholds ────────────────────────────────────────────────

#[derive(Clone, Default)]
struct AnomalyThresholds {
    entropy_mean: f64, entropy_sd: f64,
    chi2_mean:    f64, chi2_sd:    f64,
    serial_mean:  f64, serial_sd:  f64,
}

impl AnomalyThresholds {
    fn from_metrics(e: &[[f64; 2]], c: &[[f64; 2]], s: &[[f64; 2]]) -> Self {
        fn mean_sd(v: &[[f64; 2]]) -> (f64, f64) {
            if v.is_empty() { return (0.0, 1.0); }
            let n  = v.len() as f64;
            let mu = v.iter().map(|p| p[1]).sum::<f64>() / n;
            let sd = (v.iter().map(|p| (p[1] - mu).powi(2)).sum::<f64>() / n).sqrt();
            (mu, sd.max(f64::EPSILON))
        }
        let (em, es) = mean_sd(e);
        let (cm, cs) = mean_sd(c);
        let (sm, ss) = mean_sd(s);
        Self {
            entropy_mean: em, entropy_sd: es,
            chi2_mean:    cm, chi2_sd:    cs,
            serial_mean:  sm, serial_sd:  ss,
        }
    }

    fn is_suspicious(&self, entropy: f64, chi2: f64, serial: f64, k: f64) -> bool {
        (entropy - self.entropy_mean).abs() / self.entropy_sd > k
            || (chi2   - self.chi2_mean).abs()   / self.chi2_sd   > k
            || (serial - self.serial_mean).abs() / self.serial_sd > k
    }
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct RegionInsight {
    offset:      usize,
    entropy:     f64,
    chi2:        f64,
    /// Chi² p-value for this window (df = 255).
    chi2_p:      f64,
    serial_corr: f64,
    hamming:     f64,
    suspicious:  bool,
}

#[derive(Clone)]
struct AnalysisResult {
    entropy:        Vec<[f64; 2]>,
    chi2:           Vec<[f64; 2]>,
    serial_corr:    Vec<[f64; 2]>,
    hamming:        Vec<[f64; 2]>,
    byte_dist:      [f64; MAX_BYTE],
    bigram_scores:  Vec<[f64; 2]>,
    trigram_scores: Vec<[f64; 2]>,
    regions:        Vec<RegionInsight>,
    thresholds:     AnomalyThresholds,
    window_size:    usize,
    stats:          FileStatistics,
}

impl Default for AnalysisResult {
    fn default() -> Self {
        Self {
            entropy:        Vec::new(),
            chi2:           Vec::new(),
            serial_corr:    Vec::new(),
            hamming:        Vec::new(),
            byte_dist:      [0.0; MAX_BYTE],
            bigram_scores:  Vec::new(),
            trigram_scores: Vec::new(),
            regions:        Vec::new(),
            thresholds:     AnomalyThresholds::default(),
            window_size:    0,
            stats:          FileStatistics::default(),
        }
    }
}

// ── Reorderable metric slots ──────────────────────────────────────────────────

#[derive(Clone)]
struct MetricSlot {
    key:   &'static str,
    label: &'static str,
    unit:  &'static str,
    norm:  bool,
}

fn default_metric_order() -> Vec<MetricSlot> {
    vec![
        MetricSlot { key: "entropy",  label: "Entropy",            unit: "bits / symbol", norm: true  },
        MetricSlot { key: "chi2",     label: "Chi²",               unit: "statistic",     norm: true  },
        MetricSlot { key: "serial",   label: "Serial Correlation", unit: "ρ",             norm: false },
        MetricSlot { key: "hamming",  label: "Hamming Weight",     unit: "bits / byte",   norm: true  },
        MetricSlot { key: "bigram",   label: "Bigram Uniqueness",  unit: "ratio",         norm: true  },
        MetricSlot { key: "trigram",  label: "Trigram Uniqueness", unit: "ratio",         norm: true  },
    ]
}

// ── File ──────────────────────────────────────────────────────────────────────

struct BinaryFile {
    name:   String,
    data:   Vec<u8>,
    result: Option<AnalysisResult>,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    files:         Vec<BinaryFile>,
    window_size:   usize,
    sel_file:      usize,
    active_tab:    usize,
    metric_order:  Vec<MetricSlot>,
    show_hex:      bool,
    anomaly_k:     f64,
    hex_highlight: Option<(usize, usize)>,
    hex_do_scroll: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            files:         Vec::new(),
            window_size:   512,
            sel_file:      0,
            active_tab:    0,
            metric_order:  default_metric_order(),
            show_hex:      false,
            anomaly_k:     2.0,
            hex_highlight: None,
            hex_do_scroll: false,
        }
    }
}

impl App {
    fn load_file() -> Option<BinaryFile> {
        let path = FileDialog::new().pick_file()?;
        let data = fs::read(&path).ok()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Some(BinaryFile { name, data, result: None })
    }

    fn analyze(data: &[u8], w: usize, k: f64) -> AnalysisResult {
        let mut r = AnalysisResult::default();
        r.window_size = w;
        if data.is_empty() { return r; }

        // Global byte frequency distribution.
        let mut hist = [0usize; MAX_BYTE];
        for &b in data { hist[b as usize] += 1; }
        let len_f = data.len() as f64;
        for i in 0..MAX_BYTE { r.byte_dist[i] = hist[i] as f64 / len_f; }

        let step = w.max(1);
        for offset in (0..=data.len().saturating_sub(w)).step_by(step) {
            let s = &data[offset..offset + w];
            let c2 = compute_chi2(s);
            r.entropy.push([offset as f64, compute_entropy(s)]);
            r.chi2.push([offset as f64, c2]);
            r.serial_corr.push([offset as f64, serial_correlation(s)]);
            r.hamming.push([offset as f64, hamming_weight(s)]);
            r.bigram_scores.push([offset as f64, ngram_uniqueness(s, 2)]);
            r.trigram_scores.push([offset as f64, ngram_uniqueness(s, 3)]);
        }

        let thr = AnomalyThresholds::from_metrics(&r.entropy, &r.chi2, &r.serial_corr);
        for i in 0..r.entropy.len() {
            let c2 = r.chi2[i][1];
            r.regions.push(RegionInsight {
                offset:      r.entropy[i][0] as usize,
                entropy:     r.entropy[i][1],
                chi2:        c2,
                chi2_p:      chi2_pvalue(c2, 255),
                serial_corr: r.serial_corr[i][1],
                hamming:     r.hamming[i][1],
                suspicious:  thr.is_suspicious(r.entropy[i][1], c2, r.serial_corr[i][1], k),
            });
        }
        r.thresholds = thr;

        // Aggregate statistics.
        let global_chi2 = compute_chi2(data);
        let (ks_d, ks_p) = ks_uniform_test(data);
        let (runs_z, runs_p) = runs_test(data);
        let mean_window_p = if r.regions.is_empty() { 1.0 } else {
            r.regions.iter().map(|reg| reg.chi2_p).sum::<f64>() / r.regions.len() as f64
        };
        r.stats = FileStatistics {
            entropy_stats:  MetricStats::from_series(&r.entropy),
            chi2_stats:     MetricStats::from_series(&r.chi2),
            serial_stats:   MetricStats::from_series(&r.serial_corr),
            hamming_stats:  MetricStats::from_series(&r.hamming),
            ks_d, ks_p,
            global_chi2,
            global_chi2_p:  chi2_pvalue(global_chi2, 255),
            runs_z, runs_p,
            mean_window_p,
        };

        r
    }

    fn reapply_k(result: &mut AnalysisResult, k: f64) {
        let thr = AnomalyThresholds::from_metrics(&result.entropy, &result.chi2, &result.serial_corr);
        for reg in &mut result.regions {
            reg.suspicious = thr.is_suspicious(reg.entropy, reg.chi2, reg.serial_corr, k);
        }
        result.thresholds = thr;
    }
}

// ── Math ──────────────────────────────────────────────────────────────────────

fn compute_entropy(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let len = data.len() as f64;
    hist.iter()
        .filter(|&&c| c > 0)
        .map(|&c| { let p = c as f64 / len; -p * p.log2() })
        .sum()
}

fn compute_chi2(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut hist = [0usize; MAX_BYTE];
    for &b in data { hist[b as usize] += 1; }
    let exp = data.len() as f64 / 256.0;
    hist.iter().map(|&o| { let d = o as f64 - exp; d * d / exp }).sum()
}

fn serial_correlation(data: &[u8]) -> f64 {
    if data.len() < 2 { return 0.0; }
    let n = data.len() as f64;
    let (mut sum, mut sum_sq, mut serial_sum) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..data.len() {
        let x = data[i] as f64;
        let y = data[(i + 1) % data.len()] as f64;
        sum        += x;
        sum_sq     += x * x;
        serial_sum += x * y;
    }
    let num = n * serial_sum - sum * sum;
    let den = n * sum_sq    - sum * sum;
    if den.abs() < f64::EPSILON { 0.0 } else { num / den }
}

fn hamming_weight(data: &[u8]) -> f64 {
    if data.is_empty() { return 0.0; }
    data.iter().map(|b| b.count_ones()).sum::<u32>() as f64 / data.len() as f64
}

fn ngram_uniqueness(data: &[u8], n: usize) -> f64 {
    if data.len() < n { return 0.0; }
    let total = data.len() - n + 1;
    let unique: HashSet<&[u8]> = (0..total).map(|i| &data[i..i + n]).collect();
    unique.len() as f64 / total as f64
}

fn normalize(data: &[[f64; 2]]) -> (Vec<[f64; 2]>, f64, f64) {
    if data.is_empty() { return (Vec::new(), 0.0, 1.0); }
    let min   = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
    let max   = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let range = (max - min).max(f64::EPSILON);
    (data.iter().map(|p| [p[0], (p[1] - min) / range]).collect(), min, max)
}

// ── UI helpers ────────────────────────────────────────────────────────────────

fn card_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(14.0),
        outer_margin: Margin::symmetric(0.0, 4.0),
        rounding:     Rounding::same(6.0),
        fill:         pal::PANEL,
        stroke:       Stroke::new(1.0, pal::BORDER),
        ..Default::default()
    }
}

fn badge(ui: &mut egui::Ui, label: &str, fg: Color32, bg: Color32) {
    let font_id = FontId::proportional(11.0);
    let text_width = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font_id.clone(), fg).size().x);
    let desired = Vec2::new(text_width + 18.0, 20.0);
    let (rect, _) = ui.allocate_at_least(desired, egui::Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font_id, fg);
}

/// Small export button.  Returns `true` if clicked.
fn export_btn(ui: &mut egui::Ui) -> bool {
    ui.add(
        egui::Button::new(RichText::new("⬇ SVG").size(10.0).color(pal::RED))
            .fill(pal::RED_FAINT)
            .stroke(Stroke::new(1.0, pal::RED_MID))
            .rounding(Rounding::same(3.0))
            .min_size(Vec2::new(44.0, 16.0)),
    ).clicked()
}

fn metric_data<'a>(slot: &MetricSlot, result: &'a AnalysisResult) -> &'a [[f64; 2]] {
    match slot.key {
        "entropy" => &result.entropy,
        "chi2"    => &result.chi2,
        "serial"  => &result.serial_corr,
        "hamming" => &result.hamming,
        "bigram"  => &result.bigram_scores,
        "trigram" => &result.trigram_scores,
        _         => &[],
    }
}

/// Returns a (mean, sd) pair for the chosen metric slot from its stats field.
fn metric_mean_sd(slot: &MetricSlot, result: &AnalysisResult) -> Option<(f64, f64)> {
    let ms = match slot.key {
        "entropy" => &result.stats.entropy_stats,
        "chi2"    => &result.stats.chi2_stats,
        "serial"  => &result.stats.serial_stats,
        "hamming" => &result.stats.hamming_stats,
        _         => return None,
    };
    Some((ms.mean, ms.sd))
}

/// Plot one windowed metric.
/// `stat_ref`: if Some((mean, sd)), draws μ and μ±k·σ reference lines.
fn plot_metric(
    ui:           &mut egui::Ui,
    title:        &str,
    unit:         &str,
    data:         &[[f64; 2]],
    file_index:   usize,
    slot_index:   usize,
    do_normalize: bool,
    stat_ref:     Option<(f64, f64, f64)>,  // (mean, sd, k)
) {
    let (plot_pts, raw_min, raw_max) = if do_normalize {
        normalize(data)
    } else {
        let min = data.iter().map(|p| p[1]).fold(f64::INFINITY,     f64::min);
        let max = data.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        (data.to_vec(), min, max)
    };

    // Reference line values in the plot's y-coordinate space.
    let hlines: Vec<(f64, &str)> = if let Some((mean, sd, k)) = stat_ref {
        if do_normalize {
            let range = (raw_max - raw_min).max(f64::EPSILON);
            let to_n = |v: f64| (v - raw_min) / range;
            vec![
                (to_n(mean),          "μ"),
                (to_n(mean + k * sd), "μ+kσ"),
                (to_n(mean - k * sd), "μ−kσ"),
            ]
        } else {
            vec![
                (mean,          "μ"),
                (mean + k * sd, "μ+kσ"),
                (mean - k * sd, "μ−kσ"),
            ]
        }
    } else {
        vec![]
    };

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong().color(pal::TEXT).size(13.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Export button — rightmost.
                if export_btn(ui) {
                    // Build SVG reference lines from raw stat values.
                    let svg_refs: Vec<(f64, &str)> = if let Some((mean, sd, k)) = stat_ref {
                        vec![
                            (mean,          "μ"),
                            (mean + k * sd, "μ+kσ"),
                            (mean - k * sd, "μ−kσ"),
                        ]
                    } else { vec![] };
                    let svg = svg_line_chart(
                        data,
                        title,
                        "File offset",
                        &format!("{title} ({unit})"),
                        &svg_refs,
                    );
                    save_svg(svg, &format!("{title}_{file_index}").to_lowercase().replace(' ', "_"));
                }

                let range_label = if do_normalize {
                    format!("{unit}  [{:.3} – {:.3}]  norm.", raw_min, raw_max)
                } else {
                    format!("{unit}  [{:.3} – {:.3}]", raw_min, raw_max)
                };
                ui.label(RichText::new(range_label).size(11.0).color(pal::MUTED));
            });
        });
        ui.add_space(6.0);

        let colors = [
            Color32::from_rgb(180, 180, 180), // μ
            Color32::from_rgb(200, 100, 100), // μ+kσ
            Color32::from_rgb(200, 100, 100), // μ−kσ
        ];
        let hline_labels = ["μ", "μ+kσ", "μ−kσ"];

        Plot::new(format!("{}_{}_{}", title, file_index, slot_index))
            .height(160.0)
            .legend(Legend::default())
            .show_axes([true, true])
            .show_grid([true, true])
            .auto_bounds([true, true].into())
            .set_margin_fraction(Vec2::new(0.02, 0.12))
            .x_axis_formatter(|mark, _, _| format!("0x{:X}", mark.value as usize))
            .y_axis_formatter(|mark, _, _| format!("{:.3}", mark.value))
            .label_formatter(move |name, point| {
                if do_normalize {
                    let real = point.y * (raw_max - raw_min) + raw_min;
                    format!(
                        "offset: 0x{:X}\n{}: {:.4} (norm)\nraw:    {:.4}",
                        point.x as usize, name, point.y, real
                    )
                } else {
                    format!("offset: 0x{:X}\n{}: {:.4}", point.x as usize, name, point.y)
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(PlotPoints::from(plot_pts))
                        .color(pal::RED)
                        .width(1.6)
                        .name(title),
                );
                for (i, &(y_val, lbl)) in hlines.iter().enumerate() {
                    plot_ui.hline(
                        HLine::new(y_val)
                            .color(colors[i])
                            .width(if i == 0 { 1.5 } else { 1.0 })
                            .style(if i == 0 {
                                egui_plot::LineStyle::Solid
                            } else {
                                egui_plot::LineStyle::Dashed { length: 6.0 }
                            })
                            .name(lbl),
                    );
                }
            });
    });
}

/// Byte-distribution bar chart panel.
/// Returns `true` if the export button was clicked.
fn plot_distribution(ui: &mut egui::Ui, result: &AnalysisResult, file_name: &str, file_index: usize) {
    let max_freq = result.byte_dist.iter().cloned().fold(0.0f64, f64::max).max(f64::EPSILON);
    // Expected frequency under uniform distribution (for reference line).
    let uniform_freq_norm = (1.0 / MAX_BYTE as f64) / max_freq;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Byte Frequency Distribution")
                    .strong().color(pal::TEXT).size(13.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if export_btn(ui) {
                    let svg = svg_bar_chart(
                        &result.byte_dist,
                        &format!("Byte Frequency Distribution — {file_name}"),
                    );
                    save_svg(svg, &format!("byte_dist_{file_index}"));
                }
                ui.label(
                    RichText::new(format!(
                        "0x00 – 0xFF  ·  normalised  [max={:.5}]", max_freq
                    ))
                    .size(11.0)
                    .color(pal::MUTED),
                );
            });
        });
        ui.add_space(6.0);

        let bars: Vec<Bar> = (0..MAX_BYTE)
            .map(|i| {
                Bar::new(i as f64, result.byte_dist[i] / max_freq)
                    .width(0.8)
                    .fill(pal::RED_MID)
                    .stroke(Stroke::new(0.0, pal::RED))
                    .name(format!("0x{:02X}", i))
            })
            .collect();

        Plot::new(format!("byte_dist_{}", file_index))
            .height(200.0)
            .show_grid([true, true])
            .auto_bounds([true, true].into())
            .x_axis_formatter(|mark, _, _| format!("0x{:02X}", mark.value as u8))
            .y_axis_formatter(|mark, _, _| format!("{:.3}", mark.value))
            .label_formatter(move |name, point| {
                format!("byte: {}\nrel. freq: {:.5}", name, point.y * max_freq)
            })
            .show(ui, |pui| {
                pui.bar_chart(BarChart::new(bars).color(pal::RED).name("freq"));
                // Uniform reference.
                pui.hline(
                    HLine::new(uniform_freq_norm)
                        .color(Color32::from_rgb(100, 100, 100))
                        .width(1.2)
                        .style(egui_plot::LineStyle::Dashed { length: 6.0 })
                        .name("uniform"),
                );
            });
    });
}

/// Anomalous-regions table.
/// Returns `Some(byte_offset)` if the user clicked a row.
fn suspicious_regions(ui: &mut egui::Ui, result: &AnalysisResult) -> Option<usize> {
    let suspicious: Vec<&RegionInsight> =
        result.regions.iter().filter(|r| r.suspicious).collect();
    let mut jump_to: Option<usize> = None;

    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Anomalous Regions").strong().color(pal::TEXT).size(13.0));
            ui.add_space(6.0);
            badge(ui, &format!("{} flagged", suspicious.len()), pal::RED, pal::RED_LIGHT);
            ui.add_space(8.0);
            let t = &result.thresholds;
            ui.label(
                RichText::new(format!(
                    "entropy μ={:.3} σ={:.3}  ·  χ² μ={:.1} σ={:.1}  ·  serial μ={:.4} σ={:.4}",
                    t.entropy_mean, t.entropy_sd,
                    t.chi2_mean,    t.chi2_sd,
                    t.serial_mean,  t.serial_sd,
                ))
                .size(10.0)
                .color(pal::MUTED),
            );
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Click a row to jump to that offset in the Hex Dump.")
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        if suspicious.is_empty() {
            ui.label(RichText::new("No anomalies detected.").color(pal::MUTED).size(12.0));
            return;
        }

        let cols = ["Offset", "Entropy", "Chi²", "p(χ²)", "Serial ρ", "Hamming", ""];
        egui::Grid::new("regions_grid")
            .num_columns(cols.len())
            .striped(true)
            .min_col_width(72.0)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for col in &cols {
                    ui.label(RichText::new(*col).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for reg in &suspicious {
                    let resp_offset = ui.add(egui::SelectableLabel::new(
                        false,
                        RichText::new(format!("0x{:08X}", reg.offset)).monospace().size(12.0),
                    ));
                    ui.label(RichText::new(format!("{:.4}", reg.entropy)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.2}",  reg.chi2)).monospace().size(12.0));

                    // p-value column: red if significant, green if not.
                    let p_col = if reg.chi2_p < 0.05 { pal::RED } else { pal::GREEN };
                    ui.label(
                        RichText::new(format!("{:.4}", reg.chi2_p))
                            .monospace().size(12.0).color(p_col),
                    );

                    ui.label(RichText::new(format!("{:.4}", reg.serial_corr)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.4}", reg.hamming)).monospace().size(12.0));

                    let go = ui.add(
                        egui::Button::new(RichText::new("⟶ Hex").size(11.0).color(pal::RED))
                            .fill(pal::RED_FAINT)
                            .stroke(Stroke::new(1.0, pal::RED_MID))
                            .rounding(Rounding::same(4.0))
                            .min_size(Vec2::new(56.0, 18.0)),
                    );
                    if resp_offset.clicked() || go.clicked() {
                        jump_to = Some(reg.offset);
                    }
                    ui.end_row();
                }
            });
    });

    jump_to
}

/// Statistics summary tab.
fn statistics_tab(
    ui:        &mut egui::Ui,
    result:    &AnalysisResult,
    file_name: &str,
    file_index: usize,
) {
    let s = &result.stats;

    // ── Global randomness tests ──────────────────────────────────────────────
    card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Global Randomness Tests")
                    .strong().color(pal::TEXT).size(13.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(
                    egui::Button::new(RichText::new("⬇ CSV").size(10.0).color(pal::RED))
                        .fill(pal::RED_FAINT)
                        .stroke(Stroke::new(1.0, pal::RED_MID))
                        .rounding(Rounding::same(3.0))
                        .min_size(Vec2::new(44.0, 16.0)),
                ).clicked() {
                    export_csv(result, file_name);
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                "H₀: bytes are i.i.d. Uniform{0,…,255}.  Significance level α = 0.05.",
            )
            .size(10.0)
            .color(pal::MUTED)
            .italics(),
        );
        ui.add_space(8.0);

        let rows: &[(&str, &str, f64, f64, &str, &str)] = &[
            (
                "Kolmogorov–Smirnov",
                &format!("D = {:.5}", s.ks_d),
                s.ks_p,
                0.05,
                "Non-uniform distribution",
                "Consistent with Uniform[0,255]",
            ),
            (
                "Chi² (global, df=255)",
                &format!("χ² = {:.2}", s.global_chi2),
                s.global_chi2_p,
                0.05,
                "Non-uniform distribution",
                "Consistent with Uniform[0,255]",
            ),
            (
                "Wald–Wolfowitz runs",
                &format!("Z = {:.4}", s.runs_z),
                s.runs_p,
                0.05,
                "Non-random sequential structure",
                "Consistent with independent draws",
            ),
        ];

        egui::Grid::new("global_tests_grid")
            .num_columns(5)
            .min_col_width(100.0)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                for h in &["Test", "Statistic", "p-value", "Reject H₀?", "Interpretation"] {
                    ui.label(
                        RichText::new(*h).size(11.0).color(pal::MUTED).strong(),
                    );
                }
                ui.end_row();

                for &(test, stat_str, p, alpha, reject_interp, accept_interp) in rows {
                    let reject = p < alpha;
                    let p_col  = if reject { pal::RED } else { pal::GREEN };

                    ui.label(RichText::new(test).size(12.0));
                    ui.label(RichText::new(stat_str).monospace().size(12.0));
                    ui.label(
                        RichText::new(if p < 0.0001 {
                            "< 0.0001".to_owned()
                        } else {
                            format!("{:.4}", p)
                        })
                        .monospace().size(12.0).color(p_col),
                    );
                    ui.label(
                        RichText::new(if reject { "Yes" } else { "No" })
                            .size(12.0).color(p_col).strong(),
                    );
                    ui.label(
                        RichText::new(if reject { reject_interp } else { accept_interp })
                            .size(11.0).color(p_col),
                    );
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Mean per-window χ² p-value: {:.4}   (≈ 0.5 expected for uniform random data)",
                s.mean_window_p
            ))
            .size(11.0)
            .color(pal::MUTED),
        );
    });

    ui.add_space(8.0);

    // ── Per-metric aggregate statistics ──────────────────────────────────────
    card_frame().show(ui, |ui| {
        ui.label(
            RichText::new("Per-metric Statistics (windowed)")
                .strong().color(pal::TEXT).size(13.0),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Window size: {} bytes", result.window_size))
                .size(10.0).color(pal::MUTED).italics(),
        );
        ui.add_space(8.0);

        let rows: &[(&str, &str, &str, &MetricStats, Option<(f64, f64)>)] = &[
            ("Entropy",            "bits / symbol", "[0, 8]",          &s.entropy_stats, Some((0.0, 8.0))),
            ("Chi² statistic",     "statistic",     "df=255, E=255",   &s.chi2_stats,    None),
            ("Serial correlation", "ρ",             "[-1, 1]",         &s.serial_stats,  Some((-1.0, 1.0))),
            ("Hamming weight",     "bits / byte",   "[0, 8]",          &s.hamming_stats, Some((0.0, 8.0))),
        ];

        egui::Grid::new("metric_stats_grid")
            .num_columns(7)
            .min_col_width(72.0)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                for h in &["Metric", "Unit", "Theoretical", "Mean", "Std. Dev.", "Min", "Max"] {
                    ui.label(RichText::new(*h).size(11.0).color(pal::MUTED).strong());
                }
                ui.end_row();

                for &(name, unit, theory, ms, _) in rows {
                    ui.label(RichText::new(name).size(12.0));
                    ui.label(RichText::new(unit).size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(theory).monospace().size(11.0).color(pal::MUTED));
                    ui.label(RichText::new(format!("{:.5}", ms.mean)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.sd)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.min)).monospace().size(12.0));
                    ui.label(RichText::new(format!("{:.5}", ms.max)).monospace().size(12.0));
                    ui.end_row();
                }
            });
    });

    ui.add_space(8.0);

    // ── Quick interpretation guide ────────────────────────────────────────────
    card_frame().show(ui, |ui| {
        ui.label(
            RichText::new("Interpretation Guide")
                .strong().color(pal::TEXT).size(13.0),
        );
        ui.add_space(6.0);

        let items = [
            ("Entropy ≈ 8 bits/symbol",
             "Byte values are near-maximally uncertain — typical of compressed or encrypted data."),
            ("Entropy ≪ 8 bits/symbol",
             "Significant redundancy.  Structured, text, or sparse data."),
            ("Chi² p-value ≪ 0.05",
             "Byte distribution deviates significantly from uniform.  Likely structured content."),
            ("Serial correlation |ρ| > 0.1",
             "Adjacent bytes are correlated — sequential structure or repeated patterns present."),
            ("Hamming weight ≈ 4.0",
             "Expected for uniformly random bytes (each bit ≈ 0.5)."),
            ("KS p-value < 0.05",
             "Global CDF diverges from uniform — byte usage is skewed or concentrated."),
            ("Runs test p-value < 0.05",
             "Runs of bytes above/below the median are non-random — implies local structure."),
        ];

        for (term, explanation) in &items {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(*term).monospace().size(12.0).color(pal::RED));
                ui.label(RichText::new("—").size(12.0).color(pal::MUTED));
                ui.label(RichText::new(*explanation).size(12.0));
            });
            ui.add_space(3.0);
        }
    });

    // ── B&W exports for all metrics ───────────────────────────────────────────
    ui.add_space(8.0);
    card_frame().show(ui, |ui| {
        ui.label(
            RichText::new("Export All Plots (B&W SVG)")
                .strong().color(pal::TEXT).size(13.0),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let exports: &[(&str, &str, &[[f64; 2]])] = &[
                ("Entropy",            "entropy",  &result.entropy),
                ("Chi²",               "chi2",     &result.chi2),
                ("Serial Correlation", "serial",   &result.serial_corr),
                ("Hamming Weight",     "hamming",  &result.hamming),
                ("Bigram Uniqueness",  "bigram",   &result.bigram_scores),
                ("Trigram Uniqueness", "trigram",  &result.trigram_scores),
            ];
            for &(label, stem, data) in exports {
                if ui.add(
                    egui::Button::new(
                        RichText::new(format!("⬇ {label}")).size(11.0).color(pal::RED),
                    )
                    .fill(pal::RED_FAINT)
                    .stroke(Stroke::new(1.0, pal::RED_MID))
                    .rounding(Rounding::same(4.0)),
                ).clicked() {
                    let svg = svg_line_chart(
                        data,
                        &format!("{label} — {file_name}"),
                        "File offset",
                        label,
                        &[],
                    );
                    save_svg(svg, &format!("{stem}_{file_index}"));
                }
                ui.add_space(4.0);
            }
            if ui.add(
                egui::Button::new(
                    RichText::new("⬇ Byte Dist").size(11.0).color(pal::RED),
                )
                .fill(pal::RED_FAINT)
                .stroke(Stroke::new(1.0, pal::RED_MID))
                .rounding(Rounding::same(4.0)),
            ).clicked() {
                let svg = svg_bar_chart(
                    &result.byte_dist,
                    &format!("Byte Frequency Distribution — {file_name}"),
                );
                save_svg(svg, &format!("byte_dist_{file_index}"));
            }
            ui.add_space(4.0);
            if ui.add(
                egui::Button::new(
                    RichText::new("⬇ CSV (all metrics)").size(11.0).color(pal::RED),
                )
                .fill(pal::RED_FAINT)
                .stroke(Stroke::new(1.0, pal::RED_MID))
                .rounding(Rounding::same(4.0)),
            ).clicked() {
                export_csv(result, file_name);
            }
        });
    });
}

/// Hex dump with virtual scrolling, optional auto-scroll, and region highlighting.
fn hex_view(
    ui:        &mut egui::Ui,
    data:      &[u8],
    scroll_to: Option<usize>,
    highlight: Option<(usize, usize)>,
) {
    let total_lines = (data.len() + HEX_WIDTH - 1) / HEX_WIDTH;
    let row_height  = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;

    let mut scroll = egui::ScrollArea::vertical()
        .id_source("hex_scroll")
        .auto_shrink([false, false]);

    if let Some(target_byte) = scroll_to {
        let target_line = target_byte / HEX_WIDTH;
        let visible_rows = 3usize;
        let y = (target_line.saturating_sub(visible_rows)) as f32 * row_height;
        scroll = scroll.vertical_scroll_offset(y);
    }

    let hl_range: Option<std::ops::RangeInclusive<usize>> = highlight.map(|(start, len)| {
        let first = start / HEX_WIDTH;
        let last  = start.saturating_add(len).saturating_sub(1) / HEX_WIDTH;
        first..=last
    });

    scroll.show_rows(ui, row_height, total_lines, |ui, row_range| {
        for line in row_range {
            let line_offset = line * HEX_WIDTH;
            let slice_end   = (line_offset + HEX_WIDTH).min(data.len());
            let slice       = &data[line_offset..slice_end];

            let mut hex   = format!("{:08X}  ", line_offset);
            let mut ascii = String::with_capacity(HEX_WIDTH);
            for (i, b) in slice.iter().enumerate() {
                if i == 8 { hex.push(' '); }
                hex.push_str(&format!("{:02X} ", b));
                ascii.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '·' });
            }
            let missing = HEX_WIDTH - slice.len();
            for i in 0..missing {
                if slice.len() + i == 8 { hex.push(' '); }
                hex.push_str("   ");
            }
            hex.push_str(&format!(" │ {}", ascii));

            let is_highlighted = hl_range.as_ref().map_or(false, |r| r.contains(&line));

            if is_highlighted {
                let desired = Vec2::new(ui.available_width(), row_height);
                let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                ui.painter().rect(
                    rect.expand(1.0),
                    Rounding::same(2.0),
                    pal::HL_BG,
                    Stroke::new(1.0, pal::HL_BORDER),
                );
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    &hex,
                    FontId::monospace(12.0),
                    pal::HL_BORDER,
                );
            } else {
                ui.label(RichText::new(&hex).monospace().size(12.0).color(pal::TEXT));
            }
        }
    });
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        // ── Theme ─────────────────────────────────────────────
        let mut vis = ctx.style().visuals.clone();
        vis.override_text_color              = Some(pal::TEXT);
        vis.panel_fill                       = pal::BG;
        vis.window_fill                      = pal::PANEL;
        vis.faint_bg_color                   = pal::RED_FAINT;
        vis.extreme_bg_color                 = pal::PANEL;
        vis.widgets.noninteractive.bg_fill   = pal::PANEL;
        vis.widgets.noninteractive.bg_stroke = Stroke::new(1.0, pal::BORDER);
        vis.widgets.inactive.bg_fill         = pal::RED_FAINT;
        vis.widgets.hovered.bg_fill          = pal::RED_LIGHT;
        vis.widgets.hovered.bg_stroke        = Stroke::new(1.0, pal::RED_MID);
        vis.widgets.active.bg_fill           = pal::RED;
        vis.selection.bg_fill                = pal::RED_LIGHT;
        vis.selection.stroke                 = Stroke::new(1.0, pal::RED);
        ctx.set_visuals(vis);

        // ── Top bar ───────────────────────────────────────────
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
                        if let Some(file) = Self::load_file() {
                            self.files.push(file);
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
                        let (w, k) = (self.window_size, self.anomaly_k);
                        for file in &mut self.files {
                            file.result = Some(Self::analyze(&file.data, w, k));
                        }
                    }

                    ui.separator();

                    ui.label(RichText::new("Window:").size(12.0).color(pal::MUTED));
                    ui.scope(|ui| {
                        let vis = ui.visuals_mut();
                        vis.widgets.inactive.bg_fill   = pal::RED_FAINT;
                        vis.widgets.inactive.fg_stroke = Stroke::new(1.0, pal::RED);
                        vis.widgets.hovered.bg_fill    = pal::RED_LIGHT;
                        vis.widgets.active.bg_fill     = pal::RED_LIGHT;
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
                    let k_resp = ui.add(
                        egui::DragValue::new(&mut self.anomaly_k)
                            .clamp_range(0.5..=5.0_f64)
                            .speed(0.05)
                            .max_decimals(2),
                    );
                    if k_resp.changed() {
                        let k = self.anomaly_k;
                        for file in &mut self.files {
                            if let Some(ref mut result) = file.result {
                                App::reapply_k(result, k);
                            }
                        }
                    }

                    ui.separator();

                    let hex_label = if self.show_hex { "✕ Hex" } else { "⟨/⟩ Hex" };
                    if ui.add(
                        egui::Button::new(
                            RichText::new(hex_label)
                                .size(12.0)
                                .color(if self.show_hex { Color32::WHITE } else { pal::RED }),
                        )
                        .fill(if self.show_hex { pal::RED } else { pal::RED_FAINT })
                        .stroke(Stroke::new(1.0, pal::RED))
                        .rounding(Rounding::same(5.0)),
                    ).clicked() {
                        self.show_hex = !self.show_hex;
                    }

                    if let Some((off, len)) = self.hex_highlight {
                        ui.separator();
                        ui.label(
                            RichText::new(format!("⚑ 0x{:08X} + {}B", off, len))
                                .size(11.0).color(pal::RED).monospace(),
                        );
                        if ui.add(
                            egui::Button::new(RichText::new("✕").size(11.0).color(pal::MUTED))
                                .fill(pal::RED_FAINT)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(3.0)),
                        ).on_hover_text("Clear highlight").clicked() {
                            self.hex_highlight = None;
                        }
                    }
                });
            });

        // ── Left: file list ───────────────────────────────────
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
                        if self.files.is_empty() {
                            ui.label(
                                RichText::new("No files loaded.").size(12.0).color(pal::MUTED),
                            );
                        }
                        for (idx, file) in self.files.iter().enumerate() {
                            let selected = idx == self.sel_file;
                            if ui.add(egui::SelectableLabel::new(
                                selected,
                                RichText::new(&file.name).size(12.0),
                            )).clicked() {
                                self.sel_file = idx;
                            }

                            if let Some(ref r) = file.result {
                                let n = r.regions.iter().filter(|r| r.suspicious).count();
                                // Show randomness summary in sidebar.
                                let ksp = r.stats.ks_p;
                                let summary_col = if ksp < 0.05 { pal::RED } else { pal::GREEN };
                                ui.label(
                                    RichText::new(format!(
                                        "  KS p={:.3}  χ²p={:.3}",
                                        ksp, r.stats.global_chi2_p,
                                    ))
                                    .size(10.0)
                                    .color(summary_col),
                                );
                                if n > 0 {
                                    ui.label(
                                        RichText::new(format!("  ⚠ {} regions", n))
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

        // ── Right: hex dump ───────────────────────────────────
        let scroll_target  = if self.hex_do_scroll { self.hex_highlight.map(|(off, _)| off) } else { None };
        let highlight_copy = self.hex_highlight;
        if self.hex_do_scroll { self.hex_do_scroll = false; }

        if self.show_hex {
            egui::SidePanel::right("hexdump")
                .resizable(true)
                .default_width(540.0)
                .frame(Frame {
                    inner_margin: Margin::same(12.0),
                    fill:         pal::PANEL,
                    stroke:       Stroke::new(1.0, pal::BORDER),
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("HEX DUMP").size(10.0).color(pal::MUTED).strong(),
                        );
                        if let Some((off, len)) = highlight_copy {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!("⚑ 0x{:08X} – 0x{:08X}", off, off + len))
                                    .size(11.0).color(pal::RED).monospace(),
                            );
                        }
                    });

                    if let Some(file) = self.files.get(self.sel_file) {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&file.name).size(12.0).strong());
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("{} bytes", file.data.len()))
                                    .size(11.0).color(pal::MUTED),
                            );
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "OFFSET    00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F  │ ASCII"
                            )
                            .monospace().size(11.0).color(pal::MUTED),
                        );
                        ui.add_space(2.0);
                        hex_view(ui, &file.data, scroll_target, highlight_copy);
                    } else {
                        ui.add_space(24.0);
                        ui.label(
                            RichText::new("Load a binary file to inspect.")
                                .color(pal::MUTED).size(12.0),
                        );
                    }
                });
        }

        // ── Central: tabs ─────────────────────────────────────
        let mut pending_jump: Option<usize> = None;

        egui::CentralPanel::default()
            .frame(Frame {
                fill:         pal::BG,
                inner_margin: Margin::same(16.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, tab) in ["Metrics", "Distribution", "Anomalies", "Statistics"].iter().enumerate() {
                        let active = self.active_tab == i;
                        if ui.add(
                            egui::Button::new(
                                RichText::new(*tab)
                                    .size(12.0)
                                    .color(if active { pal::RED } else { pal::MUTED })
                                    .strong(),
                            )
                            .fill(if active { pal::RED_LIGHT } else { pal::PANEL })
                            .stroke(Stroke::new(1.0, if active { pal::RED } else { pal::BORDER }))
                            .rounding(Rounding::same(5.0)),
                        ).clicked() {
                            self.active_tab = i;
                        }
                    }
                });
                ui.add_space(10.0);

                if self.files.is_empty() {
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

                        // Metric reorder row (Metrics tab only).
                        if self.active_tab == 0 {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("Plot order:").size(11.0).color(pal::MUTED),
                                );
                                let n = self.metric_order.len();
                                let mut swap: Option<(usize, usize)> = None;
                                for i in 0..n {
                                    let label = self.metric_order[i].label;
                                    ui.add_space(2.0);
                                    card_frame().inner_margin(Margin::same(4.0)).show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(label).size(11.0).color(pal::TEXT),
                                            );
                                            if i > 0 && ui.add(
                                                egui::Button::new(
                                                    RichText::new("◀").size(10.0).color(pal::RED),
                                                )
                                                .fill(pal::RED_FAINT)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(3.0))
                                                .min_size(Vec2::new(18.0, 16.0)),
                                            ).on_hover_text("Move left").clicked() {
                                                swap = Some((i - 1, i));
                                            }
                                            if i + 1 < n && ui.add(
                                                egui::Button::new(
                                                    RichText::new("▶").size(10.0).color(pal::RED),
                                                )
                                                .fill(pal::RED_FAINT)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(3.0))
                                                .min_size(Vec2::new(18.0, 16.0)),
                                            ).on_hover_text("Move right").clicked() {
                                                swap = Some((i, i + 1));
                                            }
                                        });
                                    });
                                }
                                if let Some((a, b)) = swap {
                                    self.metric_order.swap(a, b);
                                }
                            });
                            ui.add_space(8.0);
                        }

                        let anomaly_k = self.anomaly_k;

                        for idx in 0..self.files.len() {
                            let file_name = self.files[idx].name.clone();
                            let result    = self.files[idx].result.clone();
                            let Some(result) = result else { continue };

                            egui::CollapsingHeader::new(
                                RichText::new(&file_name).size(13.0).strong(),
                            )
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.add_space(4.0);
                                match self.active_tab {
                                    0 => {
                                        let order = self.metric_order.clone();
                                        for (si, slot) in order.iter().enumerate() {
                                            let data     = metric_data(slot, &result);
                                            let stat_ref = metric_mean_sd(slot, &result)
                                                .map(|(m, s)| (m, s, anomaly_k));
                                            plot_metric(
                                                ui, slot.label, slot.unit,
                                                data, idx, si, slot.norm,
                                                stat_ref,
                                            );
                                            ui.add_space(4.0);
                                        }
                                    }
                                    1 => {
                                        plot_distribution(ui, &result, &file_name, idx);
                                    }
                                    2 => {
                                        if let Some(off) = suspicious_regions(ui, &result) {
                                            pending_jump = Some(off);
                                            let ws = result.window_size.max(1);
                                            self.hex_highlight = Some((off, ws));
                                            self.hex_do_scroll = true;
                                        }
                                    }
                                    3 => {
                                        statistics_tab(ui, &result, &file_name, idx);
                                    }
                                    _ => {}
                                }
                            });
                            ui.add_space(8.0);
                        }
                    });
            });

        if pending_jump.is_some() {
            self.show_hex = true;
        }
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ELICIUNT")
            .with_inner_size([1440.0, 880.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ELICIUNT",
        opts,
        Box::new(|_cc| Box::new(App::default())),
    )
}