//! Candlestick pattern recognition — warmup-exact ports of the TA-Lib `CDL*`
//! family.
//!
//! TA-Lib drives every pattern off a small set of global "candle settings",
//! each describing how to size a body or shadow relative to a trailing average
//! of a chosen range type. We replicate the C library's default settings and
//! its averaging semantics exactly:
//!
//! * `CANDLERANGE(setting, i)` is the raw range (real body, high-low, or the
//!   sum of both shadows) at bar `i`.
//! * `CANDLEAVERAGE(setting, i)` is
//!   `factor * (avg_period != 0 ? mean(range over [i-avg_period, i-1]) : range(i)) / shadow_div`
//!   where `shadow_div` is `2.0` for the `Shadows` range type, else `1.0`.
//!
//! Output matches TA-Lib: an `i32` per bar of `-100`, `0`, or `+100`, with the
//! warmup region (`< lookback`) zero-filled. The parity harness compares every
//! bar, so both the lookback and the values must be exact.

#[derive(Clone, Copy)]
enum RangeType {
    RealBody,
    HighLow,
    Shadows,
}

#[derive(Clone, Copy)]
struct CandleSetting {
    range: RangeType,
    avg_period: usize,
    factor: f64,
}

// TA-Lib `TA_CandleDefaultSettings` (ta_global.c).
const BODY_LONG: CandleSetting = CandleSetting {
    range: RangeType::RealBody,
    avg_period: 10,
    factor: 1.0,
};
const BODY_SHORT: CandleSetting = CandleSetting {
    range: RangeType::RealBody,
    avg_period: 10,
    factor: 1.0,
};
const BODY_DOJI: CandleSetting = CandleSetting {
    range: RangeType::HighLow,
    avg_period: 10,
    factor: 0.1,
};
const SHADOW_LONG: CandleSetting = CandleSetting {
    range: RangeType::RealBody,
    avg_period: 0,
    factor: 1.0,
};
const SHADOW_VERY_LONG: CandleSetting = CandleSetting {
    range: RangeType::RealBody,
    avg_period: 0,
    factor: 2.0,
};
const SHADOW_SHORT: CandleSetting = CandleSetting {
    range: RangeType::Shadows,
    avg_period: 10,
    factor: 1.0,
};
const SHADOW_VERY_SHORT: CandleSetting = CandleSetting {
    range: RangeType::HighLow,
    avg_period: 10,
    factor: 0.1,
};
const NEAR: CandleSetting = CandleSetting {
    range: RangeType::HighLow,
    avg_period: 5,
    factor: 0.2,
};
const FAR: CandleSetting = CandleSetting {
    range: RangeType::HighLow,
    avg_period: 5,
    factor: 0.6,
};
const EQUAL: CandleSetting = CandleSetting {
    range: RangeType::HighLow,
    avg_period: 5,
    factor: 0.05,
};

/// Bundled OHLC views plus the candle primitives TA-Lib builds patterns from.
struct Candles<'a> {
    open: &'a [f64],
    high: &'a [f64],
    low: &'a [f64],
    close: &'a [f64],
    len: usize,
}

impl<'a> Candles<'a> {
    fn new(open: &'a [f64], high: &'a [f64], low: &'a [f64], close: &'a [f64]) -> Self {
        let len = open.len().min(high.len()).min(low.len()).min(close.len());
        Self {
            open,
            high,
            low,
            close,
            len,
        }
    }

    #[inline]
    fn real_body(&self, i: usize) -> f64 {
        (self.close[i] - self.open[i]).abs()
    }

    #[inline]
    fn upper_shadow(&self, i: usize) -> f64 {
        self.high[i] - self.open[i].max(self.close[i])
    }

    #[inline]
    fn lower_shadow(&self, i: usize) -> f64 {
        self.open[i].min(self.close[i]) - self.low[i]
    }

    #[inline]
    fn high_low(&self, i: usize) -> f64 {
        self.high[i] - self.low[i]
    }

    /// +1 white (close >= open), -1 black.
    #[inline]
    fn color(&self, i: usize) -> i32 {
        if self.close[i] >= self.open[i] { 1 } else { -1 }
    }

    /// Top of the real body (`max(open, close)`).
    #[inline]
    fn body_top(&self, i: usize) -> f64 {
        self.open[i].max(self.close[i])
    }

    /// Bottom of the real body (`min(open, close)`).
    #[inline]
    fn body_bottom(&self, i: usize) -> f64 {
        self.open[i].min(self.close[i])
    }

    /// `TA_REALBODYGAPUP`: candle `i2`'s real body opens entirely above `i1`'s.
    #[inline]
    fn real_body_gap_up(&self, i2: usize, i1: usize) -> bool {
        self.body_bottom(i2) > self.body_top(i1)
    }

    /// `TA_REALBODYGAPDOWN`: candle `i2`'s real body sits entirely below `i1`'s.
    #[inline]
    fn real_body_gap_down(&self, i2: usize, i1: usize) -> bool {
        self.body_top(i2) < self.body_bottom(i1)
    }

    /// `TA_CANDLEGAPUP`: candle `i2`'s low is above `i1`'s high.
    #[inline]
    fn candle_gap_up(&self, i2: usize, i1: usize) -> bool {
        self.low[i2] > self.high[i1]
    }

    /// `TA_CANDLEGAPDOWN`: candle `i2`'s high is below `i1`'s low.
    #[inline]
    fn candle_gap_down(&self, i2: usize, i1: usize) -> bool {
        self.high[i2] < self.low[i1]
    }

    fn range(&self, setting: &CandleSetting, i: usize) -> f64 {
        match setting.range {
            RangeType::RealBody => self.real_body(i),
            RangeType::HighLow => self.high_low(i),
            RangeType::Shadows => self.upper_shadow(i) + self.lower_shadow(i),
        }
    }

    /// `CANDLEAVERAGE` evaluated for the setting applied to candle `i`. The
    /// trailing window is `[i - avg_period, i - 1]` (exclusive of `i`), matching
    /// TA-Lib's primed running totals.
    fn average(&self, setting: &CandleSetting, i: usize) -> f64 {
        let base = if setting.avg_period == 0 {
            self.range(setting, i)
        } else {
            let mut sum = 0.0;
            for k in (i - setting.avg_period)..i {
                sum += self.range(setting, k);
            }
            sum / setting.avg_period as f64
        };
        let divisor = if matches!(setting.range, RangeType::Shadows) {
            2.0
        } else {
            1.0
        };
        setting.factor * base / divisor
    }
}

