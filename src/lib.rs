//! Warmup-exact batch port of TA-Lib technical indicators for Rust.
//!
//! See crate README for module layout and parity policy.
//!
//! # Example
//!
//! ```
//! use ta_indicators::{bollinger_bands, macd, rsi};
//!
//! let closes = [100.0, 101.0, 102.0, 101.5, 103.0, 104.0, 103.5, 105.0];
//! let rsi_3 = rsi(&closes, 3);
//! let macd_out = macd(&closes, 3, 6, 3);
//! let bands = bollinger_bands(&closes, 3, 2.0);
//!
//! assert_eq!(rsi_3.len(), closes.len());
//! assert_eq!(macd_out.macd.len(), closes.len());
//! assert_eq!(bands.middle.len(), closes.len());
//! ```
//!
//! Outputs use `Option<f64>` for numeric indicators. Warmup bars are `None`;
//! emitted bars are matched against committed TA-Lib reference fixtures.
//!
//! TradingView CSV-validated families live under [`tradingview`]. They are
//! intentionally namespaced separately because their reference oracle is
//! TradingView export CSVs rather than TA-Lib.

mod candles;
pub mod streaming;
pub mod tradingview;
pub use candles::*;
pub use streaming::{
    CachedHurst, ChoppinessIndex, IncrementalRsi, IncrementalSma, RegimeDetector, RegimeState,
    StreamingAdx, StreamingOrderflow,
};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Macd {
    pub macd: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub hist: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct AdxFamily {
    pub adx: Vec<Option<f64>>,
    pub adxr: Vec<Option<f64>>,
    pub plus_di: Vec<Option<f64>>,
    pub minus_di: Vec<Option<f64>>,
    pub dx: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct Aroon {
    pub up: Vec<Option<f64>>,
    pub down: Vec<Option<f64>>,
    pub oscillator: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct PriceTransforms {
    pub avgprice: Vec<Option<f64>>,
    pub medprice: Vec<Option<f64>>,
    pub typprice: Vec<Option<f64>>,
    pub wclprice: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct BollingerBands {
    pub upper: Vec<Option<f64>>,
    pub middle: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct DirectionalMovement {
    pub plus_dm: Vec<Option<f64>>,
    pub minus_dm: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct StochRsi {
    pub k: Vec<Option<f64>>,
    pub d: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct LinearRegression {
    pub line: Vec<Option<f64>>,
    pub slope: Vec<Option<f64>>,
    pub angle: Vec<Option<f64>>,
    pub intercept: Vec<Option<f64>>,
    pub tsf: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct PriceContext {
    pub close_vs_ath_pct: Vec<Option<f64>>,
    pub close_vs_atl_pct: Vec<Option<f64>>,
    pub days_since_ath: Vec<Option<f64>>,
    pub days_since_atl: Vec<Option<f64>>,
}

#[inline]
fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn option_values(values: &[Option<f64>]) -> Vec<f64> {
    values
        .iter()
        .map(|value| value.unwrap_or(f64::NAN))
        .collect()
}

pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut current = None;
    let mut warm_sum = 0.0;
    let mut warm_count = 0usize;
    for (idx, value) in values.iter().copied().enumerate() {
        let Some(value) = finite(value) else {
            continue;
        };
        if let Some(prev) = current {
            let next = alpha * value + (1.0 - alpha) * prev;
            current = Some(next);
            out[idx] = Some(next);
        } else {
            warm_sum += value;
            warm_count += 1;
            if warm_count == period {
                let next = warm_sum / period as f64;
                current = Some(next);
                out[idx] = Some(next);
            }
        }
    }
    out
}

/// SMA-seeded EMA whose seed is the average of the `period` raw values ending
/// at `seed_end`, then the standard EMA recurrence forward from there. This
/// matches TA-Lib's `TA_INT_EMA` when its output start is forced to `seed_end`.
fn ema_seeded_at(values: &[f64], period: usize, seed_end: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || seed_end >= values.len() || seed_end + 1 < period {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed = values[seed_end + 1 - period..=seed_end].iter().sum::<f64>() / period as f64;
    out[seed_end] = Some(seed);
    let mut prev = seed;
    for (idx, value) in values.iter().enumerate().skip(seed_end + 1) {
        prev = alpha * value + (1.0 - alpha) * prev;
        out[idx] = Some(prev);
    }
    out
}

/// EMA over an `Option` series that is contiguously valid from `valid_start`,
/// SMA-seeded over its first `period` values. Used for the MACD signal line.
fn ema_seeded_options(
    series: &[Option<f64>],
    period: usize,
    valid_start: usize,
) -> Vec<Option<f64>> {
    let mut out = vec![None; series.len()];
    let seed_end = valid_start + period.saturating_sub(1);
    if period == 0 || seed_end >= series.len() {
        return out;
    }
    let mut sum = 0.0;
    for value in series.iter().take(seed_end + 1).skip(valid_start) {
        match value {
            Some(value) => sum += value,
            None => return out,
        }
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed = sum / period as f64;
    out[seed_end] = Some(seed);
    let mut prev = seed;
    for (idx, value) in series.iter().enumerate().skip(seed_end + 1) {
        if let Some(value) = value {
            prev = alpha * value + (1.0 - alpha) * prev;
            out[idx] = Some(prev);
        }
    }
    out
}

pub fn macd(values: &[f64], fast: usize, slow: usize, signal: usize) -> Macd {
    let len = values.len();
    let mut macd_line = vec![None; len];
    if fast == 0 || slow == 0 || signal == 0 {
        return Macd {
            macd: macd_line.clone(),
            signal: vec![None; len],
            hist: vec![None; len],
        };
    }
    // TA-Lib aligns both EMAs' SMA seed to the slow EMA's start (`slow - 1`),
    // so the fast EMA is seeded over the same window's tail rather than its own
    // natural warmup. The MACD line is then `fastEMA - slowEMA` from there.
    let seed_end = fast.max(slow) - 1;
    let fast_ema = ema_seeded_at(values, fast, seed_end);
    let slow_ema = ema_seeded_at(values, slow, seed_end);
    for idx in seed_end..len {
        if let (Some(f), Some(s)) = (fast_ema[idx], slow_ema[idx]) {
            macd_line[idx] = Some(f - s);
        }
    }
    let signal_line = ema_seeded_options(&macd_line, signal, seed_end);
    let hist = macd_line
        .iter()
        .zip(signal_line.iter())
        .map(|(macd, signal)| Some((*macd)? - (*signal)?))
        .collect();
    Macd {
        macd: macd_line,
        signal: signal_line,
        hist,
    }
}

pub fn macdfix(values: &[f64], signal: usize) -> Macd {
    macd(values, 12, 26, signal)
}

/// Simple moving average over a raw series. First value at index `period - 1`.
fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    rolling_sum(values, period)
        .into_iter()
        .map(|sum| sum.map(|sum| sum / period as f64))
        .collect()
}

/// Simple moving average over an `Option` series; a window is only emitted when
/// all `period` members are present (matches TA-Lib MA-of-MA warmup behavior).
fn sma_options(series: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; series.len()];
    if period == 0 {
        return out;
    }
    for idx in period - 1..series.len() {
        let window = &series[idx + 1 - period..=idx];
        let Some(sum) = window
            .iter()
            .try_fold(0.0, |sum, value| value.map(|value| sum + value))
        else {
            continue;
        };
        out[idx] = Some(sum / period as f64);
    }
    out
}

/// MACDEXT with all three moving averages set to SMA (TA-Lib `matype = 0`).
/// fast SMA − slow SMA forms the MACD line; an SMA over the MACD line forms the
/// signal. Matches `talib.MACDEXT(.., fastmatype=0, slowmatype=0, signalmatype=0)`.
pub fn macdext_sma(values: &[f64], fast: usize, slow: usize, signal: usize) -> Macd {
    let len = values.len();
    if fast == 0 || slow == 0 || signal == 0 {
        return Macd {
            macd: vec![None; len],
            signal: vec![None; len],
            hist: vec![None; len],
        };
    }
    let fast_ma = sma(values, fast);
    let slow_ma = sma(values, slow);
    let macd_line: Vec<Option<f64>> = fast_ma
        .iter()
        .zip(slow_ma.iter())
        .map(|(fast, slow)| Some((*fast)? - (*slow)?))
        .collect();
    let signal_line = sma_options(&macd_line, signal);
    let hist = macd_line
        .iter()
        .zip(signal_line.iter())
        .map(|(macd, signal)| Some((*macd)? - (*signal)?))
        .collect();
    Macd {
        macd: macd_line,
        signal: signal_line,
        hist,
    }
}

/// Variable-period simple moving average (TA-Lib `MAVP`, `matype = 0`). Each
/// bar uses `periods[idx]` truncated to an integer and clamped to
/// `[min_period, max_period]`; the global warmup is `max_period - 1` so every
/// emitted bar has a full window.
pub fn mavp(
    values: &[f64],
    periods: &[f64],
    min_period: usize,
    max_period: usize,
) -> Vec<Option<f64>> {
    let len = values.len().min(periods.len());
    let mut out = vec![None; values.len()];
    if max_period == 0 || max_period < min_period {
        return out;
    }
    if values[..len].iter().all(|value| value.is_finite()) {
        let mut sums = vec![0.0; len + 1];
        for (idx, value) in values.iter().take(len).enumerate() {
            sums[idx + 1] = sums[idx] + value;
        }
        for idx in (max_period - 1)..len {
            let raw = periods[idx];
            if !raw.is_finite() || raw < 0.0 {
                continue;
            }
            let period = (raw as usize).clamp(min_period, max_period);
            if period == 0 {
                continue;
            }
            let start = idx + 1 - period;
            out[idx] = Some((sums[idx + 1] - sums[start]) / period as f64);
        }
        return out;
    }
    for idx in (max_period - 1)..len {
        let raw = periods[idx];
        if !raw.is_finite() || raw < 0.0 {
            continue;
        }
        let period = (raw as usize).clamp(min_period, max_period);
        if period == 0 {
            continue;
        }
        let window = &values[idx + 1 - period..=idx];
        if window.iter().all(|value| value.is_finite()) {
            let sum: f64 = window.iter().sum();
            out[idx] = Some(sum / period as f64);
        }
    }
    out
}

/// Rolling BETA of `real0` against `real1` (TA-Lib `BETA`). Uses `period`
/// trailing simple returns; `beta = (n·Σxy − Σx·Σy) / (n·Σxx − (Σx)²)` where
/// `x` are `real0` returns and `y` are `real1` returns. First value at index
/// `period`.
pub fn beta(real0: &[f64], real1: &[f64], period: usize) -> Vec<Option<f64>> {
    let len = real0.len().min(real1.len());
    let mut out = vec![None; real0.len()];
    if period == 0 || len <= period {
        return out;
    }
    let n = period as f64;
    if real0[..len].iter().all(|value| value.is_finite())
        && real1[..len].iter().all(|value| value.is_finite())
        && real0[..len - 1].iter().all(|value| *value != 0.0)
        && real1[..len - 1].iter().all(|value| *value != 0.0)
    {
        // Sliding return sums adapted from talib-rs 0.1.2 (BSD-3-Clause);
        // see THIRD_PARTY_NOTICES.md.
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        let ret = |values: &[f64], idx: usize| (values[idx] - values[idx - 1]) / values[idx - 1];
        for idx in 1..=period {
            let x = ret(real0, idx);
            let y = ret(real1, idx);
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        let emit = |slot: &mut Option<f64>, sx: f64, sy: f64, sxx: f64, sxy: f64| {
            let denom = n * sxx - sx * sx;
            if denom != 0.0 {
                *slot = finite((n * sxy - sx * sy) / denom);
            }
        };
        emit(&mut out[period], sx, sy, sxx, sxy);
        for (idx, slot) in out.iter_mut().enumerate().take(len).skip(period + 1) {
            let old_x = ret(real0, idx - period);
            let old_y = ret(real1, idx - period);
            let new_x = ret(real0, idx);
            let new_y = ret(real1, idx);
            sx += new_x - old_x;
            sy += new_y - old_y;
            sxx += new_x * new_x - old_x * old_x;
            sxy += new_x * new_y - old_x * old_y;
            emit(slot, sx, sy, sxx, sxy);
        }
        return out;
    }
    for (t, slot) in out.iter_mut().enumerate().take(len).skip(period) {
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
        let mut ok = true;
        for j in (t + 1 - period)..=t {
            let (px, py) = (real0[j - 1], real1[j - 1]);
            if !px.is_finite()
                || !py.is_finite()
                || !real0[j].is_finite()
                || !real1[j].is_finite()
                || px == 0.0
                || py == 0.0
            {
                ok = false;
                break;
            }
            let x = (real0[j] - px) / px;
            let y = (real1[j] - py) / py;
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
        }
        if !ok {
            continue;
        }
        let denom = n * sxx - sx * sx;
        if denom == 0.0 {
            continue;
        }
        *slot = finite((n * sxy - sx * sy) / denom);
    }
    out
}

/// Rolling Pearson correlation of `real0` and `real1` (TA-Lib `CORREL`) over a
/// trailing window of raw values. First value at index `period - 1`; when the
/// variance product is non-positive TA-Lib emits `0.0`.
pub fn correl(real0: &[f64], real1: &[f64], period: usize) -> Vec<Option<f64>> {
    let len = real0.len().min(real1.len());
    let mut out = vec![None; real0.len()];
    if period == 0 {
        return out;
    }
    if period > len {
        return out;
    }
    let n = period as f64;
    if real0[..len].iter().all(|value| value.is_finite())
        && real1[..len].iter().all(|value| value.is_finite())
    {
        // Sliding-window update adapted from talib-rs 0.1.2 (BSD-3-Clause);
        // see THIRD_PARTY_NOTICES.md.
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sxx = 0.0;
        let mut syy = 0.0;
        let mut sxy = 0.0;
        for idx in 0..period {
            let x = real0[idx];
            let y = real1[idx];
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        let emit = |slot: &mut Option<f64>, sx: f64, sy: f64, sxx: f64, syy: f64, sxy: f64| {
            let numerator = n * sxy - sx * sy;
            let denominator = ((n * sxx - sx * sx) * (n * syy - sy * sy)).sqrt();
            *slot = if denominator > 0.0 {
                finite(numerator / denominator)
            } else {
                Some(0.0)
            };
        };
        emit(&mut out[period - 1], sx, sy, sxx, syy, sxy);
        for idx in period..len {
            let old_x = real0[idx - period];
            let old_y = real1[idx - period];
            let new_x = real0[idx];
            let new_y = real1[idx];
            sx += new_x - old_x;
            sy += new_y - old_y;
            sxx += new_x * new_x - old_x * old_x;
            syy += new_y * new_y - old_y * old_y;
            sxy += new_x * new_y - old_x * old_y;
            emit(&mut out[idx], sx, sy, sxx, syy, sxy);
        }
        return out;
    }
    for (t, slot) in out.iter_mut().enumerate().take(len).skip(period - 1) {
        let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut ok = true;
        for j in (t + 1 - period)..=t {
            let (x, y) = (real0[j], real1[j]);
            if !x.is_finite() || !y.is_finite() {
                ok = false;
                break;
            }
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        if !ok {
            continue;
        }
        let var_product = (sxx - sx * sx / n) * (syy - sy * sy / n);
        *slot = if var_product > 0.0 {
            finite((sxy - sx * sy / n) / var_product.sqrt())
        } else {
            Some(0.0)
        };
    }
    out
}

pub fn apo(values: &[f64], fast: usize, slow: usize) -> Vec<Option<f64>> {
    let fast_ema = ema(values, fast);
    let slow_ema = ema(values, slow);
    fast_ema
        .iter()
        .zip(slow_ema.iter())
        .map(|(fast, slow)| Some((*fast)? - (*slow)?))
        .collect()
}

pub fn ppo(values: &[f64], fast: usize, slow: usize) -> Vec<Option<f64>> {
    let fast_ema = ema(values, fast);
    let slow_ema = ema(values, slow);
    fast_ema
        .iter()
        .zip(slow_ema.iter())
        .map(|(fast, slow)| {
            let slow = slow.filter(|slow| slow.abs() > f64::EPSILON)?;
            Some(((*fast)? / slow - 1.0) * 100.0)
        })
        .collect()
}

pub fn rsi(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() <= period {
        return out;
    }
    let mut gains = 0.0;
    let mut losses = 0.0;
    for idx in 1..values.len() {
        let change = values[idx] - values[idx - 1];
        if !change.is_finite() {
            continue;
        }
        let gain = change.max(0.0);
        let loss = (-change).max(0.0);
        if idx <= period {
            gains += gain;
            losses += loss;
            if idx == period {
                gains /= period as f64;
                losses /= period as f64;
                out[idx] = Some(rsi_value(gains, losses));
            }
        } else {
            gains = (gains * (period as f64 - 1.0) + gain) / period as f64;
            losses = (losses * (period as f64 - 1.0) + loss) / period as f64;
            out[idx] = Some(rsi_value(gains, losses));
        }
    }
    out
}

fn rsi_value(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss <= f64::EPSILON {
        100.0
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    }
}

pub fn stochrsi(values: &[f64], rsi_period: usize, k_period: usize, d_period: usize) -> StochRsi {
    let rsi = rsi(values, rsi_period);
    let mut k = vec![None; values.len()];
    if k_period > 0 {
        let mut valid_count = 0usize;
        let mut lows: VecDeque<(usize, f64)> = VecDeque::new();
        let mut highs: VecDeque<(usize, f64)> = VecDeque::new();
        for idx in 0..rsi.len() {
            if let Some(value) = rsi[idx] {
                valid_count += 1;
                while lows.back().is_some_and(|(_, prior)| *prior >= value) {
                    lows.pop_back();
                }
                lows.push_back((idx, value));
                while highs.back().is_some_and(|(_, prior)| *prior <= value) {
                    highs.pop_back();
                }
                highs.push_back((idx, value));
            }
            if idx >= k_period {
                let expired = idx - k_period;
                if rsi[expired].is_some() {
                    valid_count -= 1;
                }
                while lows.front().is_some_and(|(low_idx, _)| *low_idx <= expired) {
                    lows.pop_front();
                }
                while highs
                    .front()
                    .is_some_and(|(high_idx, _)| *high_idx <= expired)
                {
                    highs.pop_front();
                }
            }
            if idx + 1 >= k_period
                && valid_count == k_period
                && let (Some((_, low)), Some((_, high)), Some(current)) =
                    (lows.front(), highs.front(), rsi[idx])
            {
                let range = high - low;
                if range.abs() > f64::EPSILON {
                    k[idx] = Some((current - low) / range * 100.0);
                }
            }
        }
    }
    let d = option_mean(&k, d_period);
    StochRsi { k, d }
}

fn option_mean(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut valid_count = 0usize;
    for idx in 0..values.len() {
        if let Some(value) = values[idx] {
            sum += value;
            valid_count += 1;
        }
        if idx >= period
            && let Some(value) = values[idx - period]
        {
            sum -= value;
            valid_count -= 1;
        }
        if idx + 1 >= period && valid_count == period {
            out[idx] = Some(sum / period as f64);
        }
    }
    out
}

pub fn kama(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() <= period {
        return out;
    }
    let fast_sc = 2.0 / (2.0 + 1.0);
    let slow_sc = 2.0 / (30.0 + 1.0);
    // TA-Lib seeds the running KAMA with the prior close (`values[period - 1]`)
    // and applies a full smoothing step on the first emitted bar.
    if values.iter().all(|value| value.is_finite()) {
        // Rolling volatility update adapted from talib-rs 0.1.2
        // (BSD-3-Clause); see THIRD_PARTY_NOTICES.md.
        let mut current = values[period - 1];
        let mut volatility = 0.0;
        for idx in 1..=period {
            volatility += (values[idx] - values[idx - 1]).abs();
        }
        for idx in period..values.len() {
            if idx > period {
                volatility += (values[idx] - values[idx - 1]).abs()
                    - (values[idx - period] - values[idx - period - 1]).abs();
            }
            let change = (values[idx] - values[idx - period]).abs();
            let er = if volatility > f64::EPSILON {
                change / volatility
            } else {
                0.0
            };
            let smoothing_base = er * (fast_sc - slow_sc) + slow_sc;
            let smoothing = smoothing_base * smoothing_base;
            current += smoothing * (values[idx] - current);
            out[idx] = Some(current);
        }
        return out;
    }
    let mut current = finite(values[period - 1]);
    for idx in period..values.len() {
        let Some(price) = finite(values[idx]) else {
            continue;
        };
        let Some(prior) = finite(values[idx - period]) else {
            continue;
        };
        let change = (price - prior).abs();
        let mut volatility = 0.0;
        let mut valid = true;
        for offset in idx + 1 - period..=idx {
            let diff = values[offset] - values[offset - 1];
            if !diff.is_finite() {
                valid = false;
                break;
            }
            volatility += diff.abs();
        }
        if !valid {
            continue;
        }
        let er = if volatility > f64::EPSILON {
            change / volatility
        } else {
            0.0
        };
        let smoothing_base = er * (fast_sc - slow_sc) + slow_sc;
        let smoothing = smoothing_base * smoothing_base;
        let next = if let Some(prev) = current {
            prev + smoothing * (price - prev)
        } else {
            price
        };
        current = Some(next);
        out[idx] = Some(next);
    }
    out
}

pub fn bollinger_bands(values: &[f64], period: usize, deviations: f64) -> BollingerBands {
    let mut upper = vec![None; values.len()];
    let mut middle = vec![None; values.len()];
    let mut lower = vec![None; values.len()];
    if period == 0 {
        return BollingerBands {
            upper,
            middle,
            lower,
        };
    }
    if period <= values.len() && values.iter().all(|value| value.is_finite()) {
        // Sliding-window update adapted from talib-rs 0.1.2 (BSD-3-Clause);
        // see THIRD_PARTY_NOTICES.md.
        let inv_period = 1.0 / period as f64;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for value in values.iter().take(period) {
            sum += *value;
            sum_sq += value * value;
        }
        let emit = |idx: usize,
                    sum: f64,
                    sum_sq: f64,
                    middle: &mut [Option<f64>],
                    upper: &mut [Option<f64>],
                    lower: &mut [Option<f64>]| {
            let mean = sum * inv_period;
            let variance = (sum_sq * inv_period - mean * mean).max(0.0);
            let std = variance.sqrt();
            middle[idx] = Some(mean);
            upper[idx] = Some(mean + deviations * std);
            lower[idx] = Some(mean - deviations * std);
        };
        emit(period - 1, sum, sum_sq, &mut middle, &mut upper, &mut lower);
        for idx in period..values.len() {
            let old = values[idx - period];
            let new = values[idx];
            sum += new - old;
            sum_sq += new * new - old * old;
            emit(idx, sum, sum_sq, &mut middle, &mut upper, &mut lower);
        }
        return BollingerBands {
            upper,
            middle,
            lower,
        };
    }
    for idx in period - 1..values.len() {
        let start = idx + 1 - period;
        let window = &values[start..=idx];
        if window.iter().any(|value| !value.is_finite()) {
            continue;
        }
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window
            .iter()
            .map(|value| {
                let delta = value - mean;
                delta * delta
            })
            .sum::<f64>()
            / period as f64;
        let std = variance.sqrt();
        middle[idx] = Some(mean);
        upper[idx] = Some(mean + deviations * std);
        lower[idx] = Some(mean - deviations * std);
    }
    BollingerBands {
        upper,
        middle,
        lower,
    }
}

pub fn bop(opens: &[f64], highs: &[f64], lows: &[f64], closes: &[f64]) -> Vec<Option<f64>> {
    let len = opens
        .len()
        .min(highs.len())
        .min(lows.len())
        .min(closes.len());
    let mut out = Vec::with_capacity(len);
    for idx in 0..len {
        let range = highs[idx] - lows[idx];
        out.push(
            (range.abs() > f64::EPSILON)
                .then_some((closes[idx] - opens[idx]) / range)
                .filter(|value| value.is_finite()),
        );
    }
    out
}

pub fn cmo(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() <= period {
        return out;
    }
    // TA-Lib CMO uses Wilder smoothing of the up/down sums (identical to RSI's
    // running average), not a simple trailing window.
    let mut gains = 0.0;
    let mut losses = 0.0;
    for idx in 1..values.len() {
        let change = values[idx] - values[idx - 1];
        if !change.is_finite() {
            continue;
        }
        let gain = change.max(0.0);
        let loss = (-change).max(0.0);
        if idx <= period {
            gains += gain;
            losses += loss;
            if idx == period {
                gains /= period as f64;
                losses /= period as f64;
            }
        } else {
            gains = (gains * (period as f64 - 1.0) + gain) / period as f64;
            losses = (losses * (period as f64 - 1.0) + loss) / period as f64;
        }
        if idx >= period {
            let denom = gains + losses;
            if denom > f64::EPSILON {
                out[idx] = Some(100.0 * (gains - losses) / denom);
            }
        }
    }
    out
}

pub fn ultosc(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    short: usize,
    medium: usize,
    long: usize,
) -> Vec<Option<f64>> {
    let len = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; len];
    if short == 0 || medium == 0 || long == 0 || len <= long {
        return out;
    }
    if short <= long
        && medium <= long
        && highs[..len].iter().all(|value| value.is_finite())
        && lows[..len].iter().all(|value| value.is_finite())
        && closes[..len].iter().all(|value| value.is_finite())
    {
        // Rolling BP/TR sums adapted from talib-rs 0.1.2 (BSD-3-Clause);
        // see THIRD_PARTY_NOTICES.md.
        let mut buying_pressure = vec![0.0; len];
        let mut true_range = vec![0.0; len];
        for idx in 1..len {
            let prev_close = closes[idx - 1];
            let true_low = lows[idx].min(prev_close);
            buying_pressure[idx] = closes[idx] - true_low;
            true_range[idx] = highs[idx].max(prev_close) - true_low;
        }
        let mut short_bp: f64 = buying_pressure[(long + 1 - short)..=long].iter().sum();
        let mut short_tr: f64 = true_range[(long + 1 - short)..=long].iter().sum();
        let mut medium_bp: f64 = buying_pressure[(long + 1 - medium)..=long].iter().sum();
        let mut medium_tr: f64 = true_range[(long + 1 - medium)..=long].iter().sum();
        let mut long_bp: f64 = buying_pressure[1..=long].iter().sum();
        let mut long_tr: f64 = true_range[1..=long].iter().sum();
        let emit = |idx: usize,
                    short_bp: f64,
                    short_tr: f64,
                    medium_bp: f64,
                    medium_tr: f64,
                    long_bp: f64,
                    long_tr: f64,
                    out: &mut [Option<f64>]| {
            if short_tr.abs() > f64::EPSILON
                && medium_tr.abs() > f64::EPSILON
                && long_tr.abs() > f64::EPSILON
            {
                let short_avg = short_bp / short_tr;
                let medium_avg = medium_bp / medium_tr;
                let long_avg = long_bp / long_tr;
                out[idx] = Some(100.0 * (4.0 * short_avg + 2.0 * medium_avg + long_avg) / 7.0);
            }
        };
        emit(
            long, short_bp, short_tr, medium_bp, medium_tr, long_bp, long_tr, &mut out,
        );
        for idx in (long + 1)..len {
            short_bp += buying_pressure[idx] - buying_pressure[idx - short];
            short_tr += true_range[idx] - true_range[idx - short];
            medium_bp += buying_pressure[idx] - buying_pressure[idx - medium];
            medium_tr += true_range[idx] - true_range[idx - medium];
            long_bp += buying_pressure[idx] - buying_pressure[idx - long];
            long_tr += true_range[idx] - true_range[idx - long];
            emit(
                idx, short_bp, short_tr, medium_bp, medium_tr, long_bp, long_tr, &mut out,
            );
        }
        return out;
    }
    let mut buying_pressure = vec![0.0; len];
    let mut true_range = vec![0.0; len];
    for idx in 1..len {
        let prev_close = closes[idx - 1];
        buying_pressure[idx] = closes[idx] - lows[idx].min(prev_close);
        true_range[idx] = highs[idx].max(prev_close) - lows[idx].min(prev_close);
    }
    for idx in long..len {
        let avg = |period: usize| -> Option<f64> {
            let start = idx + 1 - period;
            let bp = buying_pressure[start..=idx].iter().sum::<f64>();
            let tr = true_range[start..=idx].iter().sum::<f64>();
            (tr.abs() > f64::EPSILON).then_some(bp / tr)
        };
        if let (Some(short_avg), Some(medium_avg), Some(long_avg)) =
            (avg(short), avg(medium), avg(long))
        {
            out[idx] = Some(100.0 * (4.0 * short_avg + 2.0 * medium_avg + long_avg) / 7.0);
        }
    }
    out
}

pub fn trange(highs: &[f64], lows: &[f64], closes: &[f64]) -> Vec<Option<f64>> {
    let len = highs.len().min(lows.len()).min(closes.len());
    let mut out = vec![None; len];
    for idx in 1..len {
        let high = highs[idx];
        let low = lows[idx];
        let prev_close = closes[idx - 1];
        let value = high.max(prev_close) - low.min(prev_close);
        out[idx] = finite(value);
    }
    out
}

pub fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let tr = trange(highs, lows, closes);
    let mut out = vec![None; tr.len()];
    if period == 0 || tr.len() <= period {
        return out;
    }

    let mut sum = 0.0;
    for value in tr.iter().take(period + 1).skip(1) {
        let Some(value) = value else {
            return out;
        };
        sum += value;
    }

    let mut current = sum / period as f64;
    out[period] = Some(current);
    for idx in period + 1..tr.len() {
        let Some(value) = tr[idx] else { continue };
        current = (current * (period as f64 - 1.0) + value) / period as f64;
        out[idx] = Some(current);
    }
    out
}

pub fn dema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let ema2 = ema(&option_values(&ema1), period);
    ema1.iter()
        .zip(ema2.iter())
        .map(|(ema1, ema2)| Some(2.0 * (*ema1)? - (*ema2)?))
        .collect()
}

pub fn tema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let ema2 = ema(&option_values(&ema1), period);
    let ema3 = ema(&option_values(&ema2), period);
    ema1.iter()
        .zip(ema2.iter().zip(ema3.iter()))
        .map(|(ema1, (ema2, ema3))| Some(3.0 * (*ema1)? - 3.0 * (*ema2)? + (*ema3)?))
        .collect()
}

pub fn t3(values: &[f64], period: usize, vfactor: f64) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let ema2 = ema(&option_values(&ema1), period);
    let ema3 = ema(&option_values(&ema2), period);
    let ema4 = ema(&option_values(&ema3), period);
    let ema5 = ema(&option_values(&ema4), period);
    let ema6 = ema(&option_values(&ema5), period);
    let v2 = vfactor * vfactor;
    let v3 = v2 * vfactor;
    let c1 = -v3;
    let c2 = 3.0 * v2 + 3.0 * v3;
    let c3 = -6.0 * v2 - 3.0 * vfactor - 3.0 * v3;
    let c4 = 1.0 + 3.0 * vfactor + 3.0 * v2 + v3;
    ema3.iter()
        .zip(ema4.iter().zip(ema5.iter().zip(ema6.iter())))
        .map(|(ema3, (ema4, (ema5, ema6)))| {
            Some(c1 * (*ema6)? + c2 * (*ema5)? + c3 * (*ema4)? + c4 * (*ema3)?)
        })
        .collect()
}

pub fn trix(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let ema1 = ema(values, period);
    let ema2 = ema(&option_values(&ema1), period);
    let ema3 = ema(&option_values(&ema2), period);
    let mut out = vec![None; values.len()];
    for idx in 1..ema3.len() {
        let prior = ema3[idx - 1].filter(|prior| prior.abs() > f64::EPSILON);
        if let (Some(current), Some(prior)) = (ema3[idx], prior) {
            out[idx] = Some((current / prior - 1.0) * 100.0);
        }
    }
    out
}

pub fn price_transforms(
    opens: &[f64],
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
) -> PriceTransforms {
    let len = opens
        .len()
        .min(highs.len())
        .min(lows.len())
        .min(closes.len());
    let mut avgprice = Vec::with_capacity(len);
    let mut medprice = Vec::with_capacity(len);
    let mut typprice = Vec::with_capacity(len);
    let mut wclprice = Vec::with_capacity(len);
    for idx in 0..len {
        let open = opens[idx];
        let high = highs[idx];
        let low = lows[idx];
        let close = closes[idx];
        avgprice.push(finite((open + high + low + close) * 0.25));
        medprice.push(finite((high + low) * 0.5));
        typprice.push(finite((high + low + close) / 3.0));
        wclprice.push(finite((high + low + 2.0 * close) * 0.25));
    }
    PriceTransforms {
        avgprice,
        medprice,
        typprice,
        wclprice,
    }
}

pub fn rolling_sum(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut count = 0usize;
    for idx in 0..values.len() {
        if values[idx].is_finite() {
            sum += values[idx];
            count += 1;
        }
        if idx >= period && values[idx - period].is_finite() {
            sum -= values[idx - period];
            count -= 1;
        }
        if idx + 1 >= period && count == period {
            out[idx] = Some(sum);
        }
    }
    out
}

pub fn directional_movement(highs: &[f64], lows: &[f64]) -> DirectionalMovement {
    let len = highs.len().min(lows.len());
    let mut plus_dm = vec![None; len];
    let mut minus_dm = vec![None; len];
    for idx in 1..len {
        let high_diff = highs[idx] - highs[idx - 1];
        let low_diff = lows[idx - 1] - lows[idx];
        if !high_diff.is_finite() || !low_diff.is_finite() {
            continue;
        }
        plus_dm[idx] = Some(if high_diff > low_diff && high_diff > 0.0 {
            high_diff
        } else {
            0.0
        });
        minus_dm[idx] = Some(if low_diff > high_diff && low_diff > 0.0 {
            low_diff
        } else {
            0.0
        });
    }
    DirectionalMovement { plus_dm, minus_dm }
}

pub fn price_context(closes: &[f64]) -> PriceContext {
    let mut close_vs_ath_pct = vec![None; closes.len()];
    let mut close_vs_atl_pct = vec![None; closes.len()];
    let mut days_since_ath = vec![None; closes.len()];
    let mut days_since_atl = vec![None; closes.len()];
    let mut ath = f64::NEG_INFINITY;
    let mut atl = f64::INFINITY;
    let mut ath_idx = 0usize;
    let mut atl_idx = 0usize;
    for (idx, close) in closes.iter().copied().enumerate() {
        let Some(close) = finite(close) else {
            continue;
        };
        if close >= ath {
            ath = close;
            ath_idx = idx;
        }
        if close <= atl {
            atl = close;
            atl_idx = idx;
        }
        if ath.abs() > f64::EPSILON {
            close_vs_ath_pct[idx] = Some((close / ath - 1.0) * 100.0);
        }
        if atl.abs() > f64::EPSILON {
            close_vs_atl_pct[idx] = Some((close / atl - 1.0) * 100.0);
        }
        days_since_ath[idx] = Some((idx - ath_idx) as f64);
        days_since_atl[idx] = Some((idx - atl_idx) as f64);
    }
    PriceContext {
        close_vs_ath_pct,
        close_vs_atl_pct,
        days_since_ath,
        days_since_atl,
    }
}

pub fn linear_regression(values: &[f64], period: usize) -> LinearRegression {
    let mut line = vec![None; values.len()];
    let mut slope_out = vec![None; values.len()];
    let mut angle = vec![None; values.len()];
    let mut intercept_out = vec![None; values.len()];
    let mut tsf = vec![None; values.len()];
    if period == 0 {
        return LinearRegression {
            line,
            slope: slope_out,
            angle,
            intercept: intercept_out,
            tsf,
        };
    }
    let x_mean = (period - 1) as f64 * 0.5;
    let x_var = (0..period)
        .map(|idx| {
            let delta = idx as f64 - x_mean;
            delta * delta
        })
        .sum::<f64>();
    if x_var <= f64::EPSILON {
        return LinearRegression {
            line,
            slope: slope_out,
            angle,
            intercept: intercept_out,
            tsf,
        };
    }
    if period <= values.len() && values.iter().all(|value| value.is_finite()) {
        // Sliding weighted-sum update adapted from talib-rs 0.1.2
        // (BSD-3-Clause); see THIRD_PARTY_NOTICES.md.
        let n = period as f64;
        let sum_x = n * (n - 1.0) * 0.5;
        let sum_x2 = n * (n - 1.0) * (2.0 * n - 1.0) / 6.0;
        let denom = n * sum_x2 - sum_x * sum_x;
        if denom <= f64::EPSILON {
            return LinearRegression {
                line,
                slope: slope_out,
                angle,
                intercept: intercept_out,
                tsf,
            };
        }

        let mut sum_y = 0.0;
        let mut weighted_sum = 0.0;
        for (offset, value) in values.iter().take(period).enumerate() {
            sum_y += *value;
            weighted_sum += offset as f64 * *value;
        }
        let mut emit = |idx: usize, sum_y: f64, weighted_sum: f64| {
            let slope = (n * weighted_sum - sum_x * sum_y) / denom;
            let intercept = (sum_y - slope * sum_x) / n;
            slope_out[idx] = Some(slope);
            angle[idx] = Some(slope.atan().to_degrees());
            intercept_out[idx] = Some(intercept);
            line[idx] = Some(intercept + slope * (period - 1) as f64);
            tsf[idx] = Some(intercept + slope * period as f64);
        };
        emit(period - 1, sum_y, weighted_sum);
        for idx in period..values.len() {
            let old = values[idx - period];
            let new = values[idx];
            weighted_sum = weighted_sum - sum_y + old + (n - 1.0) * new;
            sum_y += new - old;
            emit(idx, sum_y, weighted_sum);
        }
        return LinearRegression {
            line,
            slope: slope_out,
            angle,
            intercept: intercept_out,
            tsf,
        };
    }
    for idx in period - 1..values.len() {
        let start = idx + 1 - period;
        let mut y_sum = 0.0;
        let mut valid = true;
        for value in &values[start..=idx] {
            let Some(value) = finite(*value) else {
                valid = false;
                break;
            };
            y_sum += value;
        }
        if !valid {
            continue;
        }
        let y_mean = y_sum / period as f64;
        let mut covariance = 0.0;
        for (offset, value) in values[start..=idx].iter().enumerate() {
            covariance += (offset as f64 - x_mean) * (*value - y_mean);
        }
        let slope = covariance / x_var;
        let intercept = y_mean - slope * x_mean;
        slope_out[idx] = Some(slope);
        angle[idx] = Some(slope.atan().to_degrees());
        intercept_out[idx] = Some(intercept);
        line[idx] = Some(intercept + slope * (period - 1) as f64);
        tsf[idx] = Some(intercept + slope * period as f64);
    }
    LinearRegression {
        line,
        slope: slope_out,
        angle,
        intercept: intercept_out,
        tsf,
    }
}

pub fn sar(highs: &[f64], lows: &[f64], acceleration: f64, maximum: f64) -> Vec<Option<f64>> {
    let len = highs.len().min(lows.len());
    let mut out = vec![None; len];
    if len < 2 || acceleration <= 0.0 || maximum <= 0.0 {
        return out;
    }
    // TA-Lib picks the initial trend from the first bar's directional movement,
    // seeds SAR/EP from bars 0 and 1, emits the seed at index 1 with no step,
    // and only folds in the second prior extreme once two real prior bars exist.
    let dm_plus = highs[1] - highs[0];
    let dm_minus = lows[0] - lows[1];
    let mut long = !(dm_minus > 0.0 && dm_minus > dm_plus);
    let (mut sar, mut ep) = if long {
        (lows[0], highs[1])
    } else {
        (highs[0], lows[1])
    };
    let mut af = acceleration;
    out[1] = finite(sar);

    for idx in 2..len {
        sar += af * (ep - sar);
        if long {
            sar = sar.min(lows[idx - 1]);
            if idx >= 3 {
                sar = sar.min(lows[idx - 2]);
            }
            if lows[idx] < sar {
                long = false;
                sar = ep;
                ep = lows[idx];
                af = acceleration;
            } else if highs[idx] > ep {
                ep = highs[idx];
                af = (af + acceleration).min(maximum);
            }
        } else {
            sar = sar.max(highs[idx - 1]);
            if idx >= 3 {
                sar = sar.max(highs[idx - 2]);
            }
            if highs[idx] > sar {
                long = true;
                sar = ep;
                ep = highs[idx];
                af = acceleration;
            } else if lows[idx] < ep {
                ep = lows[idx];
                af = (af + acceleration).min(maximum);
            }
        }
        out[idx] = finite(sar);
    }
    out
}

/// Parabolic SAR Extended (TA-Lib `SAREXT`). Mirrors the proven `sar` core but
/// adds: explicit start value / initial direction, per-side acceleration
/// (init/step/max for long and short), an offset-on-reverse, and TA-Lib's sign
/// convention where short bars are emitted as the negated SAR. First value at
/// index 1.
#[allow(clippy::too_many_arguments)]
pub fn sarext(
    highs: &[f64],
    lows: &[f64],
    start_value: f64,
    offset_on_reverse: f64,
    accel_init_long: f64,
    accel_long: f64,
    accel_max_long: f64,
    accel_init_short: f64,
    accel_short: f64,
    accel_max_short: f64,
) -> Vec<Option<f64>> {
    let len = highs.len().min(lows.len());
    let mut out = vec![None; len];
    if len < 2 {
        return out;
    }
    let mut long = if start_value == 0.0 {
        let dm_plus = highs[1] - highs[0];
        let dm_minus = lows[0] - lows[1];
        !(dm_minus > 0.0 && dm_minus > dm_plus)
    } else {
        start_value > 0.0
    };
    let (mut sar, mut ep, mut af) = if long {
        let seed = if start_value == 0.0 {
            lows[0]
        } else {
            start_value.abs()
        };
        (seed, highs[1], accel_init_long)
    } else {
        let seed = if start_value == 0.0 {
            highs[0]
        } else {
            start_value.abs()
        };
        (seed, lows[1], accel_init_short)
    };
    out[1] = finite(if long { sar } else { -sar });

    for idx in 2..len {
        sar += af * (ep - sar);
        if long {
            sar = sar.min(lows[idx - 1]);
            if idx >= 3 {
                sar = sar.min(lows[idx - 2]);
            }
            if lows[idx] < sar {
                long = false;
                sar = ep;
                if offset_on_reverse != 0.0 {
                    sar += sar * offset_on_reverse;
                }
                ep = lows[idx];
                af = accel_init_short;
            } else if highs[idx] > ep {
                ep = highs[idx];
                af = (af + accel_long).min(accel_max_long);
            }
        } else {
            sar = sar.max(highs[idx - 1]);
            if idx >= 3 {
                sar = sar.max(highs[idx - 2]);
            }
            if highs[idx] > sar {
                long = true;
                sar = ep;
                if offset_on_reverse != 0.0 {
                    sar -= sar * offset_on_reverse;
                }
                ep = highs[idx];
                af = accel_init_long;
            } else if lows[idx] < ep {
                ep = lows[idx];
                af = (af + accel_short).min(accel_max_short);
            }
        }
        out[idx] = finite(if long { sar } else { -sar });
    }
    out
}

pub fn ad(highs: &[f64], lows: &[f64], closes: &[f64], volumes: &[f64]) -> Vec<Option<f64>> {
    let mut out = vec![None; closes.len()];
    let mut current = 0.0;
    for idx in 0..closes.len() {
        let high = highs[idx];
        let low = lows[idx];
        let close = closes[idx];
        let volume = volumes[idx];
        let range = high - low;
        if !range.is_finite() || range.abs() <= f64::EPSILON || !volume.is_finite() {
            out[idx] = Some(current);
            continue;
        }
        let multiplier = ((close - low) - (high - close)) / range;
        if multiplier.is_finite() {
            current += multiplier * volume;
        }
        out[idx] = Some(current);
    }
    out
}

pub fn adosc(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
    fast: usize,
    slow: usize,
) -> Vec<Option<f64>> {
    let mut ad_line = vec![0.0; closes.len()];
    let mut current = 0.0;
    for idx in 0..closes.len() {
        let high = highs[idx];
        let low = lows[idx];
        let close = closes[idx];
        let volume = volumes[idx];
        let range = high - low;
        if range.is_finite() && range.abs() > f64::EPSILON && volume.is_finite() {
            let multiplier = ((close - low) - (high - close)) / range;
            if multiplier.is_finite() {
                current += multiplier * volume;
            }
        }
        ad_line[idx] = current;
    }
    let len = ad_line.len();
    let mut out = vec![None; len];
    if fast == 0 || slow == 0 || len == 0 {
        return out;
    }
    // TA-Lib seeds both AD-line EMAs with the first AD value, runs the recurrence
    // from bar 1, and emits the oscillator from `slow - 1`.
    let fast_k = 2.0 / (fast as f64 + 1.0);
    let slow_k = 2.0 / (slow as f64 + 1.0);
    let mut fast_ema = ad_line[0];
    let mut slow_ema = ad_line[0];
    let first_out = slow - 1;
    for idx in 1..len {
        let value = ad_line[idx];
        fast_ema = fast_k * value + (1.0 - fast_k) * fast_ema;
        slow_ema = slow_k * value + (1.0 - slow_k) * slow_ema;
        if idx >= first_out {
            out[idx] = Some(fast_ema - slow_ema);
        }
    }
    out
}

pub fn adx_family(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> AdxFamily {
    let len = highs.len().min(lows.len()).min(closes.len());
    let mut plus_di = vec![None; len];
    let mut minus_di = vec![None; len];
    let mut dx = vec![None; len];
    let mut adx = vec![None; len];
    let mut adxr = vec![None; len];
    if period == 0 || len <= period {
        return AdxFamily {
            adx,
            adxr,
            plus_di,
            minus_di,
            dx,
        };
    }

    let mut tr = vec![0.0; len];
    let mut plus_dm = vec![0.0; len];
    let mut minus_dm = vec![0.0; len];
    for idx in 1..len {
        let high_diff = highs[idx] - highs[idx - 1];
        let low_diff = lows[idx - 1] - lows[idx];
        plus_dm[idx] = if high_diff > low_diff && high_diff > 0.0 {
            high_diff
        } else {
            0.0
        };
        minus_dm[idx] = if low_diff > high_diff && low_diff > 0.0 {
            low_diff
        } else {
            0.0
        };
        tr[idx] = (highs[idx] - lows[idx])
            .max((highs[idx] - closes[idx - 1]).abs())
            .max((lows[idx] - closes[idx - 1]).abs());
    }

    // TA-Lib seeds the Wilder sums over the first `period - 1` deltas (indices
    // 1..=period-1), then applies the first smoothing step at `idx == period`,
    // which folds in the period-th delta. Using the Wilder "sum" form here is
    // exact for the DI ratios (the period factor cancels).
    let mut smooth_tr = tr[1..period].iter().sum::<f64>();
    let mut smooth_plus = plus_dm[1..period].iter().sum::<f64>();
    let mut smooth_minus = minus_dm[1..period].iter().sum::<f64>();
    for idx in period..len {
        smooth_tr = smooth_tr - smooth_tr / period as f64 + tr[idx];
        smooth_plus = smooth_plus - smooth_plus / period as f64 + plus_dm[idx];
        smooth_minus = smooth_minus - smooth_minus / period as f64 + minus_dm[idx];
        if smooth_tr > f64::EPSILON {
            let plus = 100.0 * smooth_plus / smooth_tr;
            let minus = 100.0 * smooth_minus / smooth_tr;
            plus_di[idx] = Some(plus);
            minus_di[idx] = Some(minus);
            let sum = plus + minus;
            dx[idx] = if sum > f64::EPSILON {
                Some(100.0 * (plus - minus).abs() / sum)
            } else {
                Some(0.0)
            };
        }
    }

    let mut warm_dx = Vec::with_capacity(period);
    let mut smooth_adx = None;
    for idx in period..len {
        let Some(current_dx) = dx[idx] else {
            continue;
        };
        if let Some(prev) = smooth_adx {
            let next = (prev * (period as f64 - 1.0) + current_dx) / period as f64;
            smooth_adx = Some(next);
            adx[idx] = Some(next);
        } else {
            warm_dx.push(current_dx);
            if warm_dx.len() == period {
                let next = warm_dx.iter().sum::<f64>() / period as f64;
                smooth_adx = Some(next);
                adx[idx] = Some(next);
            }
        }
    }

    // TA-Lib ADXR averages the current ADX with the ADX from `period - 1` bars
    // back (not `period`).
    let adxr_lag = period - 1;
    for idx in adxr_lag..len {
        if let (Some(current), Some(prior)) = (adx[idx], adx[idx - adxr_lag]) {
            adxr[idx] = Some((current + prior) * 0.5);
        }
    }

    AdxFamily {
        adx,
        adxr,
        plus_di,
        minus_di,
        dx,
    }
}

pub fn aroon(highs: &[f64], lows: &[f64], period: usize) -> Aroon {
    let len = highs.len().min(lows.len());
    let mut up = vec![None; len];
    let mut down = vec![None; len];
    let mut oscillator = vec![None; len];
    if period == 0 {
        return Aroon {
            up,
            down,
            oscillator,
        };
    }
    if period < len
        && highs[..len].iter().all(|value| value.is_finite())
        && lows[..len].iter().all(|value| value.is_finite())
    {
        // Rolling extremum index update adapted from talib-rs 0.1.2
        // (BSD-3-Clause); see THIRD_PARTY_NOTICES.md.
        let scale = 100.0 / period as f64;
        let window = period + 1;
        let mut high_value = highs[0];
        let mut high_idx = 0usize;
        let mut low_value = lows[0];
        let mut low_idx = 0usize;
        for idx in 1..window {
            if highs[idx] >= high_value {
                high_value = highs[idx];
                high_idx = idx;
            }
            if lows[idx] <= low_value {
                low_value = lows[idx];
                low_idx = idx;
            }
        }
        let emit = |idx: usize,
                    high_idx: usize,
                    low_idx: usize,
                    up: &mut [Option<f64>],
                    down: &mut [Option<f64>],
                    oscillator: &mut [Option<f64>]| {
            let up_value = (period - (idx - high_idx)) as f64 * scale;
            let down_value = (period - (idx - low_idx)) as f64 * scale;
            up[idx] = Some(up_value);
            down[idx] = Some(down_value);
            oscillator[idx] = Some(up_value - down_value);
        };
        emit(
            period,
            high_idx,
            low_idx,
            &mut up,
            &mut down,
            &mut oscillator,
        );

        let mut trailing_idx = 1usize;
        for idx in (period + 1)..len {
            if high_idx < trailing_idx {
                high_idx = trailing_idx;
                high_value = highs[trailing_idx];
                for (offset, value) in highs[trailing_idx + 1..=idx].iter().enumerate() {
                    if *value >= high_value {
                        high_value = *value;
                        high_idx = trailing_idx + 1 + offset;
                    }
                }
            } else if highs[idx] >= high_value {
                high_value = highs[idx];
                high_idx = idx;
            }

            if low_idx < trailing_idx {
                low_idx = trailing_idx;
                low_value = lows[trailing_idx];
                for (offset, value) in lows[trailing_idx + 1..=idx].iter().enumerate() {
                    if *value <= low_value {
                        low_value = *value;
                        low_idx = trailing_idx + 1 + offset;
                    }
                }
            } else if lows[idx] <= low_value {
                low_value = lows[idx];
                low_idx = idx;
            }

            emit(idx, high_idx, low_idx, &mut up, &mut down, &mut oscillator);
            trailing_idx += 1;
        }
        return Aroon {
            up,
            down,
            oscillator,
        };
    }
    // TA-Lib scans a trailing window of `period + 1` bars (the current bar plus
    // the prior `period`), emitting the first value at index `period`.
    for idx in period..len {
        let start = idx - period;
        let mut high_value = f64::NEG_INFINITY;
        let mut low_value = f64::INFINITY;
        let mut high_idx = start;
        let mut low_idx = start;
        let mut valid = true;
        for offset in start..=idx {
            let high = highs[offset];
            let low = lows[offset];
            if !high.is_finite() || !low.is_finite() {
                valid = false;
                break;
            }
            if high >= high_value {
                high_value = high;
                high_idx = offset;
            }
            if low <= low_value {
                low_value = low;
                low_idx = offset;
            }
        }
        if valid {
            let scale = 100.0 / period as f64;
            let up_value = (period - (idx - high_idx)) as f64 * scale;
            let down_value = (period - (idx - low_idx)) as f64 * scale;
            up[idx] = Some(up_value);
            down[idx] = Some(down_value);
            oscillator[idx] = Some(up_value - down_value);
        }
    }
    Aroon {
        up,
        down,
        oscillator,
    }
}

// ===========================================================================
// Hilbert Transform family (John Ehlers): HT_DCPERIOD, HT_PHASOR, MAMA/FAMA,
// HT_DCPHASE, HT_SINE, HT_TRENDLINE, HT_TRENDMODE.
//
// Faithful port of TA-Lib's shared Hilbert core. Every 32-lookback function
// (DCPERIOD, PHASOR, MAMA/FAMA) shares one identical warmup trajectory, and
// every 63-lookback function (DCPHASE, SINE, TRENDLINE, TRENDMODE) shares
// another. Because the cores are bit-for-bit identical across functions in a
// group, we run two unified passes and slice each function's output from them
// (this yields exactly the same values TA-Lib produces per-function).
// ===========================================================================

const HT_A: f64 = 0.0962;
const HT_B: f64 = 0.5769;

#[inline]
fn ht_rad2deg() -> f64 {
    // TA-Lib: rad2Deg = 45.0 / atan(1) = 180/PI.
    45.0 / (1.0_f64).atan()
}

/// Period-4 weighted moving average smoother (TA-Lib DO_PRICE_WMA macro state).
struct HtWma {
    sub: f64,
    sum: f64,
    trailing_value: f64,
    trailing_idx: usize,
}

impl HtWma {
    #[inline]
    fn next(&mut self, new_price: f64, close: &[f64]) -> f64 {
        self.sub += new_price;
        self.sub -= self.trailing_value;
        self.sum += new_price * 4.0;
        self.trailing_value = close[self.trailing_idx];
        self.trailing_idx += 1;
        let smoothed = self.sum * 0.1;
        self.sum -= self.sub;
        smoothed
    }
}

/// One Hilbert transform channel with odd/even circular buffers
/// (TA-Lib HILBERT_VARIABLES / DO_HILBERT_TRANSFORM macros).
#[derive(Default)]
struct HtChannel {
    odd: [f64; 3],
    even: [f64; 3],
    prev_odd: f64,
    prev_even: f64,
    prev_in_odd: f64,
    prev_in_even: f64,
}

impl HtChannel {
    #[inline]
    fn transform(&mut self, input: f64, adjusted_prev_period: f64, idx: usize, even: bool) -> f64 {
        let temp = HT_A * input;
        let mut value;
        if even {
            value = -self.even[idx];
            self.even[idx] = temp;
            value += temp;
            value -= self.prev_even;
            self.prev_even = HT_B * self.prev_in_even;
            value += self.prev_even;
            self.prev_in_even = input;
        } else {
            value = -self.odd[idx];
            self.odd[idx] = temp;
            value += temp;
            value -= self.prev_odd;
            self.prev_odd = HT_B * self.prev_in_odd;
            value += self.prev_odd;
            self.prev_in_odd = input;
        }
        value * adjusted_prev_period
    }
}

struct Ht32 {
    dcperiod: Vec<Option<f64>>,
    inphase: Vec<Option<f64>>,
    quadrature: Vec<Option<f64>>,
    mama: Vec<Option<f64>>,
    fama: Vec<Option<f64>>,
}

/// Unified 32-lookback Hilbert pass. `fast_limit`/`slow_limit` only affect the
/// MAMA/FAMA outputs; DCPERIOD and PHASOR are independent of them.
fn ht32(close: &[f64], fast_limit: f64, slow_limit: f64) -> Ht32 {
    let n = close.len();
    let lookback = 32usize;
    let mut dcperiod = vec![None; n];
    let mut inphase = vec![None; n];
    let mut quadrature = vec![None; n];
    let mut mama_out = vec![None; n];
    let mut fama_out = vec![None; n];
    if n <= lookback {
        return Ht32 {
            dcperiod,
            inphase,
            quadrature,
            mama: mama_out,
            fama: fama_out,
        };
    }
    let rad2deg = ht_rad2deg();
    let start_idx = lookback;
    let end_idx = n - 1;

    let mut today = 0usize; // trailing_wma_idx = start_idx - lookback = 0
    let mut t = close[today];
    today += 1;
    let mut sub = t;
    let mut sum = t;
    t = close[today];
    today += 1;
    sub += t;
    sum += t * 2.0;
    t = close[today];
    today += 1;
    sub += t;
    sum += t * 3.0;
    let mut wma = HtWma {
        sub,
        sum,
        trailing_value: 0.0,
        trailing_idx: 0,
    };
    for _ in 0..9 {
        let p = close[today];
        today += 1;
        wma.next(p, close);
    }

    let mut hilbert_idx = 0usize;
    let mut detrender = HtChannel::default();
    let mut q1c = HtChannel::default();
    let mut jic = HtChannel::default();
    let mut jqc = HtChannel::default();
    let mut period = 0.0;
    let mut smooth_period = 0.0;
    let mut prev_i2 = 0.0;
    let mut prev_q2 = 0.0;
    let mut re = 0.0;
    let mut im = 0.0;
    let mut i1_odd_prev3 = 0.0;
    let mut i1_odd_prev2 = 0.0;
    let mut i1_even_prev3 = 0.0;
    let mut i1_even_prev2 = 0.0;
    let mut mama = 0.0;
    let mut fama = 0.0;
    let mut prev_phase = 0.0;

    while today <= end_idx {
        let adjusted_prev_period = 0.075 * period + 0.54;
        let today_value = close[today];
        let smoothed = wma.next(today_value, close);

        let q1v;
        let i1;
        let q2;
        let i2;
        if today % 2 == 0 {
            let det = detrender.transform(smoothed, adjusted_prev_period, hilbert_idx, true);
            let q1n = q1c.transform(det, adjusted_prev_period, hilbert_idx, true);
            let jin = jic.transform(i1_even_prev3, adjusted_prev_period, hilbert_idx, true);
            let jqn = jqc.transform(q1n, adjusted_prev_period, hilbert_idx, true);
            hilbert_idx += 1;
            if hilbert_idx == 3 {
                hilbert_idx = 0;
            }
            q2 = 0.2 * (q1n + jin) + 0.8 * prev_q2;
            i2 = 0.2 * (i1_even_prev3 - jqn) + 0.8 * prev_i2;
            i1_odd_prev3 = i1_odd_prev2;
            i1_odd_prev2 = det;
            q1v = q1n;
            i1 = i1_even_prev3;
        } else {
            let det = detrender.transform(smoothed, adjusted_prev_period, hilbert_idx, false);
            let q1n = q1c.transform(det, adjusted_prev_period, hilbert_idx, false);
            let jin = jic.transform(i1_odd_prev3, adjusted_prev_period, hilbert_idx, false);
            let jqn = jqc.transform(q1n, adjusted_prev_period, hilbert_idx, false);
            q2 = 0.2 * (q1n + jin) + 0.8 * prev_q2;
            i2 = 0.2 * (i1_odd_prev3 - jqn) + 0.8 * prev_i2;
            i1_even_prev3 = i1_even_prev2;
            i1_even_prev2 = det;
            q1v = q1n;
            i1 = i1_odd_prev3;
        }

        // MAMA / FAMA adaptive smoothing.
        let alpha_phase = if i1 != 0.0 {
            (q1v / i1).atan() * rad2deg
        } else {
            0.0
        };
        let mut delta = prev_phase - alpha_phase;
        prev_phase = alpha_phase;
        if delta < 1.0 {
            delta = 1.0;
        }
        let alpha = if delta > 1.0 {
            (fast_limit / delta).max(slow_limit)
        } else {
            fast_limit
        };
        mama = alpha * today_value + (1.0 - alpha) * mama;
        let alpha_half = alpha * 0.5;
        fama = alpha_half * mama + (1.0 - alpha_half) * fama;

        // Dominant-cycle period update (shared by all 32-lookback functions).
        re = 0.2 * ((i2 * prev_i2) + (q2 * prev_q2)) + 0.8 * re;
        im = 0.2 * ((i2 * prev_q2) - (q2 * prev_i2)) + 0.8 * im;
        prev_q2 = q2;
        prev_i2 = i2;
        let prev_period = period;
        if im != 0.0 && re != 0.0 {
            period = 360.0 / ((im / re).atan() * rad2deg);
        }
        let hi = 1.5 * prev_period;
        if period > hi {
            period = hi;
        }
        let lo = 0.67 * prev_period;
        if period < lo {
            period = lo;
        }
        period = period.clamp(6.0, 50.0);
        period = 0.2 * period + 0.8 * prev_period;
        smooth_period = 0.33 * period + 0.67 * smooth_period;

        if today >= start_idx {
            dcperiod[today] = Some(smooth_period);
            inphase[today] = Some(i1);
            quadrature[today] = Some(q1v);
            mama_out[today] = Some(mama);
            fama_out[today] = Some(fama);
        }
        today += 1;
    }

    Ht32 {
        dcperiod,
        inphase,
        quadrature,
        mama: mama_out,
        fama: fama_out,
    }
}

struct Ht63 {
    dcphase: Vec<Option<f64>>,
    sine: Vec<Option<f64>>,
    leadsine: Vec<Option<f64>>,
    trendline: Vec<Option<f64>>,
    trendmode: Vec<Option<f64>>,
}

/// Unified 63-lookback Hilbert pass (DCPHASE, SINE, TRENDLINE, TRENDMODE).
fn ht63(close: &[f64]) -> Ht63 {
    const SMOOTH_PRICE_SIZE: usize = 50;
    let n = close.len();
    let lookback = 63usize;
    let mut dcphase = vec![None; n];
    let mut sine = vec![None; n];
    let mut leadsine = vec![None; n];
    let mut trendline = vec![None; n];
    // TA-Lib zero-fills the TREND_MODE warmup region (integer 0/1, no nulls).
    let mut trendmode = vec![Some(0.0); n];
    if n <= lookback {
        return Ht63 {
            dcphase,
            sine,
            leadsine,
            trendline,
            trendmode,
        };
    }
    let rad2deg = ht_rad2deg();
    let deg2rad = 1.0 / rad2deg;
    let const_deg2rad_by360 = (1.0_f64).atan() * 8.0; // = 2*PI
    let start_idx = lookback;
    let end_idx = n - 1;

    let mut today = 0usize;
    let mut t = close[today];
    today += 1;
    let mut sub = t;
    let mut sum = t;
    t = close[today];
    today += 1;
    sub += t;
    sum += t * 2.0;
    t = close[today];
    today += 1;
    sub += t;
    sum += t * 3.0;
    let mut wma = HtWma {
        sub,
        sum,
        trailing_value: 0.0,
        trailing_idx: 0,
    };
    for _ in 0..34 {
        let p = close[today];
        today += 1;
        wma.next(p, close);
    }

    let mut hilbert_idx = 0usize;
    let mut detrender = HtChannel::default();
    let mut q1c = HtChannel::default();
    let mut jic = HtChannel::default();
    let mut jqc = HtChannel::default();
    let mut period = 0.0;
    let mut smooth_period = 0.0;
    let mut prev_i2 = 0.0;
    let mut prev_q2 = 0.0;
    let mut re = 0.0;
    let mut im = 0.0;
    let mut i1_odd_prev3 = 0.0;
    let mut i1_odd_prev2 = 0.0;
    let mut i1_even_prev3 = 0.0;
    let mut i1_even_prev2 = 0.0;

    let mut smooth_price = [0.0f64; SMOOTH_PRICE_SIZE];
    let mut smooth_price_idx = 0usize;
    let mut dc_phase = 0.0;
    let mut sine_val = 0.0;
    let mut lead_sine_val = 0.0;
    let mut i_trend1 = 0.0;
    let mut i_trend2 = 0.0;
    let mut i_trend3 = 0.0;
    let mut days_in_trend = 0i32;

    while today <= end_idx {
        let adjusted_prev_period = 0.075 * period + 0.54;
        let today_value = close[today];
        let smoothed = wma.next(today_value, close);
        smooth_price[smooth_price_idx] = smoothed;

        let q2;
        let i2;
        if today % 2 == 0 {
            let det = detrender.transform(smoothed, adjusted_prev_period, hilbert_idx, true);
            let q1n = q1c.transform(det, adjusted_prev_period, hilbert_idx, true);
            let jin = jic.transform(i1_even_prev3, adjusted_prev_period, hilbert_idx, true);
            let jqn = jqc.transform(q1n, adjusted_prev_period, hilbert_idx, true);
            hilbert_idx += 1;
            if hilbert_idx == 3 {
                hilbert_idx = 0;
            }
            q2 = 0.2 * (q1n + jin) + 0.8 * prev_q2;
            i2 = 0.2 * (i1_even_prev3 - jqn) + 0.8 * prev_i2;
            i1_odd_prev3 = i1_odd_prev2;
            i1_odd_prev2 = det;
        } else {
            let det = detrender.transform(smoothed, adjusted_prev_period, hilbert_idx, false);
            let q1n = q1c.transform(det, adjusted_prev_period, hilbert_idx, false);
            let jin = jic.transform(i1_odd_prev3, adjusted_prev_period, hilbert_idx, false);
            let jqn = jqc.transform(q1n, adjusted_prev_period, hilbert_idx, false);
            q2 = 0.2 * (q1n + jin) + 0.8 * prev_q2;
            i2 = 0.2 * (i1_odd_prev3 - jqn) + 0.8 * prev_i2;
            i1_even_prev3 = i1_even_prev2;
            i1_even_prev2 = det;
        }

        re = 0.2 * ((i2 * prev_i2) + (q2 * prev_q2)) + 0.8 * re;
        im = 0.2 * ((i2 * prev_q2) - (q2 * prev_i2)) + 0.8 * im;
        prev_q2 = q2;
        prev_i2 = i2;
        let prev_period = period;
        if im != 0.0 && re != 0.0 {
            period = 360.0 / ((im / re).atan() * rad2deg);
        }
        let hi = 1.5 * prev_period;
        if period > hi {
            period = hi;
        }
        let lo = 0.67 * prev_period;
        if period < lo {
            period = lo;
        }
        period = period.clamp(6.0, 50.0);
        period = 0.2 * period + 0.8 * prev_period;
        smooth_period = 0.33 * period + 0.67 * smooth_period;

        let prev_dc_phase = dc_phase;

        // Dominant cycle phase from the smoothed-price circular buffer.
        let dc_period_int = (smooth_period + 0.5) as i32;
        let mut real_part = 0.0;
        let mut imag_part = 0.0;
        let mut idx = smooth_price_idx;
        for i in 0..dc_period_int {
            let angle = (i as f64 * const_deg2rad_by360) / dc_period_int as f64;
            let value = smooth_price[idx];
            real_part += angle.sin() * value;
            imag_part += angle.cos() * value;
            if idx == 0 {
                idx = SMOOTH_PRICE_SIZE - 1;
            } else {
                idx -= 1;
            }
        }
        let abs_imag = imag_part.abs();
        if abs_imag > 0.0 {
            dc_phase = (real_part / imag_part).atan() * rad2deg;
        } else if abs_imag <= 0.01 {
            if real_part < 0.0 {
                dc_phase -= 90.0;
            } else if real_part > 0.0 {
                dc_phase += 90.0;
            }
        }
        dc_phase += 90.0;
        dc_phase += 360.0 / smooth_period;
        if imag_part < 0.0 {
            dc_phase += 180.0;
        }
        if dc_phase > 315.0 {
            dc_phase -= 360.0;
        }

        let prev_sine = sine_val;
        let prev_lead_sine = lead_sine_val;
        sine_val = (dc_phase * deg2rad).sin();
        lead_sine_val = ((dc_phase + 45.0) * deg2rad).sin();

        // Trendline: average of raw close over the dominant-cycle period,
        // smoothed by the iTrend WMA.
        let dc_period_int2 = (smooth_period + 0.5) as i32;
        let mut sum_close = 0.0;
        let mut k = today as i64;
        for _ in 0..dc_period_int2 {
            if k < 0 {
                break;
            }
            sum_close += close[k as usize];
            k -= 1;
        }
        if dc_period_int2 > 0 {
            sum_close /= dc_period_int2 as f64;
        }
        let trendline_val = (4.0 * sum_close + 3.0 * i_trend1 + 2.0 * i_trend2 + i_trend3) / 10.0;
        i_trend3 = i_trend2;
        i_trend2 = i_trend1;
        i_trend1 = sum_close;

        // Trend mode (assume trend, then disqualify).
        let mut trend = 1i32;
        if (sine_val > lead_sine_val && prev_sine <= prev_lead_sine)
            || (sine_val < lead_sine_val && prev_sine >= prev_lead_sine)
        {
            days_in_trend = 0;
            trend = 0;
        }
        days_in_trend += 1;
        if (days_in_trend as f64) < 0.5 * smooth_period {
            trend = 0;
        }
        let dphase_delta = dc_phase - prev_dc_phase;
        if smooth_period != 0.0
            && dphase_delta > 0.67 * 360.0 / smooth_period
            && dphase_delta < 1.5 * 360.0 / smooth_period
        {
            trend = 0;
        }
        let cur_smooth = smooth_price[smooth_price_idx];
        if trendline_val != 0.0 && ((cur_smooth - trendline_val) / trendline_val).abs() >= 0.015 {
            trend = 1;
        }

        if today >= start_idx {
            dcphase[today] = Some(dc_phase);
            sine[today] = Some(sine_val);
            leadsine[today] = Some(lead_sine_val);
            trendline[today] = Some(trendline_val);
            trendmode[today] = Some(trend as f64);
        }

        smooth_price_idx += 1;
        if smooth_price_idx == SMOOTH_PRICE_SIZE {
            smooth_price_idx = 0;
        }
        today += 1;
    }

    Ht63 {
        dcphase,
        sine,
        leadsine,
        trendline,
        trendmode,
    }
}

/// HT_DCPERIOD — dominant cycle period.
pub fn ht_dcperiod(close: &[f64]) -> Vec<Option<f64>> {
    ht32(close, 0.5, 0.05).dcperiod
}

/// HT_PHASOR — (in-phase, quadrature) components.
pub fn ht_phasor(close: &[f64]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let out = ht32(close, 0.5, 0.05);
    (out.inphase, out.quadrature)
}

/// MAMA — (MAMA, FAMA) adaptive moving averages.
pub fn mama(
    close: &[f64],
    fast_limit: f64,
    slow_limit: f64,
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let out = ht32(close, fast_limit, slow_limit);
    (out.mama, out.fama)
}

/// HT_DCPHASE — dominant cycle phase.
pub fn ht_dcphase(close: &[f64]) -> Vec<Option<f64>> {
    ht63(close).dcphase
}

/// HT_SINE — (sine, lead sine) wave.
pub fn ht_sine(close: &[f64]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let out = ht63(close);
    (out.sine, out.leadsine)
}

/// HT_TRENDLINE — instantaneous trendline.
pub fn ht_trendline(close: &[f64]) -> Vec<Option<f64>> {
    ht63(close).trendline
}

/// HT_TRENDMODE — trend vs cycle mode (1 = trend, 0 = cycle; warmup zero-filled).
pub fn ht_trendmode(close: &[f64]) -> Vec<Option<f64>> {
    ht63(close).trendmode
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("expected Some value");
        assert!(
            (actual - expected).abs() < 1e-9,
            "actual {actual} expected {expected}"
        );
    }

    #[test]
    fn macd_hist_is_difference_between_macd_and_signal() {
        let values = (1..40).map(|value| value as f64).collect::<Vec<_>>();
        let out = macd(&values, 3, 6, 3);
        let idx = out.hist.iter().position(Option::is_some).unwrap();
        assert_close(
            out.hist[idx],
            out.macd[idx].unwrap() - out.signal[idx].unwrap(),
        );
        let fixed = macdfix(&values, 9);
        assert!(fixed.hist.iter().any(Option::is_some));
    }

    #[test]
    fn dema_and_tema_warm_after_nested_emas() {
        let values = (1..30).map(|value| value as f64).collect::<Vec<_>>();
        assert!(dema(&values, 5).iter().any(Option::is_some));
        assert!(tema(&values, 5).iter().any(Option::is_some));
        assert!(t3(&values, 5, 0.7).iter().any(Option::is_some));
        assert!(trix(&values, 5).iter().any(Option::is_some));
        assert!(kama(&values, 5).iter().any(Option::is_some));
    }

    #[test]
    fn bollinger_bands_match_population_std_window() {
        let values = [1.0, 2.0, 3.0];
        let out = bollinger_bands(&values, 3, 2.0);
        let std = (2.0_f64 / 3.0).sqrt();
        assert_close(out.middle[2], 2.0);
        assert_close(out.upper[2], 2.0 + 2.0 * std);
        assert_close(out.lower[2], 2.0 - 2.0 * std);
    }

    #[test]
    fn price_transform_formulas_match_talib_names() {
        let out = price_transforms(&[1.0], &[4.0], &[2.0], &[3.0]);
        assert_close(out.avgprice[0], 2.5);
        assert_close(out.medprice[0], 3.0);
        assert_close(out.typprice[0], 3.0);
        assert_close(out.wclprice[0], 3.0);
    }

    #[test]
    fn bop_cmo_and_ultosc_emit_bounded_momentum_values() {
        let opens = [9.0, 10.0, 11.0, 10.0, 9.0, 10.0, 11.0, 12.0];
        let highs = [10.0, 11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0];
        let lows = [8.0, 9.0, 10.0, 9.0, 8.0, 9.0, 10.0, 11.0];
        let closes = [10.0, 11.0, 10.0, 9.0, 10.0, 11.0, 12.0, 13.0];
        assert_close(bop(&opens, &highs, &lows, &closes)[0], 0.5);
        assert_close(cmo(&closes, 3)[3], -100.0 / 3.0);
        assert!(
            ultosc(&highs, &lows, &closes, 2, 3, 4)[4]
                .is_some_and(|value| (0.0..=100.0).contains(&value))
        );
    }

    #[test]
    fn rsi_and_stochrsi_warm_then_emit_bounded_values() {
        let values = [
            10.0, 11.0, 12.0, 11.0, 13.0, 14.0, 13.0, 15.0, 16.0, 15.0, 17.0, 18.0, 19.0, 18.0,
            20.0, 21.0,
        ];
        let rsi_out = rsi(&values, 5);
        assert_eq!(rsi_out[4], None);
        assert!(
            rsi_out
                .iter()
                .flatten()
                .all(|value| (0.0..=100.0).contains(value))
        );

        let stoch = stochrsi(&values, 5, 5, 3);
        assert!(stoch.k.iter().any(Option::is_some));
        assert!(stoch.d.iter().any(Option::is_some));
        assert!(
            stoch
                .k
                .iter()
                .flatten()
                .all(|value| (0.0..=100.0).contains(value))
        );
        assert!(
            stoch
                .d
                .iter()
                .flatten()
                .all(|value| (0.0..=100.0).contains(value))
        );
    }

    #[test]
    fn linear_regression_family_matches_simple_line() {
        let values = [2.0, 4.0, 6.0, 8.0, 10.0];
        let out = linear_regression(&values, 3);
        assert_close(out.intercept[2], 2.0);
        assert_close(out.slope[2], 2.0);
        assert_close(out.line[2], 6.0);
        assert_close(out.tsf[2], 8.0);
        assert_close(out.angle[2], 2.0_f64.atan().to_degrees());
    }

    #[test]
    fn rolling_sum_and_sar_have_explicit_warmup() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(rolling_sum(&values, 3)[1], None);
        assert_close(rolling_sum(&values, 3)[2], 6.0);
        let highs = [10.0, 11.0, 12.0, 13.0];
        let lows = [9.0, 10.0, 11.0, 12.0];
        let out = sar(&highs, &lows, 0.02, 0.2);
        assert_eq!(out[0], None);
        assert!(out.iter().skip(1).all(Option::is_some));
    }

    #[test]
    fn directional_movement_and_price_context_are_point_in_time() {
        let highs = [10.0, 12.0, 11.0, 14.0];
        let lows = [9.0, 10.0, 8.0, 11.0];
        let dm = directional_movement(&highs, &lows);
        assert_eq!(dm.plus_dm[0], None);
        assert_close(dm.plus_dm[1], 2.0);
        assert_close(dm.minus_dm[2], 2.0);

        let closes = [10.0, 12.0, 9.0, 15.0, 14.0];
        let context = price_context(&closes);
        assert_close(context.close_vs_ath_pct[2], -25.0);
        assert_close(context.close_vs_atl_pct[3], 66.66666666666666);
        assert_close(context.days_since_ath[4], 1.0);
        assert_close(context.days_since_atl[4], 2.0);
    }

    #[test]
    fn aroon_detects_recent_high_and_old_low() {
        // TA-Lib scans a `period + 1` bar window and emits the first value at
        // index `period`, so a period-4 Aroon needs 5 bars.
        let highs = [1.0, 2.0, 3.0, 4.0, 5.0];
        let lows = [1.0, 2.0, 3.0, 4.0, 5.0];
        let out = aroon(&highs, &lows, 4);
        assert_close(out.up[4], 100.0);
        assert_close(out.down[4], 0.0);
        assert_close(out.oscillator[4], 100.0);
    }

    #[test]
    fn ad_line_accumulates_close_location_volume() {
        let high = [10.0, 10.0];
        let low = [0.0, 0.0];
        let close = [10.0, 0.0];
        let volume = [2.0, 3.0];
        let out = ad(&high, &low, &close, &volume);
        assert_close(out[0], 2.0);
        assert_close(out[1], -1.0);
    }

    #[test]
    fn adx_family_exposes_di_dx_and_adx() {
        let highs = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0];
        let lows = [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0];
        let closes = [9.5, 10.5, 11.5, 12.5, 13.5, 14.5, 15.5, 16.5, 17.5, 18.5];
        let out = adx_family(&highs, &lows, &closes, 3);
        assert!(out.plus_di.iter().any(Option::is_some));
        assert!(out.dx.iter().any(Option::is_some));
        assert!(out.adx.iter().any(Option::is_some));
        assert!(out.adxr.iter().any(Option::is_some));
    }
}
