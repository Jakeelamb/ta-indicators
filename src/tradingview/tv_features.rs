




use super::{
    Candle, close_series, ema, rolling_max, rolling_min, rolling_stddev, safe_div, sma, wilders_atr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bullish,
    Bearish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotKind {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfirmedPivot {
    pub pivot_index: usize,
    pub confirmed_index: usize,
    pub kind: PivotKind,
    pub price: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquiditySweep {
    pub side: Side,
    pub swept_pivot_index: usize,
    pub level: f64,
    pub score: f64,
    pub wick_atr: f64,
    pub reclaim_atr: f64,
    pub volume_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeLiquiditySweepSignal {
    pub side: Side,
    pub swept_pivot_index: usize,
    pub level: f64,
    pub volume: f64,
    pub average_volume: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendRibbonPoint {
    pub fast: f64,
    pub slow: f64,
    pub spread_pct: f64,
    pub volume_ratio: f64,
    pub side: Option<Side>,
    pub upper: f64,
    pub lower: f64,
    pub trend_age: usize,
    pub target_price: Option<f64>,
    pub is_volume_spike: bool,
    pub bull_cross: bool,
    pub bear_cross: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MomentumVolComposite {
    pub momentum_z: f64,
    pub volatility_z: f64,
    pub volume_z: f64,
    pub score: f64,
    pub ema_diff_z: f64,
    pub composite_ma: f64,
    pub slope: f64,
    pub slope_prev: f64,
    pub signal: Option<Side>,
    pub dominant_factor: MomentumVolFactor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumVolFactor {
    Roc,
    Atr,
    VolumeFlow,
    EmaDiff,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveBaselinePoint {
    pub period: usize,
    pub value: f64,
    pub slope_pct: f64,
    pub side: Option<Side>,
    pub scores: [i8; 5],
    pub baselines: [f64; 5],
    pub upper_bands: [f64; 5],
    pub lower_bands: [f64; 5],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfStrengthPoint {
    pub baseline: f64,
    pub strength: f64,
    pub signal: f64,
    pub histogram: f64,
    pub new_strength_high: bool,
    pub cross_up: bool,
    pub cross_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MariashiRenkoPoint {
    pub virtual_open: f64,
    pub virtual_close: f64,
    pub brick_size: f64,
    pub signal: Option<Side>,
    pub trailing_stop: Option<f64>,
    pub consecutive_bricks: usize,
}

fn confirmed_pivots(candles: &[Candle], left: usize, right: usize) -> Vec<Vec<ConfirmedPivot>> {
    assert!(left > 0, "left must be positive");
    assert!(right > 0, "right must be positive");
    let mut out = vec![Vec::new(); candles.len()];
    if candles.len() < left + right + 1 {
        return out;
    }

    for center in left..candles.len() - right {
        let high = candles[center].high;
        let low = candles[center].low;
        let start = center - left;
        let end = center + right;
        let is_high = (start..=end).all(|idx| idx == center || high > candles[idx].high);
        let is_low = (start..=end).all(|idx| idx == center || low < candles[idx].low);
        let confirmed_index = center + right;
        if is_high {
            out[confirmed_index].push(ConfirmedPivot {
                pivot_index: center,
                confirmed_index,
                kind: PivotKind::High,
                price: high,
            });
        }
        if is_low {
            out[confirmed_index].push(ConfirmedPivot {
                pivot_index: center,
                confirmed_index,
                kind: PivotKind::Low,
                price: low,
            });
        }
    }
    out
}

pub fn volume_liquidity_sweep(candles: &[Candle]) -> Vec<Option<LiquiditySweep>> {
    let atr = wilders_atr(candles, 14);
    xwisetrade_volume_liquidity_sweep(candles, 5, 20, 1.5, 3)
        .into_iter()
        .enumerate()
        .map(|(idx, signals)| {
            signals.into_iter().next().map(|signal| {
                let atrv = atr[idx];
                let wick_atr = if atrv.is_finite() && atrv > 0.0 {
                    match signal.side {
                        Side::Bullish => {
                            (candles[idx].open.min(candles[idx].close) - candles[idx].low) / atrv
                        }
                        Side::Bearish => {
                            (candles[idx].high - candles[idx].open.max(candles[idx].close)) / atrv
                        }
                    }
                } else {
                    f64::NAN
                };
                let reclaim_atr = if atrv.is_finite() && atrv > 0.0 {
                    match signal.side {
                        Side::Bullish => (candles[idx].close - signal.level) / atrv,
                        Side::Bearish => (signal.level - candles[idx].close) / atrv,
                    }
                } else {
                    f64::NAN
                };
                LiquiditySweep {
                    side: signal.side,
                    swept_pivot_index: signal.swept_pivot_index,
                    level: signal.level,
                    score: 100.0,
                    wick_atr,
                    reclaim_atr,
                    volume_ratio: safe_div(signal.volume, signal.average_volume),
                }
            })
        })
        .collect()
}

pub fn xwisetrade_volume_liquidity_sweep(
    candles: &[Candle],
    pivot_len: usize,
    volume_len: usize,
    volume_multiplier: f64,
    cooldown: usize,
) -> Vec<Vec<VolumeLiquiditySweepSignal>> {
    assert!(pivot_len > 0, "pivot_len must be positive");
    assert!(volume_len > 0, "volume_len must be positive");
    assert!(
        volume_multiplier >= 1.0,
        "volume_multiplier must be at least one"
    );

    let pivots = confirmed_pivots(candles, pivot_len, pivot_len);
    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
    let average_volume = sma(&volumes, volume_len);
    let mut last_high: Option<ConfirmedPivot> = None;
    let mut last_low: Option<ConfirmedPivot> = None;
    let mut last_bull: Option<usize> = None;
    let mut last_bear: Option<usize> = None;
    let mut out = vec![Vec::new(); candles.len()];

    for idx in 0..candles.len() {
        for pivot in &pivots[idx] {
            match pivot.kind {
                PivotKind::High => last_high = Some(*pivot),
                PivotKind::Low => last_low = Some(*pivot),
            }
        }

        let avg_vol = average_volume[idx];
        if avg_vol.is_nan() || candles[idx].volume <= avg_vol * volume_multiplier {
            continue;
        }

        let bull = last_low.and_then(|pivot| {
            let swept = candles[idx].low < pivot.price && candles[idx].close > pivot.price;
            let cooled = last_bull.is_none_or(|last| idx - last > cooldown);
            (swept && cooled).then_some((pivot, Side::Bullish))
        });
        let bear = last_high.and_then(|pivot| {
            let swept = candles[idx].high > pivot.price && candles[idx].close < pivot.price;
            let cooled = last_bear.is_none_or(|last| idx - last > cooldown);
            (swept && cooled).then_some((pivot, Side::Bearish))
        });

        for (pivot, side) in [bull, bear].into_iter().flatten() {
            match side {
                Side::Bullish => last_bull = Some(idx),
                Side::Bearish => last_bear = Some(idx),
            }
            out[idx].push(VolumeLiquiditySweepSignal {
                side,
                swept_pivot_index: pivot.pivot_index,
                level: pivot.price,
                volume: candles[idx].volume,
                average_volume: avg_vol,
            });
        }
    }

    out
}

pub fn volumetric_trend_ribbon(candles: &[Candle]) -> Vec<Option<TrendRibbonPoint>> {
    volumetric_trend_ribbon_with_params(candles, 30, 5, 2.0, 1.5)
}

fn volumetric_trend_ribbon_with_params(
    candles: &[Candle],
    length: usize,
    smoothing: usize,
    width_multiplier: f64,
    target_multiplier: f64,
) -> Vec<Option<TrendRibbonPoint>> {
    assert!(length > 0, "length must be positive");
    assert!(smoothing > 0, "smoothing must be positive");
    let closes = close_series(candles);
    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
    let base_trend = vwma(&closes, &volumes, length);
    let smoothed_base = ema(&base_trend, smoothing);
    let deviations: Vec<f64> = closes
        .iter()
        .zip(&smoothed_base)
        .map(|(close, base)| (close - base).powi(2))
        .collect();
    let vwsd = vwma(&deviations, &volumes, length)
        .into_iter()
        .map(f64::sqrt)
        .collect::<Vec<_>>();
    let volume_ma = sma(&volumes, length * 2);
    let mut out = vec![None; candles.len()];
    let mut trend_age = 0usize;
    let mut previous_side = None;
    let mut target_price = None;

    for idx in 1..candles.len() {
        let base = smoothed_base[idx];
        let prev_base = smoothed_base[idx - 1];
        let volume_ratio = safe_div(candles[idx].volume, volume_ma[idx]);
        if base.is_nan() {
            continue;
        }

        let deviation = vwsd[idx];
        let has_bands = deviation.is_finite();
        let upper = if has_bands {
            base + deviation * width_multiplier
        } else {
            f64::NAN
        };
        let lower = if has_bands {
            base - deviation * width_multiplier
        } else {
            f64::NAN
        };
        let is_up = prev_base.is_finite() && base > prev_base && candles[idx].close > base;
        let is_down = prev_base.is_finite() && base < prev_base && candles[idx].close < base;
        let side = if is_up {
            Some(Side::Bullish)
        } else if is_down {
            Some(Side::Bearish)
        } else {
            None
        };
        trend_age = if side.is_some() && side == previous_side {
            trend_age + 1
        } else {
            0
        };
        previous_side = side;

        let has_previous_bands = vwsd[idx - 1].is_finite();
        let prev_upper = if has_previous_bands {
            smoothed_base[idx - 1] + vwsd[idx - 1] * width_multiplier
        } else {
            f64::NAN
        };
        let prev_lower = if has_previous_bands {
            smoothed_base[idx - 1] - vwsd[idx - 1] * width_multiplier
        } else {
            f64::NAN
        };
        let bull_cross = has_bands
            && has_previous_bands
            && candles[idx - 1].close <= prev_upper
            && candles[idx].close > upper
            && is_up;
        let bear_cross = has_bands
            && has_previous_bands
            && candles[idx - 1].close >= prev_lower
            && candles[idx].close < lower
            && is_down;
        let ribbon_width = upper - lower;
        if bull_cross {
            target_price = Some(upper + ribbon_width * target_multiplier);
        } else if bear_cross {
            target_price = Some(lower - ribbon_width * target_multiplier);
        } else if side.is_none()
            || target_price.is_some_and(|target| {
                (side == Some(Side::Bullish) && candles[idx].high > target)
                    || (side == Some(Side::Bearish) && candles[idx].low < target)
            })
        {
            target_price = None;
        }

        out[idx] = Some(TrendRibbonPoint {
            fast: base,
            slow: deviation,
            spread_pct: safe_div(ribbon_width, base) * 100.0,
            volume_ratio,
            side,
            upper,
            lower,
            trend_age,
            target_price,
            is_volume_spike: volume_ratio > 1.5,
            bull_cross,
            bear_cross,
        });
    }

    out
}

pub fn momentum_vol_composite(candles: &[Candle]) -> Vec<Option<MomentumVolComposite>> {
    momentum_vol_composite_with_params(
        candles,
        MomentumVolCompositeParams {
            roc_len: 14,
            atr_len: 14,
            volume_flow_len: 20,
            short_ema_len: 12,
            long_ema_len: 26,
            composite_ma_len: 14,
            weights: [0.25, 0.25, 0.25, 0.25],
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MomentumVolCompositeParams {
    pub roc_len: usize,
    pub atr_len: usize,
    pub volume_flow_len: usize,
    pub short_ema_len: usize,
    pub long_ema_len: usize,
    pub composite_ma_len: usize,
    pub weights: [f64; 4],
}

fn momentum_vol_composite_with_params(
    candles: &[Candle],
    params: MomentumVolCompositeParams,
) -> Vec<Option<MomentumVolComposite>> {
    assert!(params.roc_len >= 2, "roc_len must be at least two");
    assert!(params.atr_len >= 2, "atr_len must be at least two");
    assert!(
        params.volume_flow_len >= 2,
        "volume_flow_len must be at least two"
    );
    assert!(
        params.short_ema_len >= 2 && params.long_ema_len >= 2,
        "EMA lengths must be at least two"
    );
    assert!(
        params.composite_ma_len > 0,
        "composite_ma_len must be positive"
    );

    let closes = close_series(candles);
    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
    let roc = roc(&closes, params.roc_len);
    let roc_z = pine_safe_zscore(&roc, params.roc_len);

    let atr = wilders_atr(candles, params.atr_len);
    let atr_z = pine_safe_zscore(&atr, params.atr_len);

    let mut cumulative_volume = Vec::with_capacity(candles.len());
    let mut cumulative = 0.0;
    for volume in &volumes {
        cumulative += *volume;
        cumulative_volume.push(cumulative);
    }
    let volume_flow_ema = ema(&cumulative_volume, params.volume_flow_len);
    let volume_flow = cumulative_volume
        .iter()
        .zip(&volume_flow_ema)
        .map(|(cum, ema)| cum - ema)
        .collect::<Vec<_>>();
    let volume_flow_z = pine_safe_zscore(&volume_flow, params.volume_flow_len);

    let short_ema = ema(&closes, params.short_ema_len);
    let long_ema = ema(&closes, params.long_ema_len);
    let ema_diff = short_ema
        .iter()
        .zip(&long_ema)
        .map(|(short, long)| short - long)
        .collect::<Vec<_>>();
    let ema_diff_z = pine_safe_zscore(&ema_diff, params.long_ema_len);

    let weight_sum = params.weights.iter().sum::<f64>().max(1e-10);
    let composite = (0..candles.len())
        .map(|idx| {
            params.weights[0] / weight_sum * roc_z[idx]
                + params.weights[1] / weight_sum * atr_z[idx]
                + params.weights[2] / weight_sum * volume_flow_z[idx]
                + params.weights[3] / weight_sum * ema_diff_z[idx]
        })
        .collect::<Vec<_>>();
    let composite_ma = ema(&composite, params.composite_ma_len);
    let shifted1 = shifted_with_current_fallback(&composite, 1);
    let shifted2 = shifted_with_current_fallback(&composite, 2);
    let composite_ma_prev1 = ema(&shifted1, params.composite_ma_len);
    let composite_ma_prev2 = ema(&shifted2, params.composite_ma_len);

    (0..candles.len())
        .map(|idx| {
            if composite_ma[idx].is_nan()
                || composite_ma_prev1[idx].is_nan()
                || composite_ma_prev2[idx].is_nan()
            {
                None
            } else {
                let slope = composite_ma[idx] - composite_ma_prev1[idx];
                let slope_prev = composite_ma_prev1[idx] - composite_ma_prev2[idx];
                let values = [
                    (MomentumVolFactor::Roc, roc_z[idx]),
                    (MomentumVolFactor::Atr, atr_z[idx]),
                    (MomentumVolFactor::VolumeFlow, volume_flow_z[idx]),
                    (MomentumVolFactor::EmaDiff, ema_diff_z[idx]),
                ];
                let dominant_factor = values
                    .into_iter()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .map(|(factor, _)| factor)
                    .unwrap_or(MomentumVolFactor::Roc);
                Some(MomentumVolComposite {
                    momentum_z: roc_z[idx],
                    volatility_z: atr_z[idx],
                    volume_z: volume_flow_z[idx],
                    score: composite[idx],
                    ema_diff_z: ema_diff_z[idx],
                    composite_ma: composite_ma[idx],
                    slope,
                    slope_prev,
                    signal: if slope > 0.0 && slope_prev <= 0.0 {
                        Some(Side::Bullish)
                    } else if slope < 0.0 && slope_prev >= 0.0 {
                        Some(Side::Bearish)
                    } else {
                        None
                    },
                    dominant_factor,
                })
            }
        })
        .collect()
}

pub fn adaptive_baseline(candles: &[Candle]) -> Vec<Option<AdaptiveBaselinePoint>> {
    adaptive_baseline_suite(
        candles,
        AdaptiveBaselineParams {
            kijun_len: 35,
            dema_len: 40,
            median_len: 35,
            laguerre_alpha: 0.7,
            gaussian_sigma: 5.0,
            gaussian_len: 12,
            volatility_len: 21,
            volatility_smoothing: 15,
            multiplier: 1.2,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AdaptiveBaselineParams {
    pub kijun_len: usize,
    pub dema_len: usize,
    pub median_len: usize,
    pub laguerre_alpha: f64,
    pub gaussian_sigma: f64,
    pub gaussian_len: usize,
    pub volatility_len: usize,
    pub volatility_smoothing: usize,
    pub multiplier: f64,
}

fn adaptive_baseline_suite(
    candles: &[Candle],
    params: AdaptiveBaselineParams,
) -> Vec<Option<AdaptiveBaselinePoint>> {
    assert!(params.kijun_len > 0, "kijun_len must be positive");
    assert!(params.dema_len > 0, "dema_len must be positive");
    assert!(params.median_len > 0, "median_len must be positive");
    assert!(params.gaussian_len > 0, "gaussian_len must be positive");
    assert!(params.volatility_len > 0, "volatility_len must be positive");

    let closes = close_series(candles);
    let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let kijun_high = rolling_max(&highs, params.kijun_len);
    let kijun_low = rolling_min(&lows, params.kijun_len);
    let kijun = kijun_high
        .iter()
        .zip(&kijun_low)
        .map(|(high, low)| (high + low) * 0.5)
        .collect::<Vec<_>>();
    let dema = dema(&closes, params.dema_len);
    let median = rolling_median(&closes, params.median_len);
    let laguerre = laguerre_filter(&closes, params.laguerre_alpha);
    let gaussian = gaussian_filter(&closes, params.gaussian_len, params.gaussian_sigma);
    let baselines = [kijun, dema, median, laguerre, gaussian];
    let bands = baselines
        .iter()
        .map(|baseline| {
            volatility_bands(
                &closes,
                baseline,
                params.volatility_len,
                params.volatility_smoothing,
                params.multiplier,
            )
        })
        .collect::<Vec<_>>();
    let mut scores = [0i8; 5];
    let mut out: Vec<Option<AdaptiveBaselinePoint>> = vec![None; candles.len()];

    for idx in 0..candles.len() {
        let mut baseline_values = [f64::NAN; 5];
        let mut upper_bands = [f64::NAN; 5];
        let mut lower_bands = [f64::NAN; 5];
        let mut any_ready = false;
        for baseline_idx in 0..5 {
            baseline_values[baseline_idx] = baselines[baseline_idx][idx];
            upper_bands[baseline_idx] = bands[baseline_idx].0[idx];
            lower_bands[baseline_idx] = bands[baseline_idx].1[idx];
            if upper_bands[baseline_idx].is_finite() && lower_bands[baseline_idx].is_finite() {
                any_ready = true;
                if closes[idx] > upper_bands[baseline_idx] {
                    scores[baseline_idx] = 1;
                } else if closes[idx] < lower_bands[baseline_idx] {
                    scores[baseline_idx] = -1;
                }
            }
        }
        if any_ready {
            let tpi_score = scores.iter().map(|score| *score as f64).sum::<f64>() / 5.0;
            out[idx] = Some(AdaptiveBaselinePoint {
                period: 5,
                value: tpi_score,
                slope_pct: if idx > 0 {
                    if let Some(previous) = out[idx - 1] {
                        tpi_score - previous.value
                    } else {
                        0.0
                    }
                } else {
                    0.0
                },
                side: if tpi_score > 0.0 {
                    Some(Side::Bullish)
                } else if tpi_score < 0.0 {
                    Some(Side::Bearish)
                } else {
                    None
                },
                scores,
                baselines: baseline_values,
                upper_bands,
                lower_bands,
            });
        }
    }

    out
}

pub fn self_strength_oscillator(candles: &[Candle]) -> Vec<Option<SelfStrengthPoint>> {
    self_strength_oscillator_with_params(candles, 50, 9, 20)
}

fn self_strength_oscillator_with_params(
    candles: &[Candle],
    ma_len: usize,
    signal_len: usize,
    lead_lookback: usize,
) -> Vec<Option<SelfStrengthPoint>> {
    assert!(ma_len > 1, "ma_len must be greater than one");
    assert!(signal_len > 0, "signal_len must be positive");
    assert!(lead_lookback > 0, "lead_lookback must be positive");

    let closes = close_series(candles);
    let baseline = ema(&closes, ma_len);
    let strength_series = closes
        .iter()
        .zip(&baseline)
        .map(|(close, baseline)| {
            if baseline.is_nan() || *baseline == 0.0 {
                return f64::NAN;
            }
            (close - baseline) / baseline * 100.0
        })
        .collect::<Vec<_>>();
    let signal = ema(&strength_series, signal_len);
    let prior_strength = shifted_with_current_fallback(&strength_series, 1);
    let prior_high = rolling_max(&prior_strength, lead_lookback);

    strength_series
        .iter()
        .zip(signal.iter())
        .enumerate()
        .map(|(idx, (strength, signal))| {
            if strength.is_nan() || signal.is_nan() || baseline[idx].is_nan() {
                return None;
            }
            let prev = if idx > 0 {
                strength_series[idx - 1]
            } else {
                f64::NAN
            };
            Some(SelfStrengthPoint {
                baseline: baseline[idx],
                strength: *strength,
                signal: *signal,
                histogram: *strength - *signal,
                new_strength_high: *strength > 0.0
                    && prior_high[idx].is_finite()
                    && *strength > prior_high[idx],
                cross_up: prev <= 0.0 && *strength > 0.0,
                cross_down: prev >= 0.0 && *strength < 0.0,
            })
        })
        .collect()
}

pub fn mariashi_renko_system(candles: &[Candle]) -> Vec<Option<MariashiRenkoPoint>> {
    mariashi_renko_system_with_params(candles, 14, 2.0)
}

fn mariashi_renko_system_with_params(
    candles: &[Candle],
    atr_len: usize,
    brick_weight: f64,
) -> Vec<Option<MariashiRenkoPoint>> {
    assert!(atr_len > 0, "atr_len must be positive");
    assert!(brick_weight > 0.0, "brick_weight must be positive");
    let atr = wilders_atr(candles, atr_len);
    let mut out = vec![None; candles.len()];
    let Some(first) = candles.first() else {
        return out;
    };
    let mut virtual_open = first.open;
    let mut virtual_close = first.close;
    let mut last_signal: Option<Side> = None;
    let mut trailing_stop: Option<f64> = None;
    let mut consecutive_bricks = 0usize;

    for (idx, candle) in candles.iter().enumerate() {
        let brick_size = atr[idx] * brick_weight;
        if brick_size.is_nan() || brick_size <= 0.0 {
            continue;
        }
        let previous_virtual_close = virtual_close;
        let green_brick = candle.close > previous_virtual_close + brick_size;
        let red_brick = candle.close < previous_virtual_close - brick_size;
        let mut signal = None;

        if green_brick {
            virtual_open = previous_virtual_close;
            virtual_close = virtual_open + brick_size;
            consecutive_bricks = if last_signal == Some(Side::Bullish) {
                consecutive_bricks + 1
            } else {
                1
            };
        } else if red_brick {
            virtual_open = previous_virtual_close;
            virtual_close = virtual_open - brick_size;
            consecutive_bricks = if last_signal == Some(Side::Bearish) {
                consecutive_bricks + 1
            } else {
                1
            };
        }

        let tolerance_mult = if consecutive_bricks >= 27 {
            3.0
        } else if consecutive_bricks >= 10 {
            2.0
        } else if consecutive_bricks >= 4 {
            1.5
        } else {
            1.0
        };
        if last_signal == Some(Side::Bullish) {
            let new_stop = candle.close - brick_size * tolerance_mult;
            trailing_stop = Some(trailing_stop.map_or(new_stop, |stop| stop.max(new_stop)));
        } else if last_signal == Some(Side::Bearish) {
            let new_stop = candle.close + brick_size * tolerance_mult;
            trailing_stop = Some(trailing_stop.map_or(new_stop, |stop| stop.min(new_stop)));
        }

        let stop_buy_triggered = last_signal == Some(Side::Bullish)
            && trailing_stop.is_some_and(|stop| candle.close <= stop);
        let stop_sell_triggered = last_signal == Some(Side::Bearish)
            && trailing_stop.is_some_and(|stop| candle.close >= stop);

        if green_brick && (last_signal != Some(Side::Bullish) || stop_sell_triggered) {
            last_signal = Some(Side::Bullish);
            trailing_stop = Some(candle.close - brick_size);
            consecutive_bricks = 1;
            signal = Some(Side::Bullish);
        } else if red_brick && (last_signal != Some(Side::Bearish) || stop_buy_triggered) {
            last_signal = Some(Side::Bearish);
            trailing_stop = Some(candle.close + brick_size);
            consecutive_bricks = 1;
            signal = Some(Side::Bearish);
        }

        out[idx] = Some(MariashiRenkoPoint {
            virtual_open,
            virtual_close,
            brick_size,
            signal,
            trailing_stop,
            consecutive_bricks,
        });
    }

    out
}

fn vwma(values: &[f64], volumes: &[f64], period: usize) -> Vec<f64> {
    assert_eq!(values.len(), volumes.len(), "values and volumes must align");
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for (idx, output) in out.iter_mut().enumerate().skip(period.saturating_sub(1)) {
        let start = idx + 1 - period;
        let mut weighted_sum = 0.0;
        let mut volume_sum = 0.0;
        let mut valid = true;
        for offset in start..=idx {
            if values[offset].is_nan() || volumes[offset].is_nan() {
                valid = false;
                break;
            }
            let volume = volumes[offset].max(0.0);
            weighted_sum += values[offset] * volume;
            volume_sum += volume;
        }
        if valid && volume_sum > 0.0 {
            *output = weighted_sum / volume_sum;
        }
    }
    out
}

fn roc(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for idx in period..values.len() {
        out[idx] = safe_div(values[idx] - values[idx - period], values[idx - period]) * 100.0;
    }
    out
}

fn pine_safe_zscore(values: &[f64], period: usize) -> Vec<f64> {
    let mean = sma(values, period);
    let sd = rolling_stddev(values, period);
    (0..values.len())
        .map(|idx| {
            if values[idx].is_nan() || mean[idx].is_nan() || sd[idx].is_nan() || sd[idx] <= 0.0 {
                0.0
            } else {
                (values[idx] - mean[idx]) / sd[idx]
            }
        })
        .collect()
}

fn shifted_with_current_fallback(values: &[f64], shift: usize) -> Vec<f64> {
    (0..values.len())
        .map(|idx| {
            if idx >= shift {
                values[idx - shift]
            } else {
                values[idx]
            }
        })
        .collect()
}

fn dema(values: &[f64], period: usize) -> Vec<f64> {
    let first = ema(values, period);
    let second = ema(&first, period);
    first
        .iter()
        .zip(&second)
        .map(|(a, b)| 2.0 * a - b)
        .collect()
}

fn rolling_median(values: &[f64], period: usize) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for idx in period.saturating_sub(1)..values.len() {
        let start = idx + 1 - period;
        let mut window = values[start..=idx].to_vec();
        if window.iter().any(|value| value.is_nan()) {
            continue;
        }
        window.sort_by(f64::total_cmp);
        let mid = period / 2;
        out[idx] = if period % 2 == 0 {
            (window[mid - 1] + window[mid]) * 0.5
        } else {
            window[mid]
        };
    }
    out
}

fn laguerre_filter(values: &[f64], alpha: f64) -> Vec<f64> {
    let mut out = vec![f64::NAN; values.len()];
    let mut l0 = 0.0;
    let mut l1 = 0.0;
    let mut l2 = 0.0;
    let mut l3 = 0.0;
    for (idx, value) in values.iter().enumerate() {
        if value.is_nan() {
            continue;
        }
        let prev_l0 = l0;
        let prev_l1 = l1;
        let prev_l2 = l2;
        l0 = alpha * value + (1.0 - alpha) * prev_l0;
        l1 = -(1.0 - alpha) * l0 + prev_l0 + (1.0 - alpha) * prev_l1;
        l2 = -(1.0 - alpha) * l1 + prev_l1 + (1.0 - alpha) * prev_l2;
        l3 = -(1.0 - alpha) * l2 + prev_l2 + (1.0 - alpha) * l3;
        out[idx] = (l0 + 2.0 * l1 + 2.0 * l2 + l3) / 6.0;
    }
    out
}

fn gaussian_filter(values: &[f64], period: usize, sigma: f64) -> Vec<f64> {
    assert!(period > 0, "period must be positive");
    let mut out = vec![f64::NAN; values.len()];
    for idx in period.saturating_sub(1)..values.len() {
        let mut total_weight = 0.0;
        let mut weighted_sum = 0.0;
        let mut valid = true;
        for offset in 0..period {
            let value = values[idx - offset];
            if value.is_nan() {
                valid = false;
                break;
            }
            let centered = (offset as f64 - (period - 1) as f64 / 2.0) / sigma;
            let weight = (-0.5 * centered.powi(2)).exp();
            total_weight += weight;
            weighted_sum += value * weight;
        }
        if valid && total_weight > 0.0 {
            out[idx] = weighted_sum / total_weight;
        }
    }
    out
}

fn volatility_bands(
    source: &[f64],
    baseline: &[f64],
    volatility_len: usize,
    volatility_smoothing: usize,
    multiplier: f64,
) -> (Vec<f64>, Vec<f64>) {
    let residual = source
        .iter()
        .zip(baseline)
        .map(|(source, baseline)| source - baseline)
        .collect::<Vec<_>>();
    let residual_mean = sma(&residual, volatility_len);
    let abs_dev = residual
        .iter()
        .zip(&residual_mean)
        .map(|(residual, mean)| (residual - mean).abs())
        .collect::<Vec<_>>();
    let mad = sma(&abs_dev, volatility_len)
        .into_iter()
        .map(|value| value * 1.4826)
        .collect::<Vec<_>>();
    let volatility = if volatility_smoothing > 1 {
        ema(&mad, volatility_smoothing)
    } else {
        mad
    };
    let upper = baseline
        .iter()
        .zip(&volatility)
        .map(|(baseline, volatility)| baseline + volatility * multiplier)
        .collect();
    let lower = baseline
        .iter()
        .zip(&volatility)
        .map(|(baseline, volatility)| baseline - volatility * multiplier)
        .collect();
    (upper, lower)
}