/// Run a per-bar pattern test from `lookback` onward, zero-filling the warmup
/// region to mirror TA-Lib's Python output.
fn scan(len: usize, lookback: usize, mut test: impl FnMut(usize) -> i32) -> Vec<i32> {
    let mut out = vec![0i32; len];
    if lookback < len {
        for (i, slot) in out.iter_mut().enumerate().skip(lookback) {
            *slot = test(i);
        }
    }
    out
}

/// Doji: real body no larger than the doji-body average.
pub fn cdl_doji(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_DOJI.avg_period, |i| {
        if k.real_body(i) <= k.average(&BODY_DOJI, i) {
            100
        } else {
            0
        }
    })
}

/// Dragonfly doji: doji body with a negligible upper shadow and a long lower shadow.
pub fn cdl_dragonfly_doji(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_DOJI.avg_period.max(SHADOW_VERY_SHORT.avg_period),
        |i| {
            let svs = k.average(&SHADOW_VERY_SHORT, i);
            if k.real_body(i) <= k.average(&BODY_DOJI, i)
                && k.upper_shadow(i) < svs
                && k.lower_shadow(i) > svs
            {
                100
            } else {
                0
            }
        },
    )
}

/// Gravestone doji: doji body with a negligible lower shadow and a long upper shadow.
pub fn cdl_gravestone_doji(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_DOJI.avg_period.max(SHADOW_VERY_SHORT.avg_period),
        |i| {
            let svs = k.average(&SHADOW_VERY_SHORT, i);
            if k.real_body(i) <= k.average(&BODY_DOJI, i)
                && k.lower_shadow(i) < svs
                && k.upper_shadow(i) > svs
            {
                100
            } else {
                0
            }
        },
    )
}

/// Long-legged doji: doji body with at least one long shadow.
pub fn cdl_long_legged_doji(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_DOJI.avg_period.max(SHADOW_LONG.avg_period),
        |i| {
            let shadow_long = k.average(&SHADOW_LONG, i);
            if k.real_body(i) <= k.average(&BODY_DOJI, i)
                && (k.upper_shadow(i) > shadow_long || k.lower_shadow(i) > shadow_long)
            {
                100
            } else {
                0
            }
        },
    )
}

/// Marubozu: long body with negligible shadows on both ends.
pub fn cdl_marubozu(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_LONG.avg_period.max(SHADOW_VERY_SHORT.avg_period),
        |i| {
            let svs = k.average(&SHADOW_VERY_SHORT, i);
            if k.real_body(i) > k.average(&BODY_LONG, i)
                && k.upper_shadow(i) < svs
                && k.lower_shadow(i) < svs
            {
                100 * k.color(i)
            } else {
                0
            }
        },
    )
}

/// Closing marubozu: long body with no shadow on the closing end.
pub fn cdl_closing_marubozu(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_LONG.avg_period.max(SHADOW_VERY_SHORT.avg_period),
        |i| {
            let svs = k.average(&SHADOW_VERY_SHORT, i);
            if k.real_body(i) > k.average(&BODY_LONG, i)
                && ((k.color(i) == 1 && k.upper_shadow(i) < svs)
                    || (k.color(i) == -1 && k.lower_shadow(i) < svs))
            {
                100 * k.color(i)
            } else {
                0
            }
        },
    )
}

/// Long white/black line: long body with short shadows.
pub fn cdl_long_line(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_LONG.avg_period.max(SHADOW_SHORT.avg_period),
        |i| {
            let short = k.average(&SHADOW_SHORT, i);
            if k.real_body(i) > k.average(&BODY_LONG, i)
                && k.upper_shadow(i) < short
                && k.lower_shadow(i) < short
            {
                100 * k.color(i)
            } else {
                0
            }
        },
    )
}

/// Short line: short body with short shadows.
pub fn cdl_short_line(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(SHADOW_SHORT.avg_period),
        |i| {
            let short = k.average(&SHADOW_SHORT, i);
            if k.real_body(i) < k.average(&BODY_SHORT, i)
                && k.upper_shadow(i) < short
                && k.lower_shadow(i) < short
            {
                100 * k.color(i)
            } else {
                0
            }
        },
    )
}

/// Spinning top: small body with both shadows longer than the body.
pub fn cdl_spinning_top(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_SHORT.avg_period, |i| {
        let body = k.real_body(i);
        if body < k.average(&BODY_SHORT, i) && k.upper_shadow(i) > body && k.lower_shadow(i) > body
        {
            100 * k.color(i)
        } else {
            0
        }
    })
}

/// High-wave: small body with very long shadows on both sides.
pub fn cdl_high_wave(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(SHADOW_VERY_LONG.avg_period),
        |i| {
            let very_long = k.average(&SHADOW_VERY_LONG, i);
            if k.real_body(i) < k.average(&BODY_SHORT, i)
                && k.upper_shadow(i) > very_long
                && k.lower_shadow(i) > very_long
            {
                100 * k.color(i)
            } else {
                0
            }
        },
    )
}

// ---------------------------------------------------------------------------
// Multi-candle patterns. Penetration parameters use TA-Lib's documented
// defaults: 0.3 for the abandoned-baby / morning- / evening-star family and
// 0.5 for dark cloud cover and mat hold.
// ---------------------------------------------------------------------------

/// Two crows: long white, then a black gapping up, then a black opening within
/// the second body and closing within the first body. Always bearish.
pub fn cdl_2crows(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_LONG.avg_period + 2, |i| {
        if k.color(i - 2) == 1
            && k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.color(i - 1) == -1
            && k.real_body_gap_up(i - 1, i - 2)
            && k.color(i) == -1
            && k.open[i] < k.open[i - 1]
            && k.open[i] > k.close[i - 1]
            && k.close[i] > k.open[i - 2]
            && k.close[i] < k.close[i - 2]
        {
            -100
        } else {
            0
        }
    })
}

/// Three black crows: three long black candles with very short lower shadows,
/// each opening within the prior body and closing progressively lower.
pub fn cdl_3blackcrows(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, SHADOW_VERY_SHORT.avg_period + 3, |i| {
        if k.color(i - 3) == 1
            && k.color(i - 2) == -1
            && k.lower_shadow(i - 2) < k.average(&SHADOW_VERY_SHORT, i - 2)
            && k.color(i - 1) == -1
            && k.lower_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.color(i) == -1
            && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.open[i - 1] < k.open[i - 2]
            && k.open[i - 1] > k.close[i - 2]
            && k.open[i] < k.open[i - 1]
            && k.open[i] > k.close[i - 1]
            && k.high[i - 3] > k.close[i - 2]
            && k.close[i - 2] > k.close[i - 1]
            && k.close[i - 1] > k.close[i]
        {
            -100
        } else {
            0
        }
    })
}

