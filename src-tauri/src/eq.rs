use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use rodio::{Sample, Source};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::viz::VizTap;

pub const BAND_COUNT: usize = 10;
/// ISO centers kept so older graphic curves and built-in presets load as peaking bands.
pub const BAND_FREQS: [f32; BAND_COUNT] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const BAND_Q: f32 = std::f32::consts::SQRT_2;
pub const GAIN_MIN: f32 = -12.0;
pub const GAIN_MAX: f32 = 12.0;
pub const PREAMP_MIN: f32 = -30.0;
pub const PREAMP_MAX: f32 = 12.0;
pub const Q_MIN: f32 = 0.1;
pub const Q_MAX: f32 = 10.0;
pub const FREQ_MIN: f32 = 20.0;
pub const FREQ_MAX: f32 = 20_000.0;
pub const MAX_USER_PRESETS: usize = 20;
pub const WORKING_PRESET_ID: &str = "custom";
pub const FLAT_PRESET_ID: &str = "flat";

const GAIN_EPS: f32 = 0.01;
const DESIGN_RATE: f64 = 48_000.0;

/// Default-mode sliders. Index 0 is a low shelf and index 9 is a high shelf.
/// The other three are peaking bands. Hidden bands stay in the same cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    pub kind: FilterKind,
    pub freq: f32,
    pub q: f32,
    pub gain: f32,
}

#[derive(Debug, Clone, Copy)]
struct MacroLock {
    index: usize,
    kind: FilterKind,
    freq: f32,
    q: f32,
}

