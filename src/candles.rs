

















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


    #[inline]
    fn color(&self, i: usize) -> i32 {
        if self.close[i] >= self.open[i] { 1 } else { -1 }
    }


    #[inline]
    fn body_top(&self, i: usize) -> f64 {
        self.open[i].max(self.close[i])
    }


    #[inline]
    fn body_bottom(&self, i: usize) -> f64 {
        self.open[i].min(self.close[i])
    }


    #[inline]
    fn real_body_gap_up(&self, i2: usize, i1: usize) -> bool {
        self.body_bottom(i2) > self.body_top(i1)
    }


    #[inline]
    fn real_body_gap_down(&self, i2: usize, i1: usize) -> bool {
        self.body_top(i2) < self.body_bottom(i1)
    }


    #[inline]
    fn candle_gap_up(&self, i2: usize, i1: usize) -> bool {
        self.low[i2] > self.high[i1]
    }


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



fn scan(len: usize, lookback: usize, mut test: impl FnMut(usize) -> i32) -> Vec<i32> {
    let mut out = vec![0i32; len];
    if lookback < len {
        for (i, slot) in out.iter_mut().enumerate().skip(lookback) {
            *slot = test(i);
        }
    }
    out
}


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