/// Three inside up/down: a long candle, a harami body engulfed by it, then a
/// confirming candle of the opposite color to the first.
pub fn cdl_3inside(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 2,
        |i| {
            if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
                && k.real_body(i - 1) <= k.average(&BODY_SHORT, i - 1)
                && k.body_top(i - 1) < k.body_top(i - 2)
                && k.body_bottom(i - 1) > k.body_bottom(i - 2)
                && ((k.color(i - 2) == 1 && k.color(i) == -1 && k.close[i] < k.open[i - 2])
                    || (k.color(i - 2) == -1 && k.color(i) == 1 && k.close[i] > k.open[i - 2]))
            {
                -k.color(i - 2) * 100
            } else {
                0
            }
        },
    )
}

/// Three-line strike: three candles of one color, then a fourth of the opposite
/// color that engulfs the prior run.
pub fn cdl_3linestrike(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, NEAR.avg_period + 3, |i| {
        if k.color(i - 3) == k.color(i - 2)
            && k.color(i - 2) == k.color(i - 1)
            && k.color(i) == -k.color(i - 1)
            && k.open[i - 2] >= k.body_bottom(i - 3) - k.average(&NEAR, i - 3)
            && k.open[i - 2] <= k.body_top(i - 3) + k.average(&NEAR, i - 3)
            && k.open[i - 1] >= k.body_bottom(i - 2) - k.average(&NEAR, i - 2)
            && k.open[i - 1] <= k.body_top(i - 2) + k.average(&NEAR, i - 2)
            && ((k.color(i - 1) == 1
                && k.close[i - 1] > k.close[i - 2]
                && k.close[i - 2] > k.close[i - 3]
                && k.open[i] > k.close[i - 1]
                && k.close[i] < k.open[i - 3])
                || (k.color(i - 1) == -1
                    && k.close[i - 1] < k.close[i - 2]
                    && k.close[i - 2] < k.close[i - 3]
                    && k.open[i] < k.close[i - 1]
                    && k.close[i] > k.open[i - 3]))
        {
            k.color(i - 1) * 100
        } else {
            0
        }
    })
}

/// Three outside up/down: an engulfing pair confirmed by a third candle that
/// continues in the engulfing direction.
pub fn cdl_3outside(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, 3, |i| {
        if (k.color(i - 1) == 1
            && k.color(i - 2) == -1
            && k.close[i - 1] > k.open[i - 2]
            && k.open[i - 1] < k.close[i - 2]
            && k.close[i] > k.close[i - 1])
            || (k.color(i - 1) == -1
                && k.color(i - 2) == 1
                && k.open[i - 1] > k.close[i - 2]
                && k.close[i - 1] < k.open[i - 2]
                && k.close[i] < k.close[i - 1])
        {
            k.color(i - 1) * 100
        } else {
            0
        }
    })
}

/// Three stars in the south: a faltering downtrend of three black candles.
pub fn cdl_3starsinsouth(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = SHADOW_VERY_SHORT
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(BODY_LONG.avg_period)
        .max(BODY_SHORT.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.color(i - 2) == -1
            && k.color(i - 1) == -1
            && k.color(i) == -1
            && k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.lower_shadow(i - 2) > k.average(&SHADOW_LONG, i - 2)
            && k.real_body(i - 1) < k.real_body(i - 2)
            && k.open[i - 1] > k.close[i - 2]
            && k.open[i - 1] <= k.high[i - 2]
            && k.low[i - 1] < k.close[i - 2]
            && k.low[i - 1] >= k.low[i - 2]
            && k.lower_shadow(i - 1) > k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.low[i] > k.low[i - 1]
            && k.high[i] < k.high[i - 1]
        {
            100
        } else {
            0
        }
    })
}

/// Three white soldiers: three rising white candles with very short upper
/// shadows and only modest body shrinkage.
pub fn cdl_3whitesoldiers(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = SHADOW_VERY_SHORT
        .avg_period
        .max(BODY_SHORT.avg_period)
        .max(FAR.avg_period)
        .max(NEAR.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.color(i - 2) == 1
            && k.upper_shadow(i - 2) < k.average(&SHADOW_VERY_SHORT, i - 2)
            && k.color(i - 1) == 1
            && k.upper_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.color(i) == 1
            && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.close[i] > k.close[i - 1]
            && k.close[i - 1] > k.close[i - 2]
            && k.open[i - 1] > k.open[i - 2]
            && k.open[i - 1] <= k.close[i - 2] + k.average(&NEAR, i - 2)
            && k.open[i] > k.open[i - 1]
            && k.open[i] <= k.close[i - 1] + k.average(&NEAR, i - 1)
            && k.real_body(i - 1) > k.real_body(i - 2) - k.average(&FAR, i - 2)
            && k.real_body(i) > k.real_body(i - 1) - k.average(&FAR, i - 1)
            && k.real_body(i) > k.average(&BODY_SHORT, i)
        {
            100
        } else {
            0
        }
    })
}

/// Abandoned baby: a doji isolated by gaps from a long candle and a confirming
/// candle of the opposite color.
pub fn cdl_abandonedbaby(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.3;
    let lookback = BODY_DOJI
        .avg_period
        .max(BODY_LONG.avg_period)
        .max(BODY_SHORT.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.real_body(i - 1) <= k.average(&BODY_DOJI, i - 1)
            && k.real_body(i) > k.average(&BODY_SHORT, i)
            && ((k.color(i - 2) == 1
                && k.color(i) == -1
                && k.close[i] < k.close[i - 2] - k.real_body(i - 2) * penetration
                && k.candle_gap_up(i - 1, i - 2)
                && k.candle_gap_down(i, i - 1))
                || (k.color(i - 2) == -1
                    && k.color(i) == 1
                    && k.close[i] > k.close[i - 2] + k.real_body(i - 2) * penetration
                    && k.candle_gap_down(i - 1, i - 2)
                    && k.candle_gap_up(i, i - 1)))
        {
            k.color(i) * 100
        } else {
            0
        }
    })
}