const MACRO_LOCKS: [MacroLock; 5] = [
    MacroLock {
        index: 0,
        kind: FilterKind::LowShelf,
        freq: 90.0,
        q: 0.71,
    },
    MacroLock {
        index: 2,
        kind: FilterKind::Peak,
        freq: 300.0,
        q: 1.0,
    },
    MacroLock {
        index: 4,
        kind: FilterKind::Peak,
        freq: 1000.0,
        q: 1.0,
    },
    MacroLock {
        index: 7,
        kind: FilterKind::Peak,
        freq: 3500.0,
        q: 1.0,
    },
    MacroLock {
        index: 9,
        kind: FilterKind::HighShelf,
        freq: 10_000.0,
        q: 0.71,
    },
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqUserPreset {
    pub id: String,
    pub name: String,
    pub bands: [EqBand; BAND_COUNT],
    pub preamp: f32,
    #[serde(default = "default_true")]
    pub auto_preamp: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPersist {
    pub enabled: bool,
    pub preset_id: String,
    pub bands: [EqBand; BAND_COUNT],
    pub preamp: f32,
    pub auto_preamp: bool,
    pub custom_presets: Vec<EqUserPreset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EqPersistRaw {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_preset_id")]
    preset_id: String,
    #[serde(default)]
    bands: Option<Vec<EqBand>>,
    #[serde(default)]
    gains: Option<Vec<f32>>,
    #[serde(default)]
    preamp: f32,
    #[serde(default = "default_true")]
    auto_preamp: bool,
    #[serde(default)]
    custom_presets: Vec<EqUserPreset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EqUserPresetRaw {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    bands: Option<Vec<EqBand>>,
    #[serde(default)]
    gains: Option<Vec<f32>>,
    #[serde(default)]
    preamp: f32,
    #[serde(default = "default_true")]
    auto_preamp: bool,
}

impl From<EqPersistRaw> for EqPersist {
    fn from(raw: EqPersistRaw) -> Self {
        Self {
            enabled: raw.enabled,
            preset_id: if raw.preset_id.trim().is_empty() {
                default_preset_id()
            } else {
                raw.preset_id
            },
            bands: bands_from_raw(raw.bands, raw.gains),
            preamp: clamp_preamp(raw.preamp),
            auto_preamp: raw.auto_preamp,
            custom_presets: raw.custom_presets,
        }
    }
}

impl<'de> Deserialize<'de> for EqPersist {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(EqPersistRaw::deserialize(deserializer)?.into())
    }
}

impl From<EqUserPresetRaw> for EqUserPreset {
    fn from(raw: EqUserPresetRaw) -> Self {
        Self {
            id: raw.id,
            name: raw.name,
            bands: bands_from_raw(raw.bands, raw.gains),
            preamp: clamp_preamp(raw.preamp),
            auto_preamp: raw.auto_preamp,
        }
    }
}

impl<'de> Deserialize<'de> for EqUserPreset {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(EqUserPresetRaw::deserialize(deserializer)?.into())
    }
}

impl Default for EqPersist {
    fn default() -> Self {
        Self {
            enabled: false,
            preset_id: default_preset_id(),
            bands: tone_bands(),
            preamp: 0.0,
            auto_preamp: true,
            custom_presets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqUpdate {
    pub enabled: bool,
    pub preset_id: String,
    #[serde(default)]
    pub bands: Vec<EqBand>,
    pub preamp: f32,
    pub auto_preamp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBuiltin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bands: [EqBand; BAND_COUNT],
    pub preamp: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqState {
    pub enabled: bool,
    pub preset_id: String,
    pub bands: [EqBand; BAND_COUNT],
    pub preamp: f32,
    pub auto_preamp: bool,
    /// False when hidden bands, or unlocked bass/treble, are part of the curve.
    pub tone_template: bool,
    pub custom_presets: Vec<EqUserPreset>,
    pub builtins: Vec<EqBuiltin>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EqParams {
    pub enabled: bool,
    pub bands: [EqBand; BAND_COUNT],
    pub preamp: f32,
    pub auto_preamp: bool,
}

impl EqParams {
    pub fn bypass() -> Self {
        Self {
            enabled: false,
            bands: tone_bands(),
            preamp: 0.0,
            auto_preamp: true,
        }
    }

    pub fn from_persist(eq: &EqPersist) -> Self {
        Self {
            enabled: eq.enabled,
            bands: normalize_bands(&eq.bands),
            preamp: clamp_preamp(eq.preamp),
            auto_preamp: eq.auto_preamp,
        }
    }

    pub fn from_update(update: &EqUpdate) -> Self {
        Self {
            enabled: update.enabled,
            bands: normalize_bands(&update.bands),
            preamp: clamp_preamp(update.preamp),
            auto_preamp: update.auto_preamp,
        }
    }
}

pub fn state_from(eq: &EqPersist) -> EqState {
    let bands = normalize_bands(&eq.bands);
    EqState {
        enabled: eq.enabled,
        preset_id: eq.preset_id.clone(),
        bands,
        preamp: clamp_preamp(eq.preamp),
        auto_preamp: eq.auto_preamp,
        tone_template: is_tone_template(&bands),
        custom_presets: eq.custom_presets.clone(),
        builtins: Vec::new(),
    }
}

pub fn state_with_catalog(eq: &EqPersist) -> EqState {
    let mut state = state_from(eq);
    state.builtins = builtin_catalog();
    state
}

pub fn apply_update(eq: &mut EqPersist, update: &EqUpdate) {
    eq.enabled = update.enabled;
    eq.preset_id = if update.preset_id.trim().is_empty() {
        WORKING_PRESET_ID.to_string()
    } else {
        update.preset_id.clone()
    };
    eq.bands = normalize_bands(&update.bands);
    eq.preamp = clamp_preamp(update.preamp);
    eq.auto_preamp = update.auto_preamp;
}

pub fn upsert_user_preset(eq: &mut EqPersist, mut preset: EqUserPreset) -> AppResult<()> {
    preset.name = preset.name.trim().to_string();
    if preset.name.is_empty() {
        preset.name = "Custom EQ".into();
    }
    preset.bands = normalize_bands(&preset.bands);
    preset.preamp = clamp_preamp(preset.preamp);
    if preset.id.trim().is_empty() || is_builtin_id(&preset.id) || preset.id == WORKING_PRESET_ID {
        return Err(AppError::msg("Built-in presets can't be changed"));
    }
    if let Some(existing) = eq
        .custom_presets
        .iter_mut()
        .find(|item| item.id == preset.id)
    {
        *existing = preset.clone();
    } else {
        if eq.custom_presets.len() >= MAX_USER_PRESETS {
            return Err(AppError::msg("You can save up to 20 presets"));
        }
        eq.custom_presets.push(preset.clone());
    }
    eq.preset_id = preset.id;
    eq.bands = preset.bands;
    eq.preamp = preset.preamp;
    eq.auto_preamp = preset.auto_preamp;
    eq.enabled = true;
    Ok(())
}

pub fn delete_user_preset(eq: &mut EqPersist, id: &str) -> AppResult<()> {
    if is_builtin_id(id) || id == WORKING_PRESET_ID {
        return Err(AppError::msg("Built-in presets can't be removed"));
    }
    let before = eq.custom_presets.len();
    eq.custom_presets.retain(|item| item.id != id);
    if eq.custom_presets.len() == before {
        return Err(AppError::msg("Preset not found"));
    }
    if eq.preset_id == id {
        eq.preset_id = FLAT_PRESET_ID.into();
        eq.bands = tone_bands();
        eq.preamp = 0.0;
    }
    Ok(())
}

pub fn builtin_catalog() -> Vec<EqBuiltin> {
    builtins()
        .iter()
        .map(|item| EqBuiltin {
            id: item.id.to_string(),
            name: item.name.to_string(),
            description: item.description.to_string(),
            bands: if item.id == "flat" {
                tone_bands()
            } else {
                graphic_bands(&item.gains)
            },
            preamp: item.preamp,
        })
        .collect()
}

pub fn is_builtin_id(id: &str) -> bool {
    builtins().iter().any(|item| item.id == id)
}

pub fn clamp_gain(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(GAIN_MIN, GAIN_MAX)
}

pub fn clamp_preamp(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(PREAMP_MIN, PREAMP_MAX)
}

pub fn clamp_q(value: f32) -> f32 {
    if !value.is_finite() {
        return 1.0;
    }
    value.clamp(Q_MIN, Q_MAX)
}

pub fn clamp_freq(value: f32) -> f32 {
    if !value.is_finite() {
        return 1000.0;
    }
    value.clamp(FREQ_MIN, FREQ_MAX)
}

pub fn tone_bands() -> [EqBand; BAND_COUNT] {
    let mut bands = [EqBand {
        kind: FilterKind::Peak,
        freq: 1000.0,
        q: 1.0,
        gain: 0.0,
    }; BAND_COUNT];
    let spare = [160.0, 600.0, 2000.0, 6000.0, 14_000.0];
    let mut spare_i = 0;
    for (index, band) in bands.iter_mut().enumerate() {
        if let Some(lock) = MACRO_LOCKS.iter().find(|item| item.index == index) {
            band.kind = lock.kind;
            band.freq = lock.freq;
            band.q = lock.q;
        } else {
            band.freq = spare[spare_i];
            spare_i += 1;
        }
    }
    bands
}

pub fn graphic_bands(gains: &[f32]) -> [EqBand; BAND_COUNT] {
    let mut bands = tone_bands();
    for (index, gain) in gains.iter().take(BAND_COUNT).enumerate() {
        bands[index] = EqBand {
            kind: FilterKind::Peak,
            freq: BAND_FREQS[index],
            q: BAND_Q,
            gain: clamp_gain(*gain),
        };
    }
    bands
}

pub fn normalize_bands(input: &[EqBand]) -> [EqBand; BAND_COUNT] {
    let mut out = tone_bands();
    for (index, band) in input.iter().take(BAND_COUNT).enumerate() {
        let q = clamp_q(band.q);
        out[index] = EqBand {
            kind: band.kind,
            freq: clamp_freq(band.freq),
            q: match band.kind {
                FilterKind::Peak => q,
                FilterKind::LowShelf | FilterKind::HighShelf => q.min(1.0),
            },
            gain: clamp_gain(band.gain),
        };
    }
    out
}

fn bands_from_raw(bands: Option<Vec<EqBand>>, gains: Option<Vec<f32>>) -> [EqBand; BAND_COUNT] {
    if let Some(bands) = bands {
        return normalize_bands(&bands);
    }
    if let Some(gains) = gains {
        return graphic_bands(&gains);
    }
    tone_bands()
}

pub fn is_tone_template(bands: &[EqBand; BAND_COUNT]) -> bool {
    for lock in MACRO_LOCKS {
        let band = bands[lock.index];
        if band.kind != lock.kind
            || (band.freq - lock.freq).abs() > 0.05
            || (band.q - lock.q).abs() > 0.001
        {
            return false;
        }
    }
    for (index, band) in bands.iter().enumerate() {
        if MACRO_LOCKS.iter().any(|lock| lock.index == index) {
            continue;
        }
        if band.gain.abs() >= GAIN_EPS {
            return false;
        }
    }
    true
}

pub fn composite_peak_db(bands: &[EqBand; BAND_COUNT], sample_rate: f64) -> f32 {
    let nyquist = sample_rate * 0.45;
    if !(sample_rate.is_finite() && nyquist > 20.0) {
        return 0.0;
    }
    let mut freqs = [0.0f64; 256 + BAND_COUNT * 9];
    let mut count = 0usize;
    for step in 0..256 {
        let freq = 20.0 * 1000f64.powf(step as f64 / 255.0);
        if freq < nyquist {
            freqs[count] = freq;
            count += 1;
        }
    }
    for band in bands {
        if band.gain.abs() < GAIN_EPS {
            continue;
        }
        let center = f64::from(band.freq);
        if !(20.0..nyquist).contains(&center) {
            continue;
        }
        let half = (center / f64::from(band.q.max(0.1))).max(1.0);
        for step in 0..9 {
            let freq = (center - half) + (half * 2.0) * step as f64 / 8.0;
            if (20.0..nyquist).contains(&freq) && count < freqs.len() {
                freqs[count] = freq;
                count += 1;
            }
        }
    }
    let mut peak = 0.0f64;
    for freq in freqs.iter().take(count).copied() {
        peak = peak.max(magnitude_db(bands, 0.0, sample_rate, freq));
    }
    peak.max(0.0) as f32
}

pub fn effective_preamp(params: &EqParams) -> f32 {
    let user = clamp_preamp(params.preamp);
    if params.auto_preamp {
        let peak = composite_peak_db(&params.bands, DESIGN_RATE);
        if peak > 0.0 {
            return user.min(-peak).max(-48.0);
        }
    }
    user
}

pub fn is_bypass(params: &EqParams) -> bool {
    if !params.enabled {
        return true;
    }
    params.bands.iter().all(|band| band.gain.abs() < GAIN_EPS)
        && effective_preamp(params).abs() < GAIN_EPS
}

#[derive(Clone, Copy, Debug)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Default for BiquadCoeffs {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

fn design_coeffs(sample_rate: f64, band: EqBand) -> Option<BiquadCoeffs> {
    if !(sample_rate.is_finite() && sample_rate >= 1.0) {
        return None;
    }
    let freq = band.freq as f64;
    let q = band.q as f64;
    let gain_db = band.gain as f64;
    if !(freq.is_finite() && q.is_finite() && gain_db.is_finite()) {
        return None;
    }
    if freq <= 0.0 || q <= 0.0 || freq >= sample_rate * 0.49 {
        return None;
    }
    if gain_db.abs() < f64::from(GAIN_EPS) {
        return None;
    }
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
    let cos_w = w0.cos();
    let sin_w = w0.sin();
    let (b0, b1, b2, a0, a1, a2) = match band.kind {
        FilterKind::Peak => {
            let alpha = sin_w / (2.0 * q);
            (
                1.0 + alpha * a,
                -2.0 * cos_w,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w,
                1.0 - alpha / a,
            )
        }
        FilterKind::LowShelf | FilterKind::HighShelf => {
            let shelf_q = q.clamp(0.1, 1.0);
            let inner = (a + 1.0 / a) * (1.0 / shelf_q - 1.0) + 2.0;
            if inner < 0.0 {
                return None;
            }
            let alpha = sin_w / 2.0 * inner.sqrt();
            let sqrt_a = a.sqrt();
            if band.kind == FilterKind::LowShelf {
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w + 2.0 * sqrt_a * alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w),
                    a * ((a + 1.0) - (a - 1.0) * cos_w - 2.0 * sqrt_a * alpha),
                    (a + 1.0) + (a - 1.0) * cos_w + 2.0 * sqrt_a * alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w),
                    (a + 1.0) + (a - 1.0) * cos_w - 2.0 * sqrt_a * alpha,
                )
            } else {
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w + 2.0 * sqrt_a * alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w),
                    a * ((a + 1.0) + (a - 1.0) * cos_w - 2.0 * sqrt_a * alpha),
                    (a + 1.0) - (a - 1.0) * cos_w + 2.0 * sqrt_a * alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w),
                    (a + 1.0) - (a - 1.0) * cos_w - 2.0 * sqrt_a * alpha,
                )
            }
        }
    };
    if !a0.is_finite() || a0.abs() < f64::EPSILON {
        return None;
    }
    let inv = 1.0 / a0;
    let coeffs = BiquadCoeffs {
        b0: b0 * inv,
        b1: b1 * inv,
        b2: b2 * inv,
        a1: a1 * inv,
        a2: a2 * inv,
    };
    if ![coeffs.b0, coeffs.b1, coeffs.b2, coeffs.a1, coeffs.a2]
        .into_iter()
        .all(f64::is_finite)
    {
        return None;
    }
    if !stable(coeffs) {
        return None;
    }
    Some(coeffs)
}

fn stable(coeffs: BiquadCoeffs) -> bool {
    coeffs.a2.abs() < 1.0 && coeffs.a1.abs() < 1.0 + coeffs.a2
}

pub fn magnitude_db(
    bands: &[EqBand; BAND_COUNT],
    preamp_db: f32,
    sample_rate: f64,
    freq: f64,
) -> f64 {
    if !(freq.is_finite() && freq > 0.0 && sample_rate > freq * 2.0) {
        return f64::from(preamp_db);
    }
    let w = 2.0 * std::f64::consts::PI * freq / sample_rate;
    let mut db = f64::from(preamp_db);
    for band in bands {
        if let Some(coeffs) = design_coeffs(sample_rate, *band) {
            db += biquad_magnitude_db(coeffs, w);
        }
    }
    db
}

fn biquad_magnitude_db(coeffs: BiquadCoeffs, w: f64) -> f64 {
    let z1_re = w.cos();
    let z1_im = -w.sin();
    let z2_re = z1_re * z1_re - z1_im * z1_im;
    let z2_im = 2.0 * z1_re * z1_im;
    let num_re = coeffs.b0 + coeffs.b1 * z1_re + coeffs.b2 * z2_re;
    let num_im = coeffs.b1 * z1_im + coeffs.b2 * z2_im;
    let den_re = 1.0 + coeffs.a1 * z1_re + coeffs.a2 * z2_re;
    let den_im = coeffs.a1 * z1_im + coeffs.a2 * z2_im;
    let num = (num_re * num_re + num_im * num_im).sqrt();
    let den = (den_re * den_re + den_im * den_im).sqrt();
    if den < 1e-20 || !num.is_finite() {
        return 0.0;
    }
    20.0 * (num / den).log10()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedParametric {
    pub preamp: f32,
    pub bands: [EqBand; BAND_COUNT],
}

pub fn parse_parametric_eq(text: &str) -> AppResult<ParsedParametric> {
    let filter_re = filter_line_re();
    let preamp_re = preamp_line_re();
    let mut preamp = 0.0;
    let mut found = [EqBand {
        kind: FilterKind::Peak,
        freq: 1000.0,
        q: 1.0,
        gain: 0.0,
    }; BAND_COUNT];
    let mut found_count = 0usize;
    let mut saw_graphic = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if line
            .get(..9)
            .is_some_and(|head| head.eq_ignore_ascii_case("graphiceq"))
        {
            saw_graphic = true;
            continue;
        }
        if let Some(caps) = preamp_re.captures(line) {
            if let Ok(value) = caps[1].parse::<f32>() {
                preamp = clamp_preamp(value);
            }
            continue;
        }
        let Some(caps) = filter_re.captures(line) else {
            continue;
        };
        if caps[1].eq_ignore_ascii_case("off") {
            continue;
        }
        let kind = if caps[2].eq_ignore_ascii_case("pk")
            || caps[2].eq_ignore_ascii_case("peak")
            || caps[2].eq_ignore_ascii_case("peaking")
        {
            FilterKind::Peak
        } else if caps[2].eq_ignore_ascii_case("lsc")
            || caps[2].eq_ignore_ascii_case("ls")
            || caps[2].eq_ignore_ascii_case("lowshelf")
        {
            FilterKind::LowShelf
        } else if caps[2].eq_ignore_ascii_case("hsc")
            || caps[2].eq_ignore_ascii_case("hs")
            || caps[2].eq_ignore_ascii_case("highshelf")
        {
            FilterKind::HighShelf
        } else {
            continue;
        };
        let (Ok(freq), Ok(gain), Ok(q)) = (
            caps[3].parse::<f32>(),
            caps[4].parse::<f32>(),
            caps[5].parse::<f32>(),
        ) else {
            continue;
        };
        if found_count < BAND_COUNT {
            found[found_count] = EqBand {
                kind,
                freq: clamp_freq(freq),
                q: clamp_q(q),
                gain: clamp_gain(gain),
            };
        }
        found_count += 1;
    }
    if found_count == 0 {
        if saw_graphic {
            return Err(AppError::msg(
                "Paste a ParametricEQ profile (Preamp and Filter lines). Graphic EQ curves are not used.",
            ));
        }
        return Err(AppError::msg(
            "No parametric filters found. Paste Preamp and Filter lines from AutoEQ.",
        ));
    }
    if found_count > BAND_COUNT {
        return Err(AppError::msg(format!(
            "This profile has {found_count} filters. Audios! uses {BAND_COUNT} bands.",
        )));
    }
    let mut bands = tone_bands();
    bands[..found_count].copy_from_slice(&found[..found_count]);
    Ok(ParsedParametric { preamp, bands })
}

fn filter_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^filter\s*\d+\s*:\s*(ON|OFF)\s+(PK|LSC|HSC|LS|HS|PEAK|PEAKING|LOWSHELF|HIGHSHELF)\s+Fc\s+([+-]?[0-9]*\.?[0-9]+)\s*Hz\s+Gain\s+([+-]?[0-9]*\.?[0-9]+)\s*dB\s+Q\s+([+-]?[0-9]*\.?[0-9]+)",
        )
        .expect("filter regex")
    })
}

