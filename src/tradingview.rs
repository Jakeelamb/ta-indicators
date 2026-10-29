





mod expanded;
mod tv_features;
pub mod validated;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Candle {
    #[inline]
    pub fn new(open: f64, high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self {
            open,
            high,
            low,
            close,
            volume,
        }
    }

    #[inline]
    pub fn hl2(self) -> f64 {
        (self.high + self.low) * 0.5
    }

    #[inline]
    pub fn hlc3(self) -> f64 {
        (self.high + self.low + self.close) / 3.0
    }
}

pub(crate) fn close_series(candles: &[Candle]) -> Vec<f64> {
    candles.iter().map(|candle| candle.close).collect()
}

pub(crate) fn ema(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut seeded = false;
    let mut current = f64::NAN;
    let mut window_sum = 0.0;
    let mut window_count = 0usize;

    for (idx, value) in values.iter().enumerate() {
        if value.is_nan() {
            window_sum = 0.0;
            window_count = 0;
            seeded = false;
            current = f64::NAN;
            continue;
        }
        if !seeded {
            window_sum += value;
            window_count += 1;
            if window_count == period {
                current = window_sum / period as f64;
                out[idx] = current;
                seeded = true;
            }
            continue;
        }
        current = alpha * value + (1.0 - alpha) * current;
        out[idx] = current;
    }

    out
}

pub(crate) fn sma(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    let mut sum = 0.0;
    let mut finite_count = 0usize;

    for (idx, value) in values.iter().enumerate() {
        if value.is_finite() {
            sum += value;
            finite_count += 1;
        }
        if idx >= period {
            let dropped = values[idx - period];
            if dropped.is_finite() {
                sum -= dropped;
                finite_count -= 1;
            }
        }
        if idx + 1 >= period && finite_count == period {
            out[idx] = sum / period as f64;
        }
    }

    out
}

pub(crate) fn wilders_atr(candles: &[Candle], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let tr = true_ranges(candles);
    let mut out = vec![f64::NAN; candles.len()];
    let mut sum = 0.0;
    let mut current = f64::NAN;

    for (idx, value) in tr.iter().enumerate() {
        if idx < period {
            sum += value;
            if idx + 1 == period {
                current = sum / period as f64;
                out[idx] = current;
            }
            continue;
        }
        current = (current * (period - 1) as f64 + value) / period as f64;
        out[idx] = current;
    }

    out
}

pub(crate) fn true_ranges(candles: &[Candle]) -> Vec<f64> {
    let mut out = Vec::with_capacity(candles.len());
    for (idx, candle) in candles.iter().enumerate() {
        if idx == 0 {
            out.push(candle.high - candle.low);
            continue;
        }
        let prev_close = candles[idx - 1].close;
        out.push(
            (candle.high - candle.low)
                .max((candle.high - prev_close).abs())
                .max((candle.low - prev_close).abs()),
        );
    }
    out
}

pub(crate) fn rolling_stddev(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mean = sma(values, period);
    let mut out = vec![f64::NAN; values.len()];

    for idx in period.saturating_sub(1)..values.len() {
        if mean[idx].is_nan() {
            continue;
        }
        let start = idx + 1 - period;
        let mut sum_sq = 0.0;
        let mut valid = true;
        for value in &values[start..=idx] {
            if value.is_nan() {
                valid = false;
                break;
            }
            let diff = value - mean[idx];
            sum_sq += diff * diff;
        }
        if valid {
            out[idx] = (sum_sq / period as f64).sqrt();
        }
    }

    out
}

pub(crate) fn rolling_max(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for idx in period.saturating_sub(1)..values.len() {
        let start = idx + 1 - period;
        out[idx] = values[start..=idx]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
    }
    out
}

pub(crate) fn rolling_min(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for idx in period.saturating_sub(1)..values.len() {
        let start = idx + 1 - period;
        out[idx] = values[start..=idx]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
    }
    out
}

#[inline]
pub(crate) fn safe_div(numerator: f64, denominator: f64) -> f64 {
    if denominator.abs() <= f64::EPSILON {
        f64::NAN
    } else {
        numerator / denominator
    }
}