/// Advance block: three rising white candles whose advance falters via
/// shrinking bodies and lengthening upper shadows.
pub fn cdl_advanceblock(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = SHADOW_LONG
        .avg_period
        .max(SHADOW_SHORT.avg_period)
        .max(FAR.avg_period)
        .max(NEAR.avg_period)
        .max(BODY_LONG.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.color(i - 2) == 1
            && k.color(i - 1) == 1
            && k.color(i) == 1
            && k.close[i] > k.close[i - 1]
            && k.close[i - 1] > k.close[i - 2]
            && k.open[i - 1] > k.open[i - 2]
            && k.open[i - 1] <= k.close[i - 2] + k.average(&NEAR, i - 2)
            && k.open[i] > k.open[i - 1]
            && k.open[i] <= k.close[i - 1] + k.average(&NEAR, i - 1)
            && k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.upper_shadow(i - 2) < k.average(&SHADOW_SHORT, i - 2)
            && ((k.real_body(i - 1) < k.real_body(i - 2) - k.average(&FAR, i - 2)
                && k.real_body(i) < k.real_body(i - 1) + k.average(&NEAR, i - 1))
                || (k.real_body(i) < k.real_body(i - 1) - k.average(&FAR, i - 1))
                || (k.real_body(i) < k.real_body(i - 1)
                    && k.real_body(i - 1) < k.real_body(i - 2)
                    && (k.upper_shadow(i) > k.average(&SHADOW_SHORT, i)
                        || k.upper_shadow(i - 1) > k.average(&SHADOW_SHORT, i - 1)))
                || (k.real_body(i) < k.real_body(i - 1)
                    && k.upper_shadow(i) > k.average(&SHADOW_LONG, i)))
        {
            -100
        } else {
            0
        }
    })
}

/// Belt hold: a long candle that opens at its extreme (no shadow on the open
/// end).
pub fn cdl_belthold(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_LONG.avg_period.max(SHADOW_VERY_SHORT.avg_period),
        |i| {
            if k.real_body(i) > k.average(&BODY_LONG, i)
                && ((k.color(i) == 1 && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i))
                    || (k.color(i) == -1 && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)))
            {
                k.color(i) * 100
            } else {
                0
            }
        },
    )
}

/// Breakaway: a five-candle gap-and-run that closes back through the gap.
pub fn cdl_breakaway(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_LONG.avg_period + 4, |i| {
        if k.real_body(i - 4) > k.average(&BODY_LONG, i - 4)
            && k.color(i - 4) == k.color(i - 3)
            && k.color(i - 3) == k.color(i - 1)
            && k.color(i - 1) == -k.color(i)
            && ((k.color(i - 4) == -1
                && k.real_body_gap_down(i - 3, i - 4)
                && k.high[i - 2] < k.high[i - 3]
                && k.low[i - 2] < k.low[i - 3]
                && k.high[i - 1] < k.high[i - 2]
                && k.low[i - 1] < k.low[i - 2]
                && k.close[i] > k.open[i - 3]
                && k.close[i] < k.close[i - 4])
                || (k.color(i - 4) == 1
                    && k.real_body_gap_up(i - 3, i - 4)
                    && k.high[i - 2] > k.high[i - 3]
                    && k.low[i - 2] > k.low[i - 3]
                    && k.high[i - 1] > k.high[i - 2]
                    && k.low[i - 1] > k.low[i - 2]
                    && k.close[i] < k.open[i - 3]
                    && k.close[i] > k.close[i - 4]))
        {
            k.color(i) * 100
        } else {
            0
        }
    })
}

/// Concealing baby swallow: four black candles where the third's upper shadow
/// pierces the second body and the fourth engulfs the third.
pub fn cdl_concealbabyswall(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, SHADOW_VERY_SHORT.avg_period + 3, |i| {
        if k.color(i - 3) == -1
            && k.color(i - 2) == -1
            && k.color(i - 1) == -1
            && k.color(i) == -1
            && k.lower_shadow(i - 3) < k.average(&SHADOW_VERY_SHORT, i - 3)
            && k.upper_shadow(i - 3) < k.average(&SHADOW_VERY_SHORT, i - 3)
            && k.lower_shadow(i - 2) < k.average(&SHADOW_VERY_SHORT, i - 2)
            && k.upper_shadow(i - 2) < k.average(&SHADOW_VERY_SHORT, i - 2)
            && k.real_body_gap_down(i - 1, i - 2)
            && k.upper_shadow(i - 1) > k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.high[i - 1] > k.close[i - 2]
            && k.high[i] > k.high[i - 1]
            && k.low[i] < k.low[i - 1]
        {
            100
        } else {
            0
        }
    })
}

/// Counterattack: two long opposite-colored candles that close at the same
/// level.
pub fn cdl_counterattack(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period.max(BODY_LONG.avg_period) + 1, |i| {
        if k.color(i - 1) == -k.color(i)
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.real_body(i) > k.average(&BODY_LONG, i)
            && k.close[i] <= k.close[i - 1] + k.average(&EQUAL, i - 1)
            && k.close[i] >= k.close[i - 1] - k.average(&EQUAL, i - 1)
        {
            k.color(i) * 100
        } else {
            0
        }
    })
}

/// Dark cloud cover: a black candle opening above a prior long white candle and
/// closing well into its body.
pub fn cdl_darkcloudcover(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.5;
    scan(k.len, BODY_LONG.avg_period + 1, |i| {
        if k.color(i - 1) == 1
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.color(i) == -1
            && k.open[i] > k.high[i - 1]
            && k.close[i] > k.open[i - 1]
            && k.close[i] < k.close[i - 1] - k.real_body(i - 1) * penetration
        {
            -100
        } else {
            0
        }
    })
}

/// Doji star: a long candle followed by a gapping doji.
pub fn cdl_dojistar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_DOJI.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.real_body(i) <= k.average(&BODY_DOJI, i)
                && ((k.color(i - 1) == 1 && k.real_body_gap_up(i, i - 1))
                    || (k.color(i - 1) == -1 && k.real_body_gap_down(i, i - 1)))
            {
                -k.color(i - 1) * 100
            } else {
                0
            }
        },
    )
}

/// Engulfing: a candle whose real body fully engulfs the prior opposite-colored
/// body (±80 when one end exactly matches).
pub fn cdl_engulfing(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, 2, |i| {
        let engulfs = (k.color(i) == 1
            && k.color(i - 1) == -1
            && ((k.close[i] >= k.open[i - 1] && k.open[i] < k.close[i - 1])
                || (k.close[i] > k.open[i - 1] && k.open[i] <= k.close[i - 1])))
            || (k.color(i) == -1
                && k.color(i - 1) == 1
                && ((k.open[i] >= k.close[i - 1] && k.close[i] < k.open[i - 1])
                    || (k.open[i] > k.close[i - 1] && k.close[i] <= k.open[i - 1])));
        if engulfs {
            if k.open[i] != k.close[i - 1] && k.close[i] != k.open[i - 1] {
                k.color(i) * 100
            } else {
                k.color(i) * 80
            }
        } else {
            0
        }
    })
}