fn preamp_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^preamp\s*:\s*([+-]?[0-9]*\.?[0-9]+)\s*dB").expect("preamp regex")
    })
}

struct BuiltinDef {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    gains: [f32; BAND_COUNT],
    preamp: f32,
}

fn builtins() -> &'static [BuiltinDef] {
    &[
        BuiltinDef {
            id: "flat",
            name: "Flat",
            description: "No equalization",
            gains: [0.0; BAND_COUNT],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "bass-boost",
            name: "Bass Boost",
            description: "Extra low end",
            gains: [6.5, 5.5, 3.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "bass-cut",
            name: "Bass Cut",
            description: "Less low end",
            gains: [-5.5, -4.5, -2.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "warm",
            name: "Warm",
            description: "Softer highs",
            gains: [1.5, 2.5, 2.0, 1.0, 0.5, 0.0, -0.5, -1.5, -2.5, -2.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "bright",
            name: "Bright",
            description: "More treble",
            gains: [-0.5, -1.0, -0.5, 0.0, 0.0, 0.5, 1.5, 3.0, 4.5, 5.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "smooth",
            name: "Smooth",
            description: "Softer treble",
            gains: [0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -2.0, -4.0, -5.0, -5.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "vocal",
            name: "Vocal",
            description: "Vocals forward",
            gains: [-1.0, -0.5, 0.0, -3.0, -3.5, 0.5, 3.5, 4.0, 1.0, 0.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "spoken-word",
            name: "Spoken Word",
            description: "Podcasts and audiobooks",
            gains: [-6.0, -5.0, -3.0, -1.0, 2.0, 4.0, 3.0, 0.5, -2.5, -4.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "loudness",
            name: "Loudness",
            description: "Fuller at low volume",
            gains: [5.5, 4.0, 1.5, 0.0, -1.5, -1.5, 0.0, 2.0, 4.0, 4.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "night",
            name: "Night",
            description: "Quieter bass",
            gains: [-4.0, -3.0, -1.0, 0.5, 1.0, 1.5, 2.5, 3.0, 1.0, 0.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "small-speakers",
            name: "Small Speakers",
            description: "Laptops and TVs",
            gains: [-3.5, 0.5, 3.5, 4.0, 1.5, 0.0, -2.5, -3.5, 0.5, 1.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "headphones",
            name: "Headphones",
            description: "Over-ear headphones",
            gains: [0.5, 2.5, 2.0, 0.5, 0.0, -0.5, -2.5, -3.0, -0.5, 2.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "electronic",
            name: "Electronic",
            description: "Dance and electronic",
            gains: [5.0, 4.0, 0.5, -3.0, -3.5, 0.0, 1.0, 2.0, 4.0, 5.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "hip-hop",
            name: "Hip-Hop",
            description: "Hip-hop and R&B",
            gains: [6.0, 3.5, -0.5, -2.5, -1.0, 1.0, 3.0, 3.5, 1.0, 0.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "rock",
            name: "Rock",
            description: "Rock and guitar",
            gains: [2.0, 3.0, 0.5, -1.5, 0.0, 2.5, 4.0, 2.5, 2.0, 2.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "acoustic",
            name: "Acoustic",
            description: "Acoustic and folk",
            gains: [-2.0, 0.5, 3.0, 2.5, 0.0, 0.5, 2.0, 3.5, 1.5, 1.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "jazz",
            name: "Jazz",
            description: "Jazz",
            gains: [0.0, 2.5, 1.5, -2.0, 0.0, 1.5, 0.5, -2.0, -1.5, 1.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "classical",
            name: "Classical",
            description: "Orchestral",
            gains: [-2.5, -1.5, 0.5, 0.5, 1.0, 1.0, 0.5, 1.0, 2.5, 3.5],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "piano",
            name: "Piano",
            description: "Solo piano",
            gains: [-2.5, -1.0, 0.0, 0.5, 1.0, 1.5, 3.0, 2.5, 0.5, 0.0],
            preamp: 0.0,
        },
        BuiltinDef {
            id: "live",
            name: "Live",
            description: "Live recordings",
            gains: [0.0, 0.5, 1.5, 3.0, 2.5, 0.5, 0.0, 1.5, 3.5, 4.0],
            preamp: 0.0,
        },
    ]
}

fn default_preset_id() -> String {
    FLAT_PRESET_ID.into()
}

fn default_true() -> bool {
    true
}

pub struct EqShared {
    params: Mutex<EqParams>,
    generation: AtomicU64,
    viz: VizTap,
}

impl Default for EqShared {
    fn default() -> Self {
        Self {
            params: Mutex::new(EqParams::bypass()),
            generation: AtomicU64::new(1),
            viz: VizTap::new(),
        }
    }
}

impl EqShared {
    pub fn set(&self, params: EqParams) {
        *self
            .params
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = params;
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn params(&self) -> EqParams {
        self.params
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn set_viz_enabled(&self, enabled: bool) {
        self.viz.set_enabled(enabled);
    }

    pub fn viz_enabled(&self) -> bool {
        self.viz.enabled()
    }

    #[inline]
    pub fn push_viz(&self, sample: f32, rate: u32) {
        self.viz.push(sample, rate);
    }

    pub fn spectrum(&self) -> Option<[u8; crate::viz::BANDS]> {
        self.viz.spectrum()
    }
}

#[derive(Clone, Copy)]
struct Biquad {
    coeffs: BiquadCoeffs,
    target: BiquadCoeffs,
    z1: f64,
    z2: f64,
    ramp: u32,
    ramp_len: u32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self {
            coeffs: BiquadCoeffs::default(),
            target: BiquadCoeffs::default(),
            z1: 0.0,
            z2: 0.0,
            ramp: 0,
            ramp_len: 0,
        }
    }
}

impl Biquad {
    fn new(coeffs: BiquadCoeffs, ramp_len: u32) -> Self {
        Self {
            coeffs: BiquadCoeffs::default(),
            target: coeffs,
            z1: 0.0,
            z2: 0.0,
            ramp: ramp_len,
            ramp_len,
        }
    }

    fn retarget(&mut self, coeffs: BiquadCoeffs) {
        self.target = coeffs;
        self.ramp = self.ramp_len;
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        if self.ramp > 0 {
            let t = 1.0 / f64::from(self.ramp);
            self.coeffs.b0 += (self.target.b0 - self.coeffs.b0) * t;
            self.coeffs.b1 += (self.target.b1 - self.coeffs.b1) * t;
            self.coeffs.b2 += (self.target.b2 - self.coeffs.b2) * t;
            self.coeffs.a1 += (self.target.a1 - self.coeffs.a1) * t;
            self.coeffs.a2 += (self.target.a2 - self.coeffs.a2) * t;
            self.ramp -= 1;
            if self.ramp == 0 {
                self.coeffs = self.target;
            }
        }
        let y = self.coeffs.b0 * x + self.z1;
        self.z1 = flush(self.coeffs.b1 * x - self.coeffs.a1 * y + self.z2);
        self.z2 = flush(self.coeffs.b2 * x - self.coeffs.a2 * y);
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
        self.coeffs = self.target;
        self.ramp = 0;
    }
}

fn flush(value: f64) -> f64 {
    if value.abs() < 1e-20 {
        0.0
    } else {
        value
    }
}

const MAX_CHANNELS: usize = 8;

#[derive(Clone, Copy)]
struct ChannelEq {
    bands: [Biquad; BAND_COUNT],
    len: usize,
}

impl Default for ChannelEq {
    fn default() -> Self {
        Self {
            bands: [Biquad::default(); BAND_COUNT],
            len: 0,
        }
    }
}

pub struct EqFilter {
    sample_rate: u32,
    channels: u16,
    channel_idx: u16,
    channels_eq: [ChannelEq; MAX_CHANNELS],
    preamp_lin: f64,
    bypass: bool,
}

impl EqFilter {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let mut filter = Self {
            sample_rate,
            channels: channels.max(1),
            channel_idx: 0,
            channels_eq: [ChannelEq::default(); MAX_CHANNELS],
            preamp_lin: 1.0,
            bypass: true,
        };
        filter.configure(&EqParams::bypass());
        filter
    }

    pub fn configure(&mut self, params: &EqParams) {
        self.bypass = is_bypass(params);
        let sr = f64::from(self.sample_rate.max(1));
        let preamp = if self.bypass {
            0.0
        } else if params.auto_preamp {
            let user = clamp_preamp(params.preamp);
            let peak = composite_peak_db(&params.bands, sr);
            if peak > 0.0 {
                user.min(-peak).max(-48.0)
            } else {
                user
            }
        } else {
            clamp_preamp(params.preamp)
        };
        self.preamp_lin = 10f64.powf(f64::from(preamp) / 20.0);
        let slots = self.channel_slots();
        if self.bypass {
            for channel in &mut self.channels_eq[..slots] {
                channel.len = 0;
            }
            return;
        }
        let ramp_len = (self.sample_rate / 200).max(1);
        let mut template = [Biquad::default(); BAND_COUNT];
        let mut len = 0usize;
        for band in params.bands {
            if let Some(coeffs) = design_coeffs(sr, band) {
                template[len] = Biquad::new(coeffs, ramp_len);
                len += 1;
            }
        }
        if len == 0 && (self.preamp_lin - 1.0).abs() < 1e-6 {
            self.bypass = true;
            for channel in &mut self.channels_eq[..slots] {
                channel.len = 0;
            }
            return;
        }
        let same = self.channels_eq[..slots]
            .iter()
            .all(|channel| channel.len == len);
        if same && len > 0 {
            for channel in &mut self.channels_eq[..slots] {
                for (band, source) in channel.bands.iter_mut().zip(template.iter()).take(len) {
                    band.retarget(source.target);
                }
            }
            return;
        }
        for channel in &mut self.channels_eq[..slots] {
            channel.bands = template;
            channel.len = len;
        }
    }

    fn channel_slots(&self) -> usize {
        (self.channels.max(1) as usize).min(MAX_CHANNELS)
    }

    pub fn reset(&mut self) {
        self.channel_idx = 0;
        let slots = self.channel_slots();
        for channel in &mut self.channels_eq[..slots] {
            for band in &mut channel.bands[..channel.len] {
                band.reset();
            }
        }
    }

    #[cfg(test)]
    pub fn process_buffer(&mut self, samples: &[f32]) -> Vec<f32> {
        samples
            .iter()
            .copied()
            .map(|sample| self.process(sample))
            .collect()
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let slots = self.channel_slots();
        let ch = self.channel_idx as usize % slots;
        self.channel_idx = (self.channel_idx + 1) % slots as u16;
        if self.bypass {
            return x;
        }
        let mut y = f64::from(x);
        let channel = &mut self.channels_eq[ch];
        for band in &mut channel.bands[..channel.len] {
            y = band.process(y);
        }
        (y * self.preamp_lin) as f32
    }
}

/// Frames averaged into one visualizer sample. Stereo `next` calls are one per channel.
const VIZ_DECIMATE: u32 = 4;

/// What the player engine wants to know about one source as it plays.
/// Called on the audio callback thread; keep implementations cheap.
pub trait SourceHooks: Send {
    /// About to produce the first sample.
    fn started(&mut self);
    /// `frames` frames of this file have been emitted so far.
    fn progressed(&mut self, frames: u64);
    /// The inner decoder ran out.
    fn finished(&mut self);
    /// A seek landed; `frames` is the new file position.
    fn seeked(&mut self, frames: u64);
    /// When true the source yields nothing, so rodio moves past it.
    fn cancelled(&self) -> bool;
}

/// How often `progressed` fires, in frames. ~20 ms at 48 kHz.
const PROGRESS_EVERY_FRAMES: u64 = 1024;

pub struct EqSource<I> {
    inner: I,
    shared: Arc<EqShared>,
    generation: u64,
    filter: EqFilter,
    viz_sum: f32,
    viz_count: u32,
    viz_span: u32,
    viz_rate: u32,
    hooks: Option<Box<dyn SourceHooks>>,
    channels: u64,
    rate: u64,
    samples: u64,
    frames: u64,
    last_report: u64,
    started: bool,
    done: bool,
}

impl<I> EqSource<I>
where
    I: Source,
    I::Item: Sample,
{
    pub fn new(inner: I, shared: Arc<EqShared>) -> Self {
        let channels = inner.channels().max(1) as u32;
        let rate = inner.sample_rate().max(1);
        let mut filter = EqFilter::new(rate, channels as u16);
        let params = shared.params();
        filter.configure(&params);
        Self {
            inner,
            generation: shared.generation(),
            shared,
            filter,
            viz_sum: 0.0,
            viz_count: 0,
            viz_span: channels * VIZ_DECIMATE,
            viz_rate: (rate / VIZ_DECIMATE).max(1),
            hooks: None,
            channels: channels as u64,
            rate: rate as u64,
            samples: 0,
            frames: 0,
            last_report: 0,
            started: false,
            done: false,
        }
    }

    pub fn with_hooks(mut self, hooks: Box<dyn SourceHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Start counting from `pos`, for a decoder that was seeked before wrapping.
    pub fn starting_at(mut self, pos: Duration) -> Self {
        self.frames = (pos.as_secs_f64() * self.rate as f64) as u64;
        self.last_report = self.frames;
        self
    }

    #[inline]
    fn tap(&mut self, sample: f32) {
        if !self.shared.viz_enabled() {
            return;
        }
        self.viz_sum += sample;
        self.viz_count += 1;
        if self.viz_count < self.viz_span {
            return;
        }
        let frame = self.viz_sum / self.viz_count as f32;
        self.viz_sum = 0.0;
        self.viz_count = 0;
        self.shared.push_viz(frame, self.viz_rate);
    }

    fn sync(&mut self) {
        let gen = self.shared.generation();
        if gen != self.generation {
            self.filter.configure(&self.shared.params());
            self.generation = gen;
        }
    }
}

impl<I> Iterator for EqSource<I>
where
    I: Source,
    I::Item: Sample,
{
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.done {
            return None;
        }
        // Symphonia can panic mid-file. This runs on the audio callback
        // thread, where an unwind would take the output stream with it.
        // Treat a panic as the end of this source.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.next_inner())) {
            Ok(sample) => sample,
            Err(_) => {
                self.done = true;
                if let Some(hooks) = self.hooks.as_mut() {
                    hooks.finished();
                }
                None
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I> EqSource<I>
where
    I: Source,
    I::Item: Sample,
{
    #[inline]
    fn next_inner(&mut self) -> Option<f32> {
        if let Some(hooks) = self.hooks.as_mut() {
            if hooks.cancelled() {
                return None;
            }
            if !self.started {
                self.started = true;
                hooks.started();
                hooks.progressed(self.frames);
            }
        }
        self.sync();
        let Some(raw) = self.inner.next() else {
            if !self.done {
                self.done = true;
                if let Some(hooks) = self.hooks.as_mut() {
                    hooks.progressed(self.frames);
                    hooks.finished();
                }
            }
            return None;
        };
        let sample = self.filter.process(raw.to_f32());
        self.tap(sample);
        if self.hooks.is_some() {
            self.samples += 1;
            if self.samples >= self.channels {
                self.samples = 0;
                self.frames += 1;
                if self.frames - self.last_report >= PROGRESS_EVERY_FRAMES {
                    self.last_report = self.frames;
                    if let Some(hooks) = self.hooks.as_mut() {
                        hooks.progressed(self.frames);
                    }
                }
            }
        }
        Some(sample)
    }
}

impl<I> Source for EqSource<I>
where
    I: Source,
    I::Item: Sample,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        self.filter.reset();
        self.frames = (pos.as_secs_f64() * self.rate as f64) as u64;
        self.last_report = self.frames;
        self.samples = 0;
        self.done = false;
        if let Some(hooks) = self.hooks.as_mut() {
            hooks.seeked(self.frames);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_preset() -> EqUserPreset {
        let mut bands = tone_bands();
        bands[0].gain = 2.0;
        bands[9].gain = -1.0;
        EqUserPreset {
            id: "custom-1".into(),
            name: "Desk".into(),
            bands,
            preamp: -1.5,
            auto_preamp: true,
        }
    }

    fn peak(freq: f32, q: f32, gain: f32) -> EqBand {
        EqBand {
            kind: FilterKind::Peak,
            freq,
            q,
            gain,
        }
    }

    #[test]
    fn flat_and_disabled_are_bypass() {
        let mut disabled = EqParams::bypass();
        disabled.enabled = false;
        disabled.bands[0].gain = 6.0;
        assert!(is_bypass(&disabled));

        let flat = EqParams {
            enabled: true,
            bands: tone_bands(),
            preamp: 0.0,
            auto_preamp: true,
        };
        assert!(is_bypass(&flat));
        assert!(is_tone_template(&flat.bands));
    }

    #[test]
    fn bypass_is_identity() {
        let samples = [0.15f32, -0.4, 0.8, -0.05, 0.22, -0.31];
        let mut filter = EqFilter::new(44_100, 2);
        filter.configure(&EqParams::bypass());
        assert_eq!(filter.process_buffer(&samples), samples.to_vec());

        let mut flat = EqFilter::new(44_100, 2);
        flat.configure(&EqParams {
            enabled: true,
            bands: tone_bands(),
            preamp: 0.0,
            auto_preamp: true,
        });
        assert_eq!(flat.process_buffer(&samples), samples.to_vec());
    }

    #[test]
    fn enabled_curve_changes_samples() {
        let samples = [0.2f32, -0.2, 0.4, -0.4];
        let mut bands = tone_bands();
        bands[0].gain = 6.0;
        let mut filter = EqFilter::new(44_100, 1);
        filter.configure(&EqParams {
            enabled: true,
            bands,
            preamp: 0.0,
            auto_preamp: false,
        });
        filter.reset();
        let out = filter.process_buffer(&samples);
        assert_ne!(out, samples);
        assert!(out.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn low_frequency_high_rate_stays_finite() {
        let mut bands = tone_bands();
        bands[0] = EqBand {
            kind: FilterKind::LowShelf,
            freq: 32.0,
            q: 0.71,
            gain: 6.0,
        };
        let mut filter = EqFilter::new(192_000, 1);
        filter.configure(&EqParams {
            enabled: true,
            bands,
            preamp: 0.0,
            auto_preamp: false,
        });
        filter.reset();
        for n in 0..8_192 {
            let x = (2.0 * std::f64::consts::PI * 32.0 * n as f64 / 192_000.0).sin() as f32 * 0.5;
            let y = filter.process(x);
            assert!(y.is_finite());
            assert!(y.abs() < 2.0, "{y}");
        }
    }

    #[test]
    fn peaking_magnitude_matches_gain_at_center() {
        let mut bands = tone_bands();
        bands[4] = peak(1000.0, BAND_Q, 6.0);
        let db = magnitude_db(&bands, 0.0, 44_100.0, 1000.0);
        assert!((db - 6.0).abs() < 0.05, "{db}");
        let away = magnitude_db(&bands, 0.0, 44_100.0, 100.0);
        assert!(away.abs() < 0.4, "{away}");
    }

    #[test]
    fn narrow_peak_between_grid_points_is_measured() {
        let mut bands = tone_bands();
        let freq = 20.0 * 1000f32.powf(10.5 / 95.0);
        bands[4] = peak(freq, 10.0, 6.0);
        let db = composite_peak_db(&bands, 48_000.0);
        assert!(db > 5.5, "{db}");
    }

    #[test]
    fn shelf_q_above_one_is_stored_as_one() {
        let mut bands = tone_bands();
        bands[0].q = 4.0;
        bands[0].gain = 3.0;
        let stored = normalize_bands(&bands);
        assert_eq!(stored[0].kind, FilterKind::LowShelf);
        assert_eq!(stored[0].q, 1.0);
        assert!(design_coeffs(192_000.0, stored[0]).is_some());
    }

    #[test]
    fn low_shelf_approaches_gain_below_corner() {
        let mut bands = tone_bands();
        bands[0] = EqBand {
            kind: FilterKind::LowShelf,
            freq: 90.0,
            q: 0.71,
            gain: 5.0,
        };
        let db = magnitude_db(&bands, 0.0, 48_000.0, 20.0);
        assert!((db - 5.0).abs() < 0.4, "{db}");
    }

    #[test]
    fn auto_preamp_uses_composite_peak() {
        let mut bands = tone_bands();
        bands[4] = peak(1000.0, 1.0, 6.0);
        bands[2] = peak(1000.0, 1.0, 6.0);
        let params = EqParams {
            enabled: true,
            bands,
            preamp: 0.0,
            auto_preamp: true,
        };
        let pre = effective_preamp(&params);
        assert!(pre < -6.0, "{pre}");
        assert!(pre >= -48.0);
    }

    #[test]
    fn presets_keep_ten_bands() {
        let catalog = builtin_catalog();
        assert_eq!(catalog.len(), 20);
        let mut ids = std::collections::HashSet::new();
        for preset in &catalog {
            assert_eq!(preset.bands.len(), BAND_COUNT);
            assert!(ids.insert(preset.id.clone()));
            for band in preset.bands {
                assert!(band.gain >= GAIN_MIN && band.gain <= GAIN_MAX);
            }
        }
        let flat = catalog.iter().find(|item| item.id == "flat").unwrap();
        assert!(is_tone_template(&flat.bands));
        assert_eq!(flat.bands[0].kind, FilterKind::LowShelf);
        assert_eq!(flat.bands[9].kind, FilterKind::HighShelf);
        let bass = catalog.iter().find(|item| item.id == "bass-boost").unwrap();
        let small = catalog
            .iter()
            .find(|item| item.id == "small-speakers")
            .unwrap();
        assert!(bass.bands[0].gain > small.bands[0].gain);
        assert!(!is_tone_template(&bass.bands));
        let vocal = catalog.iter().find(|item| item.id == "vocal").unwrap();
        assert!(vocal.bands[3].gain < 0.0 && vocal.bands[7].gain > 0.0);
    }

    #[test]
    fn legacy_gains_become_peaking_bands() {
        let raw = r#"{"id":"custom-2","name":"Pad","gains":[1,2,3],"preamp":0,"autoPreamp":false}"#;
        let preset: EqUserPreset = serde_json::from_str(raw).unwrap();
        assert_eq!(preset.bands[0].kind, FilterKind::Peak);
        assert_eq!(preset.bands[0].freq, 32.0);
        assert_eq!(preset.bands[0].gain, 1.0);
        assert_eq!(preset.bands[2].gain, 3.0);
        assert_eq!(preset.bands[9].gain, 0.0);
        let again = serde_json::to_string(&preset).unwrap();
        assert!(again.contains("lowShelf") || again.contains("\"kind\""));
        let back: EqUserPreset = serde_json::from_str(&again).unwrap();
        assert_eq!(preset.bands, back.bands);
    }

    #[test]
    fn tone_template_is_distinct_from_graphic_bands() {
        let graphic = graphic_bands(&[3.0, 4.0, 1.0, 0.0, 2.0, 0.0, 0.0, -1.0, 5.0, 2.0]);
        assert!(!is_tone_template(&graphic));
        let tone = tone_bands();
        assert!(is_tone_template(&tone));
        assert_eq!(tone[0].kind, FilterKind::LowShelf);
        assert_eq!(tone[9].kind, FilterKind::HighShelf);
    }

    #[test]
    fn parser_strips_comments_and_rejects_graphic_only() {
        let text = r#"
            # HD 600
            Preamp: -6.8 dB
            Filter 1: ON LSC Fc 105 Hz Gain 3.5 dB Q 0.70
            Filter 2: OFF PK Fc 200 Hz Gain 9 dB Q 1
            nonsense !!!
            Filter 3: ON PK Fc 1000 Hz Gain -2.5 dB Q 1.41
            Filter 4: ON HSC Fc 10000 Hz Gain 1.5 dB Q 0.71
        "#;
        let parsed = parse_parametric_eq(text).unwrap();
        assert!((parsed.preamp + 6.8).abs() < 0.01);
        assert_eq!(parsed.bands[0].kind, FilterKind::LowShelf);
        assert_eq!(parsed.bands[0].freq, 105.0);
        assert!((parsed.bands[0].gain - 3.5).abs() < 0.01);
        assert_eq!(parsed.bands[1].kind, FilterKind::Peak);
        assert_eq!(parsed.bands[1].gain, -2.5);
        assert_eq!(parsed.bands[2].kind, FilterKind::HighShelf);
        assert_eq!(parsed.bands[3].gain, 0.0);
        assert!(parse_parametric_eq("GraphicEQ: 20 -6; 1000 2").is_err());
        assert!(parse_parametric_eq("Filter 1: MAYBE PK Fc no Hz Gain x dB Q y").is_err());
        // Pasted text with multibyte characters must not panic on a byte slice.
        assert!(parse_parametric_eq("ÉÉÉÉÉ\nFilter 1: ON PK Fc 1000 Hz Gain 1 dB Q 1").is_ok());
        assert!(parse_parametric_eq("日本語のテキスト").is_err());
    }

    #[test]
    fn parser_refuses_more_than_ten_filters() {
        let mut text = String::from("Preamp: -3 dB\n");
        for index in 1..=11 {
            text.push_str(&format!("Filter {index}: ON PK Fc 1000 Hz Gain 1 dB Q 1\n"));
        }
        assert!(parse_parametric_eq(&text).is_err());
    }

    #[test]
    fn upsert_caps_and_protects_builtins() {
        let mut eq = EqPersist::default();
        assert!(upsert_user_preset(&mut eq, sample_preset()).is_ok());
        assert_eq!(eq.custom_presets.len(), 1);
        assert_eq!(eq.preset_id, "custom-1");
        let mut builtin = sample_preset();
        builtin.id = "vocal".into();
        assert!(upsert_user_preset(&mut eq, builtin).is_err());
        for index in 0..MAX_USER_PRESETS {
            let mut extra = sample_preset();
            extra.id = format!("custom-x-{index}");
            let _ = upsert_user_preset(&mut eq, extra);
        }
        assert_eq!(eq.custom_presets.len(), MAX_USER_PRESETS);
        let mut overflow = sample_preset();
        overflow.id = "custom-overflow".into();
        assert!(upsert_user_preset(&mut eq, overflow).is_err());
        assert!(delete_user_preset(&mut eq, "flat").is_err());
        assert!(delete_user_preset(&mut eq, "custom-1").is_ok());
        let selected = eq.preset_id.clone();
        assert!(delete_user_preset(&mut eq, &selected).is_ok());
        assert_eq!(eq.preset_id, FLAT_PRESET_ID);
    }

    #[test]
    fn seek_resets_filter_memory() {
        let mut bands = tone_bands();
        bands[0].gain = 4.0;
        let params = EqParams {
            enabled: true,
            bands,
            preamp: 0.0,
            auto_preamp: false,
        };
        let mut filter = EqFilter::new(44_100, 1);
        filter.configure(&params);
        let _ = filter.process(0.9);
        let _ = filter.process(-0.4);
        filter.reset();
        let mut fresh = EqFilter::new(44_100, 1);
        fresh.configure(&params);
        fresh.reset();
        let a = filter.process(0.3);
        let b = fresh.process(0.3);
        assert!((a - b).abs() < 1e-5);
    }
}
