




use super::{Candle, safe_div, sma, tv_features::Side};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchoredVwapPoint {
    pub anchor_index: usize,
    pub vwap: f64,
    pub sigma: f64,
    pub upper_band: f64,
    pub lower_band: f64,
    pub distance_pct: f64,
    pub signal: Option<Side>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WyckoffPhase {
    Accumulation,
    Markup,
    Distribution,
    Markdown,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WyckoffPhasePoint {
    pub range_high: f64,
    pub range_low: f64,
    pub range_position: f64,
    pub volume_ratio: f64,
    pub trend_slope_pct: f64,
    pub phase: WyckoffPhase,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DarvasTurtleBreakoutPoint {
    pub box_high: f64,
    pub box_low: f64,
    pub breakout: Option<Side>,
    pub stop: f64,
    pub risk_pct: f64,
    pub bars_since_breakout: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IchimokuCloudPoint {
    pub tenkan: f64,
    pub kijun: f64,
    pub senkou_a: f64,
    pub senkou_b: f64,
    pub cloud_top: f64,
    pub cloud_bottom: f64,
    pub cloud_thickness_pct: f64,
    pub price_state: Option<Side>,
    pub tk_cross: Option<Side>,
    pub kumo_twist: Option<Side>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeikinAshiPoint {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub color: Option<Side>,
    pub body_pct: f64,
    pub upper_wick_pct: f64,
    pub lower_wick_pct: f64,
}

pub fn anchored_vwap(
    candles: &[Candle],
    anchors: &[bool],
    band_mult: f64,
) -> Vec<Option<AnchoredVwapPoint>> {
    assert!(
        candles.len() == anchors.len(),
        "anchors must align with candles"
    );
    assert!(band_mult > 0.0, "band_mult must be positive");

    let mut out = Vec::with_capacity(candles.len());
    let mut anchor_index: Option<usize> = None;
    let mut cumulative_volume = 0.0;
    let mut cumulative_pv = 0.0;
    let mut cumulative_p2v = 0.0;
    let mut prev_close = f64::NAN;
    let mut prev_vwap = f64::NAN;

    for (idx, candle) in candles.iter().enumerate() {
        if anchors[idx] || anchor_index.is_none() {
            anchor_index = Some(idx);
            cumulative_volume = 0.0;
            cumulative_pv = 0.0;
            cumulative_p2v = 0.0;
            prev_vwap = f64::NAN;
        }

        let source = candle.hlc3();
        cumulative_volume += candle.volume;
        cumulative_pv += source * candle.volume;
        cumulative_p2v += source * source * candle.volume;
        if cumulative_volume <= 0.0 {
            out.push(None);
            prev_close = candle.close;
            continue;
        }

        let vwap = cumulative_pv / cumulative_volume;
        let variance = (cumulative_p2v / cumulative_volume - vwap * vwap).max(0.0);
        let sigma = variance.sqrt();
        let upper_band = vwap + band_mult * sigma;
        let lower_band = vwap - band_mult * sigma;
        let signal = if prev_vwap.is_finite() && prev_close.is_finite() {
            if prev_close <= prev_vwap && candle.close > vwap {
                Some(Side::Bullish)
            } else if prev_close >= prev_vwap && candle.close < vwap {
                Some(Side::Bearish)
            } else {
                None
            }
        } else {
            None
        };
        out.push(Some(AnchoredVwapPoint {
            anchor_index: anchor_index.unwrap_or(idx),
            vwap,
            sigma,
            upper_band,
            lower_band,
            distance_pct: safe_div(candle.close - vwap, vwap) * 100.0,
            signal,
        }));
        prev_close = candle.close;
        prev_vwap = vwap;
    }
    out
}

pub fn wyckoff_phase(
    candles: &[Candle],
    lookback: usize,
    volume_period: usize,
) -> Vec<Option<WyckoffPhasePoint>> {
    assert!(lookback > 1, "lookback must be greater than 1");
    assert!(volume_period > 0, "volume_period must be positive");

    let volumes = candles.iter().map(|c| c.volume).collect::<Vec<_>>();
    let avg_volume = sma(&volumes, volume_period);

    candles
        .iter()
        .enumerate()
        .map(|(idx, candle)| {
            if idx == 0 {
                return None;
            }
            let range_start = idx.saturating_sub(lookback);
            let high = candles[range_start..idx]
                .iter()
                .map(|c| c.high)
                .fold(f64::NEG_INFINITY, f64::max);
            let low = candles[range_start..idx]
                .iter()
                .map(|c| c.low)
                .fold(f64::INFINITY, f64::min);
            let range = high - low;
            if range <= 0.0 {
                return None;
            }
            let range_position = safe_div(candle.close - low, range).clamp(0.0, 1.0);
            let volume_ratio = safe_div(candle.volume, avg_volume[idx]);
            let trend_slope_pct = if idx >= lookback {
                safe_div(
                    candle.close - candles[idx - lookback].close,
                    candles[idx - lookback].close,
                ) * 100.0
            } else {
                f64::NAN
            };
            let phase = if candle.close > high && trend_slope_pct > 0.0 {
                WyckoffPhase::Markup
            } else if candle.close < low && trend_slope_pct < 0.0 {
                WyckoffPhase::Markdown
            } else if range_position <= 0.35 && volume_ratio >= 1.1 && trend_slope_pct >= -2.0 {
                WyckoffPhase::Accumulation
            } else if range_position >= 0.65 && volume_ratio >= 1.1 && trend_slope_pct <= 2.0 {
                WyckoffPhase::Distribution
            } else {
                WyckoffPhase::Neutral
            };
            Some(WyckoffPhasePoint {
                range_high: high,
                range_low: low,
                range_position,
                volume_ratio,
                trend_slope_pct,
                phase,
            })
        })
        .collect()
}

pub fn darvas_turtle_breakout(
    candles: &[Candle],
    lookback: usize,
) -> Vec<Option<DarvasTurtleBreakoutPoint>> {
    assert!(lookback > 1, "lookback must be greater than 1");

    let mut active_side = None;
    let mut active_age: Option<usize> = None;

    candles
        .iter()
        .enumerate()
        .map(|(idx, candle)| {
            if idx == 0 {
                return None;
            }
            let range_start = idx.saturating_sub(lookback);
            let box_high = candles[range_start..idx]
                .iter()
                .map(|c| c.high)
                .fold(f64::NEG_INFINITY, f64::max);
            let box_low = candles[range_start..idx]
                .iter()
                .map(|c| c.low)
                .fold(f64::INFINITY, f64::min);
            let breakout = if candle.close > box_high {
                Some(Side::Bullish)
            } else if candle.close < box_low {
                Some(Side::Bearish)
            } else {
                None
            };
            if let Some(side) = breakout {
                active_side = Some(side);
                active_age = Some(0);
            } else if active_side.is_some() {
                active_age = active_age.map(|age| age + 1);
            }
            let stop = match breakout.or(active_side) {
                Some(Side::Bullish) => box_low,
                Some(Side::Bearish) => box_high,
                None => f64::NAN,
            };
            let risk_pct = match breakout.or(active_side) {
                Some(Side::Bullish) => safe_div(candle.close - stop, candle.close) * 100.0,
                Some(Side::Bearish) => safe_div(stop - candle.close, candle.close) * 100.0,
                None => f64::NAN,
            };
            Some(DarvasTurtleBreakoutPoint {
                box_high,
                box_low,
                breakout,
                stop,
                risk_pct,
                bars_since_breakout: active_age,
            })
        })
        .collect()
}

pub fn ichimoku_cloud_state(
    candles: &[Candle],
    tenkan_period: usize,
    kijun_period: usize,
    senkou_b_period: usize,
) -> Vec<Option<IchimokuCloudPoint>> {
    assert!(tenkan_period > 1, "tenkan_period must be greater than 1");
    assert!(kijun_period > 1, "kijun_period must be greater than 1");
    assert!(
        senkou_b_period > kijun_period,
        "senkou_b_period should exceed kijun_period"
    );

    let partial_midpoint = |idx: usize, period: usize| {
        let start = (idx + 1).saturating_sub(period);
        let high = candles[start..=idx]
            .iter()
            .map(|c| c.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let low = candles[start..=idx]
            .iter()
            .map(|c| c.low)
            .fold(f64::INFINITY, f64::min);
        midpoint(high, low)
    };

    (0..candles.len())
        .map(|idx| {
            let tenkan = partial_midpoint(idx, tenkan_period);
            let kijun = partial_midpoint(idx, kijun_period);
            let senkou_a = (tenkan + kijun) * 0.5;
            let senkou_b = partial_midpoint(idx, senkou_b_period);
            let cloud_top = senkou_a.max(senkou_b);
            let cloud_bottom = senkou_a.min(senkou_b);
            let prev = idx.checked_sub(1).map(|prev| {
                let prev_tenkan = partial_midpoint(prev, tenkan_period);
                let prev_kijun = partial_midpoint(prev, kijun_period);
                let prev_senkou_a = (prev_tenkan + prev_kijun) * 0.5;
                let prev_senkou_b = partial_midpoint(prev, senkou_b_period);
                (prev_tenkan, prev_kijun, prev_senkou_a, prev_senkou_b)
            });
            let tk_cross = match prev {
                Some((prev_tenkan, prev_kijun, _, _))
                    if prev_tenkan <= prev_kijun && tenkan > kijun =>
                {
                    Some(Side::Bullish)
                }
                Some((prev_tenkan, prev_kijun, _, _))
                    if prev_tenkan >= prev_kijun && tenkan < kijun =>
                {
                    Some(Side::Bearish)
                }
                _ => None,
            };
            let kumo_twist = match prev {
                Some((_, _, prev_a, prev_b)) if prev_a <= prev_b && senkou_a > senkou_b => {
                    Some(Side::Bullish)
                }
                Some((_, _, prev_a, prev_b)) if prev_a >= prev_b && senkou_a < senkou_b => {
                    Some(Side::Bearish)
                }
                _ => None,
            };
            let price_state = if candles[idx].close > cloud_top {
                Some(Side::Bullish)
            } else if candles[idx].close < cloud_bottom {
                Some(Side::Bearish)
            } else {
                None
            };
            Some(IchimokuCloudPoint {
                tenkan,
                kijun,
                senkou_a,
                senkou_b,
                cloud_top,
                cloud_bottom,
                cloud_thickness_pct: safe_div(cloud_top - cloud_bottom, candles[idx].close) * 100.0,
                price_state,
                tk_cross,
                kumo_twist,
            })
        })
        .collect()
}

pub fn heikin_ashi_transform(candles: &[Candle]) -> Vec<Option<HeikinAshiPoint>> {
    let mut out = Vec::with_capacity(candles.len());
    let mut prev_open = f64::NAN;
    let mut prev_close = f64::NAN;
    for candle in candles {
        let close = (candle.open + candle.high + candle.low + candle.close) * 0.25;
        let open = if prev_open.is_nan() {
            (candle.open + candle.close) * 0.5
        } else {
            (prev_open + prev_close) * 0.5
        };
        let high = candle.high.max(open).max(close);
        let low = candle.low.min(open).min(close);
        let range = high - low;
        let color = if close > open {
            Some(Side::Bullish)
        } else if close < open {
            Some(Side::Bearish)
        } else {
            None
        };
        out.push(Some(HeikinAshiPoint {
            open,
            high,
            low,
            close,
            color,
            body_pct: safe_div((close - open).abs(), range) * 100.0,
            upper_wick_pct: safe_div(high - open.max(close), range) * 100.0,
            lower_wick_pct: safe_div(open.min(close) - low, range) * 100.0,
        }));
        prev_open = open;
        prev_close = close;
    }
    out
}

fn midpoint(high: f64, low: f64) -> f64 {
    (high + low) * 0.5
}