/// Evening doji star: long white, gapping doji, then a black candle closing well
/// into the first body.
pub fn cdl_eveningdojistar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.3;
    let lookback = BODY_DOJI
        .avg_period
        .max(BODY_LONG.avg_period)
        .max(BODY_SHORT.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.color(i - 2) == 1
            && k.real_body(i - 1) <= k.average(&BODY_DOJI, i - 1)
            && k.real_body_gap_up(i - 1, i - 2)
            && k.real_body(i) > k.average(&BODY_SHORT, i)
            && k.color(i) == -1
            && k.close[i] < k.close[i - 2] - k.real_body(i - 2) * penetration
        {
            -100
        } else {
            0
        }
    })
}

/// Evening star: long white, a short-bodied star gapping up, then a black candle
/// closing well into the first body.
pub fn cdl_eveningstar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.3;
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 2,
        |i| {
            if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
                && k.color(i - 2) == 1
                && k.real_body(i - 1) <= k.average(&BODY_SHORT, i - 1)
                && k.real_body_gap_up(i - 1, i - 2)
                && k.real_body(i) > k.average(&BODY_SHORT, i)
                && k.color(i) == -1
                && k.close[i] < k.close[i - 2] - k.real_body(i - 2) * penetration
            {
                -100
            } else {
                0
            }
        },
    )
}

/// Up/down-gap side-by-side white lines: a gap followed by two similar white
/// candles.
pub fn cdl_gapsidesidewhite(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, NEAR.avg_period.max(EQUAL.avg_period) + 2, |i| {
        let gap_up = k.real_body_gap_up(i - 1, i - 2) && k.real_body_gap_up(i, i - 2);
        let gap_down = k.real_body_gap_down(i - 1, i - 2) && k.real_body_gap_down(i, i - 2);
        if (gap_up || gap_down)
            && k.color(i - 1) == 1
            && k.color(i) == 1
            && k.real_body(i) >= k.real_body(i - 1) - k.average(&NEAR, i - 1)
            && k.real_body(i) <= k.real_body(i - 1) + k.average(&NEAR, i - 1)
            && k.open[i] >= k.open[i - 1] - k.average(&EQUAL, i - 1)
            && k.open[i] <= k.open[i - 1] + k.average(&EQUAL, i - 1)
        {
            if gap_up { 100 } else { -100 }
        } else {
            0
        }
    })
}

/// Hammer: a small-bodied candle with a long lower shadow near the prior lows.
pub fn cdl_hammer(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_SHORT
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(SHADOW_VERY_SHORT.avg_period)
        .max(NEAR.avg_period)
        + 1;
    scan(k.len, lookback, |i| {
        if k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.lower_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.body_bottom(i) <= k.low[i - 1] + k.average(&NEAR, i - 1)
        {
            100
        } else {
            0
        }
    })
}

/// Hanging man: a small-bodied candle with a long lower shadow near the prior
/// highs.
pub fn cdl_hangingman(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_SHORT
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(SHADOW_VERY_SHORT.avg_period)
        .max(NEAR.avg_period)
        + 1;
    scan(k.len, lookback, |i| {
        if k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.lower_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.body_bottom(i) >= k.high[i - 1] - k.average(&NEAR, i - 1)
        {
            -100
        } else {
            0
        }
    })
}

/// Harami: a small real body engulfed by the prior long body (±80 when one end
/// matches).
pub fn cdl_harami(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.real_body(i) <= k.average(&BODY_SHORT, i)
            {
                if k.body_top(i) < k.body_top(i - 1) && k.body_bottom(i) > k.body_bottom(i - 1) {
                    -k.color(i - 1) * 100
                } else if k.body_top(i) <= k.body_top(i - 1)
                    && k.body_bottom(i) >= k.body_bottom(i - 1)
                {
                    -k.color(i - 1) * 80
                } else {
                    0
                }
            } else {
                0
            }
        },
    )
}

/// Harami cross: a harami whose inside candle is a doji.
pub fn cdl_haramicross(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_DOJI.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.real_body(i) <= k.average(&BODY_DOJI, i)
            {
                if k.body_top(i) < k.body_top(i - 1) && k.body_bottom(i) > k.body_bottom(i - 1) {
                    -k.color(i - 1) * 100
                } else if k.body_top(i) <= k.body_top(i - 1)
                    && k.body_bottom(i) >= k.body_bottom(i - 1)
                {
                    -k.color(i - 1) * 80
                } else {
                    0
                }
            } else {
                0
            }
        },
    )
}

/// Hikkake: an inside bar followed by a false breakout, optionally confirmed
/// within three bars (±200 on confirmation). Stateful across bars.
pub fn cdl_hikkake(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = 5usize;
    let mut out = vec![0i32; k.len];
    if lookback >= k.len {
        return out;
    }
    let detect = |i: usize| -> bool {
        k.high[i - 1] < k.high[i - 2]
            && k.low[i - 1] > k.low[i - 2]
            && ((k.high[i] < k.high[i - 1] && k.low[i] < k.low[i - 1])
                || (k.high[i] > k.high[i - 1] && k.low[i] > k.low[i - 1]))
    };
    let mut pattern_result = 0i32;
    let mut pattern_idx = 0usize;
    let mut i = lookback - 3;
    while i < k.len {
        if detect(i) {
            pattern_result = 100 * if k.high[i] < k.high[i - 1] { 1 } else { -1 };
            pattern_idx = i;
            if i >= lookback {
                out[i] = pattern_result;
            }
        } else if i <= pattern_idx + 3
            && ((pattern_result > 0 && k.close[i] > k.high[pattern_idx - 1])
                || (pattern_result < 0 && k.close[i] < k.low[pattern_idx - 1]))
        {
            if i >= lookback {
                out[i] = pattern_result + 100 * if pattern_result > 0 { 1 } else { -1 };
            }
            pattern_idx = 0;
        }
        i += 1;
    }
    out
}

/// Modified hikkake: a three-bar inside formation with a close near the second
/// candle's extreme, optionally confirmed within three bars. Stateful.
pub fn cdl_hikkakemod(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = 1usize.max(NEAR.avg_period) + 5;
    let mut out = vec![0i32; k.len];
    if lookback >= k.len {
        return out;
    }
    let detect = |i: usize| -> bool {
        let near = k.average(&NEAR, i - 2);
        k.high[i - 2] < k.high[i - 3]
            && k.low[i - 2] > k.low[i - 3]
            && k.high[i - 1] < k.high[i - 2]
            && k.low[i - 1] > k.low[i - 2]
            && ((k.high[i] < k.high[i - 1]
                && k.low[i] < k.low[i - 1]
                && k.close[i - 2] <= k.low[i - 2] + near)
                || (k.high[i] > k.high[i - 1]
                    && k.low[i] > k.low[i - 1]
                    && k.close[i - 2] >= k.high[i - 2] - near))
    };
    let mut pattern_result = 0i32;
    let mut pattern_idx = 0usize;
    let mut i = lookback - 3;
    while i < k.len {
        if detect(i) {
            pattern_result = 100 * if k.high[i] < k.high[i - 1] { 1 } else { -1 };
            pattern_idx = i;
            if i >= lookback {
                out[i] = pattern_result;
            }
        } else if i <= pattern_idx + 3
            && ((pattern_result > 0 && k.close[i] > k.high[pattern_idx - 1])
                || (pattern_result < 0 && k.close[i] < k.low[pattern_idx - 1]))
        {
            if i >= lookback {
                out[i] = pattern_result + 100 * if pattern_result > 0 { 1 } else { -1 };
            }
            pattern_idx = 0;
        }
        i += 1;
    }
    out
}

/// Homing pigeon: a small black body engulfed by a prior long black body.
pub fn cdl_homingpigeon(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.color(i - 1) == -1
                && k.color(i) == -1
                && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.real_body(i) <= k.average(&BODY_SHORT, i)
                && k.open[i] < k.open[i - 1]
                && k.close[i] > k.close[i - 1]
            {
                100
            } else {
                0
            }
        },
    )
}

/// Identical three crows: three black candles whose opens match the prior
/// closes.
pub fn cdl_identical3crows(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        SHADOW_VERY_SHORT.avg_period.max(EQUAL.avg_period) + 2,
        |i| {
            if k.color(i - 2) == -1
                && k.lower_shadow(i - 2) < k.average(&SHADOW_VERY_SHORT, i - 2)
                && k.color(i - 1) == -1
                && k.lower_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
                && k.color(i) == -1
                && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
                && k.close[i - 2] > k.close[i - 1]
                && k.close[i - 1] > k.close[i]
                && k.open[i - 1] <= k.close[i - 2] + k.average(&EQUAL, i - 2)
                && k.open[i - 1] >= k.close[i - 2] - k.average(&EQUAL, i - 2)
                && k.open[i] <= k.close[i - 1] + k.average(&EQUAL, i - 1)
                && k.open[i] >= k.close[i - 1] - k.average(&EQUAL, i - 1)
            {
                -100
            } else {
                0
            }
        },
    )
}

/// In-neck: a black candle followed by a white one closing just into the prior
/// body.
pub fn cdl_inneck(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period.max(BODY_LONG.avg_period) + 1, |i| {
        if k.color(i - 1) == -1
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.color(i) == 1
            && k.open[i] < k.low[i - 1]
            && k.close[i] <= k.close[i - 1] + k.average(&EQUAL, i - 1)
            && k.close[i] >= k.close[i - 1]
        {
            -100
        } else {
            0
        }
    })
}

/// Inverted hammer: a small-bodied candle with a long upper shadow gapping down.
pub fn cdl_invertedhammer(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_SHORT
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(SHADOW_VERY_SHORT.avg_period)
        + 1;
    scan(k.len, lookback, |i| {
        if k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.upper_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.real_body_gap_down(i, i - 1)
        {
            100
        } else {
            0
        }
    })
}

/// Kicking: two opposite marubozu separated by a gap.
pub fn cdl_kicking(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        SHADOW_VERY_SHORT.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.color(i - 1) == -k.color(i)
                && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.upper_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
                && k.lower_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
                && k.real_body(i) > k.average(&BODY_LONG, i)
                && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
                && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
                && ((k.color(i - 1) == -1 && k.candle_gap_up(i, i - 1))
                    || (k.color(i - 1) == 1 && k.candle_gap_down(i, i - 1)))
            {
                k.color(i) * 100
            } else {
                0
            }
        },
    )
}

/// Kicking by length: like kicking, but the result follows the longer marubozu.
pub fn cdl_kickingbylength(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        SHADOW_VERY_SHORT.avg_period.max(BODY_LONG.avg_period) + 1,
        |i| {
            if k.color(i - 1) == -k.color(i)
                && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
                && k.upper_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
                && k.lower_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
                && k.real_body(i) > k.average(&BODY_LONG, i)
                && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
                && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
                && ((k.color(i - 1) == -1 && k.candle_gap_up(i, i - 1))
                    || (k.color(i - 1) == 1 && k.candle_gap_down(i, i - 1)))
            {
                let longer = if k.real_body(i) > k.real_body(i - 1) {
                    i
                } else {
                    i - 1
                };
                k.color(longer) * 100
            } else {
                0
            }
        },
    )
}

/// Ladder bottom: three declining black candles, a black candle with an upper
/// shadow, then a white candle breaking out.
pub fn cdl_ladderbottom(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, SHADOW_VERY_SHORT.avg_period + 4, |i| {
        if k.color(i - 4) == -1
            && k.color(i - 3) == -1
            && k.color(i - 2) == -1
            && k.open[i - 4] > k.open[i - 3]
            && k.open[i - 3] > k.open[i - 2]
            && k.close[i - 4] > k.close[i - 3]
            && k.close[i - 3] > k.close[i - 2]
            && k.color(i - 1) == -1
            && k.upper_shadow(i - 1) > k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.color(i) == 1
            && k.open[i] > k.open[i - 1]
            && k.close[i] > k.high[i - 1]
        {
            100
        } else {
            0
        }
    })
}

/// Matching low: two black candles closing at the same level.
pub fn cdl_matchinglow(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period + 1, |i| {
        if k.color(i - 1) == -1
            && k.color(i) == -1
            && k.close[i] <= k.close[i - 1] + k.average(&EQUAL, i - 1)
            && k.close[i] >= k.close[i - 1] - k.average(&EQUAL, i - 1)
        {
            100
        } else {
            0
        }
    })
}

/// Mat hold: a long white candle, an upside gap, three holding candles, then a
/// white candle breaking out.
pub fn cdl_mathold(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.5;
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 4,
        |i| {
            if k.real_body(i - 4) > k.average(&BODY_LONG, i - 4)
                && k.real_body(i - 3) < k.average(&BODY_SHORT, i - 3)
                && k.real_body(i - 2) < k.average(&BODY_SHORT, i - 2)
                && k.real_body(i - 1) < k.average(&BODY_SHORT, i - 1)
                && k.color(i - 4) == 1
                && k.color(i - 3) == -1
                && k.color(i) == 1
                && k.real_body_gap_up(i - 3, i - 4)
                && k.body_bottom(i - 2) < k.close[i - 4]
                && k.body_bottom(i - 1) < k.close[i - 4]
                && k.body_bottom(i - 2) > k.close[i - 4] - k.real_body(i - 4) * penetration
                && k.body_bottom(i - 1) > k.close[i - 4] - k.real_body(i - 4) * penetration
                && k.body_top(i - 2) < k.open[i - 3]
                && k.body_top(i - 1) < k.body_top(i - 2)
                && k.open[i] > k.close[i - 1]
                && k.close[i] > k.high[i - 3].max(k.high[i - 2]).max(k.high[i - 1])
            {
                100
            } else {
                0
            }
        },
    )
}

/// Morning doji star: long black, gapping doji, then a white candle closing well
/// into the first body.
pub fn cdl_morningdojistar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.3;
    let lookback = BODY_DOJI
        .avg_period
        .max(BODY_LONG.avg_period)
        .max(BODY_SHORT.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.color(i - 2) == -1
            && k.real_body(i - 1) <= k.average(&BODY_DOJI, i - 1)
            && k.real_body_gap_down(i - 1, i - 2)
            && k.real_body(i) > k.average(&BODY_SHORT, i)
            && k.color(i) == 1
            && k.close[i] > k.close[i - 2] + k.real_body(i - 2) * penetration
        {
            100
        } else {
            0
        }
    })
}

/// Morning star: long black, a short-bodied star gapping down, then a white
/// candle closing well into the first body.
pub fn cdl_morningstar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let penetration = 0.3;
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 2,
        |i| {
            if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
                && k.color(i - 2) == -1
                && k.real_body(i - 1) <= k.average(&BODY_SHORT, i - 1)
                && k.real_body_gap_down(i - 1, i - 2)
                && k.real_body(i) > k.average(&BODY_SHORT, i)
                && k.color(i) == 1
                && k.close[i] > k.close[i - 2] + k.real_body(i - 2) * penetration
            {
                100
            } else {
                0
            }
        },
    )
}

/// On-neck: a black candle followed by a white one closing at the prior low.
pub fn cdl_onneck(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period.max(BODY_LONG.avg_period) + 1, |i| {
        if k.color(i - 1) == -1
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.color(i) == 1
            && k.open[i] < k.low[i - 1]
            && k.close[i] <= k.low[i - 1] + k.average(&EQUAL, i - 1)
            && k.close[i] >= k.low[i - 1] - k.average(&EQUAL, i - 1)
        {
            -100
        } else {
            0
        }
    })
}

/// Piercing: a black candle followed by a white candle opening below the prior
/// low and closing above the prior midpoint.
pub fn cdl_piercing(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_LONG.avg_period + 1, |i| {
        if k.color(i - 1) == -1
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.color(i) == 1
            && k.real_body(i) > k.average(&BODY_LONG, i)
            && k.open[i] < k.low[i - 1]
            && k.close[i] < k.open[i - 1]
            && k.close[i] > k.close[i - 1] + k.real_body(i - 1) * 0.5
        {
            100
        } else {
            0
        }
    })
}

/// Rickshaw man: a long-legged doji whose body sits near the high-low midpoint.
pub fn cdl_rickshawman(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_DOJI
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(NEAR.avg_period);
    scan(k.len, lookback, |i| {
        let near = k.average(&NEAR, i);
        let mid = k.low[i] + k.high_low(i) / 2.0;
        if k.real_body(i) <= k.average(&BODY_DOJI, i)
            && k.lower_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.upper_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.body_bottom(i) <= mid + near
            && k.body_top(i) >= mid - near
        {
            100
        } else {
            0
        }
    })
}

/// Rising/falling three methods: a long candle, three small counter-trend
/// candles held within its range, then a long continuation candle.
pub fn cdl_risefall3methods(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 4,
        |i| {
            let dir = k.color(i - 4) as f64;
            if k.real_body(i - 4) > k.average(&BODY_LONG, i - 4)
                && k.real_body(i - 3) < k.average(&BODY_SHORT, i - 3)
                && k.real_body(i - 2) < k.average(&BODY_SHORT, i - 2)
                && k.real_body(i - 1) < k.average(&BODY_SHORT, i - 1)
                && k.real_body(i) > k.average(&BODY_LONG, i)
                && k.color(i - 4) == -k.color(i - 3)
                && k.color(i - 3) == k.color(i - 2)
                && k.color(i - 2) == k.color(i - 1)
                && k.color(i - 1) == -k.color(i)
                && k.body_bottom(i - 3) < k.high[i - 4]
                && k.body_top(i - 3) > k.low[i - 4]
                && k.body_bottom(i - 2) < k.high[i - 4]
                && k.body_top(i - 2) > k.low[i - 4]
                && k.body_bottom(i - 1) < k.high[i - 4]
                && k.body_top(i - 1) > k.low[i - 4]
                && k.close[i - 2] * dir < k.close[i - 3] * dir
                && k.close[i - 1] * dir < k.close[i - 2] * dir
                && k.open[i] * dir > k.close[i - 1] * dir
                && k.close[i] * dir > k.close[i - 4] * dir
            {
                100 * k.color(i - 4)
            } else {
                0
            }
        },
    )
}

/// Separating lines: a candle that opens at the prior open but reverses into a
/// long belt-hold of the opposite color.
pub fn cdl_separatinglines(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = SHADOW_VERY_SHORT
        .avg_period
        .max(BODY_LONG.avg_period)
        .max(EQUAL.avg_period)
        + 1;
    scan(k.len, lookback, |i| {
        if k.color(i - 1) == -k.color(i)
            && k.open[i] <= k.open[i - 1] + k.average(&EQUAL, i - 1)
            && k.open[i] >= k.open[i - 1] - k.average(&EQUAL, i - 1)
            && k.real_body(i) > k.average(&BODY_LONG, i)
            && ((k.color(i) == 1 && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i))
                || (k.color(i) == -1 && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)))
        {
            k.color(i) * 100
        } else {
            0
        }
    })
}

/// Shooting star: a small-bodied candle with a long upper shadow gapping up.
pub fn cdl_shootingstar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_SHORT
        .avg_period
        .max(SHADOW_LONG.avg_period)
        .max(SHADOW_VERY_SHORT.avg_period)
        + 1;
    scan(k.len, lookback, |i| {
        if k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.upper_shadow(i) > k.average(&SHADOW_LONG, i)
            && k.lower_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.real_body_gap_up(i, i - 1)
        {
            -100
        } else {
            0
        }
    })
}

/// Stalled pattern: two long white candles then a small white candle riding the
/// second's shoulder.
pub fn cdl_stalledpattern(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_LONG
        .avg_period
        .max(BODY_SHORT.avg_period)
        .max(SHADOW_VERY_SHORT.avg_period)
        .max(NEAR.avg_period)
        + 2;
    scan(k.len, lookback, |i| {
        if k.color(i - 2) == 1
            && k.color(i - 1) == 1
            && k.color(i) == 1
            && k.close[i] > k.close[i - 1]
            && k.close[i - 1] > k.close[i - 2]
            && k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.upper_shadow(i - 1) < k.average(&SHADOW_VERY_SHORT, i - 1)
            && k.open[i - 1] > k.open[i - 2]
            && k.open[i - 1] <= k.close[i - 2] + k.average(&NEAR, i - 2)
            && k.real_body(i) < k.average(&BODY_SHORT, i)
            && k.open[i] >= k.close[i - 1] - k.real_body(i) - k.average(&NEAR, i - 1)
        {
            -100
        } else {
            0
        }
    })
}

/// Stick sandwich: a black candle, a white candle trading above it, then a black
/// candle closing at the first close.
pub fn cdl_sticksandwich(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period + 2, |i| {
        if k.color(i - 2) == -1
            && k.color(i - 1) == 1
            && k.color(i) == -1
            && k.low[i - 1] > k.close[i - 2]
            && k.close[i] <= k.close[i - 2] + k.average(&EQUAL, i - 2)
            && k.close[i] >= k.close[i - 2] - k.average(&EQUAL, i - 2)
        {
            100
        } else {
            0
        }
    })
}

/// Takuri: a doji-bodied candle with a very long lower shadow and negligible
/// upper shadow.
pub fn cdl_takuri(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    let lookback = BODY_DOJI
        .avg_period
        .max(SHADOW_VERY_SHORT.avg_period)
        .max(SHADOW_VERY_LONG.avg_period);
    scan(k.len, lookback, |i| {
        if k.real_body(i) <= k.average(&BODY_DOJI, i)
            && k.upper_shadow(i) < k.average(&SHADOW_VERY_SHORT, i)
            && k.lower_shadow(i) > k.average(&SHADOW_VERY_LONG, i)
        {
            100
        } else {
            0
        }
    })
}

/// Tasuki gap: a gap followed by an opposite-colored candle of similar size that
/// fails to close the gap.
pub fn cdl_tasukigap(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, NEAR.avg_period + 2, |i| {
        let near = k.average(&NEAR, i - 1);
        if (k.real_body_gap_up(i - 1, i - 2)
            && k.color(i - 1) == 1
            && k.color(i) == -1
            && k.open[i] < k.close[i - 1]
            && k.open[i] > k.open[i - 1]
            && k.close[i] < k.open[i - 1]
            && k.close[i] > k.body_top(i - 2)
            && (k.real_body(i - 1) - k.real_body(i)).abs() < near)
            || (k.real_body_gap_down(i - 1, i - 2)
                && k.color(i - 1) == -1
                && k.color(i) == 1
                && k.open[i] < k.open[i - 1]
                && k.open[i] > k.close[i - 1]
                && k.close[i] > k.open[i - 1]
                && k.close[i] < k.body_bottom(i - 2)
                && (k.real_body(i - 1) - k.real_body(i)).abs() < near)
        {
            k.color(i - 1) * 100
        } else {
            0
        }
    })
}

/// Thrusting: a black candle followed by a white candle closing into, but under
/// the midpoint of, the prior body.
pub fn cdl_thrusting(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, EQUAL.avg_period.max(BODY_LONG.avg_period) + 1, |i| {
        if k.color(i - 1) == -1
            && k.real_body(i - 1) > k.average(&BODY_LONG, i - 1)
            && k.color(i) == 1
            && k.open[i] < k.low[i - 1]
            && k.close[i] > k.close[i - 1] + k.average(&EQUAL, i - 1)
            && k.close[i] <= k.close[i - 1] + k.real_body(i - 1) * 0.5
        {
            -100
        } else {
            0
        }
    })
}

/// Tristar: three doji where the middle one gaps away from the others.
pub fn cdl_tristar(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, BODY_DOJI.avg_period + 2, |i| {
        let doji = k.average(&BODY_DOJI, i - 2);
        if k.real_body(i - 2) <= doji && k.real_body(i - 1) <= doji && k.real_body(i) <= doji {
            let mut result = 0;
            if k.real_body_gap_up(i - 1, i - 2) && k.body_top(i) < k.body_top(i - 1) {
                result = -100;
            }
            if k.real_body_gap_down(i - 1, i - 2) && k.body_bottom(i) > k.body_bottom(i - 1) {
                result = 100;
            }
            result
        } else {
            0
        }
    })
}

/// Unique three river bottom: a long black candle, a black harami making a new
/// low, then a small white candle.
pub fn cdl_unique3river(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 2,
        |i| {
            if k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
                && k.color(i - 2) == -1
                && k.color(i - 1) == -1
                && k.close[i - 1] > k.close[i - 2]
                && k.open[i - 1] <= k.open[i - 2]
                && k.low[i - 1] < k.low[i - 2]
                && k.real_body(i) < k.average(&BODY_SHORT, i)
                && k.color(i) == 1
                && k.open[i] > k.low[i - 1]
            {
                100
            } else {
                0
            }
        },
    )
}

/// Upside gap two crows: a long white candle, a short black gapping up, then a
/// black candle engulfing it but still closing above the first.
pub fn cdl_upsidegap2crows(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(
        k.len,
        BODY_SHORT.avg_period.max(BODY_LONG.avg_period) + 2,
        |i| {
            if k.color(i - 2) == 1
                && k.real_body(i - 2) > k.average(&BODY_LONG, i - 2)
                && k.color(i - 1) == -1
                && k.real_body(i - 1) <= k.average(&BODY_SHORT, i - 1)
                && k.real_body_gap_up(i - 1, i - 2)
                && k.color(i) == -1
                && k.open[i] > k.open[i - 1]
                && k.close[i] < k.close[i - 1]
                && k.close[i] > k.close[i - 2]
            {
                -100
            } else {
                0
            }
        },
    )
}

/// Up/down-gap three methods: a gap between two same-colored candles filled by a
/// third opposite candle.
pub fn cdl_xsidegap3methods(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> Vec<i32> {
    let k = Candles::new(open, high, low, close);
    scan(k.len, 2, |i| {
        if k.color(i - 2) == k.color(i - 1)
            && k.color(i - 1) == -k.color(i)
            && k.open[i] < k.body_top(i - 1)
            && k.open[i] > k.body_bottom(i - 1)
            && k.close[i] < k.body_top(i - 2)
            && k.close[i] > k.body_bottom(i - 2)
            && ((k.color(i - 2) == 1 && k.real_body_gap_up(i - 1, i - 2))
                || (k.color(i - 2) == -1 && k.real_body_gap_down(i - 1, i - 2)))
        {
            k.color(i - 2) * 100
        } else {
            0
        }
    })
}
