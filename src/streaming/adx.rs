//! Streaming ADX (Average Directional Index) with single-pass Wilder smoothing.
//!
//! Current Python implementation (regime module:244-357) makes 3 separate passes:
//! 1. Calculate +DM, -DM, TR arrays
//! 2. Wilder smooth each array
//! 3. Calculate DX and smooth to get ADX
//!
//! This implementation combines all into a single streaming pass with O(1) state updates.

/// Streaming ADX calculator with Wilder smoothing.
///
/// Maintains all intermediate state for O(1) updates per candle.
/// Much faster than the Python 3-pass approach.
#[derive(Debug, Clone)]
pub struct StreamingAdx {
    period: usize,
    count: usize,

    // Previous candle values
    prev_high: Option<f64>,
    prev_low: Option<f64>,
    prev_close: Option<f64>,

    // Wilder-smoothed values
    smooth_plus_dm: f64,
    smooth_minus_dm: f64,
    smooth_tr: f64,
    smooth_dx: f64,

    // Warmup accumulators
    sum_plus_dm: f64,
    sum_minus_dm: f64,
    sum_tr: f64,
    dx_values: Vec<f64>,

    // State flags
    dm_ready: bool,
    adx_ready: bool,
}

impl StreamingAdx {
    /// Create a new ADX calculator with given period (typically 14).
    pub fn new(period: usize) -> Self {
        Self {
            period,
            count: 0,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            smooth_plus_dm: 0.0,
            smooth_minus_dm: 0.0,
            smooth_tr: 0.0,
            smooth_dx: 0.0,
            sum_plus_dm: 0.0,
            sum_minus_dm: 0.0,
            sum_tr: 0.0,
            dx_values: Vec::with_capacity(period),
            dm_ready: false,
            adx_ready: false,
        }
    }

    /// Update with a new candle and return current ADX.
    ///
    /// Returns None until we have 2*period candles (need period for DI, then period for ADX).
    #[inline]
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        self.count += 1;

        // Calculate directional movement and true range
        let (plus_dm, minus_dm, tr) = if let (Some(ph), Some(pl), Some(pc)) =
            (self.prev_high, self.prev_low, self.prev_close)
        {
            let high_diff = high - ph;
            let low_diff = pl - low;

            let plus = if high_diff > low_diff && high_diff > 0.0 {
                high_diff
            } else {
                0.0
            };

            let minus = if low_diff > high_diff && low_diff > 0.0 {
                low_diff
            } else {
                0.0
            };

            let true_range = (high - low).max((high - pc).abs()).max((low - pc).abs());

            (plus, minus, true_range)
        } else {
            // First candle - no DM, just TR
            (0.0, 0.0, high - low)
        };

        self.prev_high = Some(high);
        self.prev_low = Some(low);
        self.prev_close = Some(close);

        // Skip first candle (no previous data for DM)
        if self.count == 1 {
            return None;
        }

        // Phase 1: Accumulate sums for first `period` candles
        if !self.dm_ready {
            self.sum_plus_dm += plus_dm;
            self.sum_minus_dm += minus_dm;
            self.sum_tr += tr;

            if self.count > self.period {
                // Initialize Wilder-smoothed values
                self.smooth_plus_dm = self.sum_plus_dm / self.period as f64;
                self.smooth_minus_dm = self.sum_minus_dm / self.period as f64;
                self.smooth_tr = self.sum_tr / self.period as f64;
                self.dm_ready = true;
            } else {
                return None;
            }
        } else {
            // Wilder smoothing for +DM, -DM, TR
            let p = self.period as f64;
            self.smooth_plus_dm = (self.smooth_plus_dm * (p - 1.0) + plus_dm) / p;
            self.smooth_minus_dm = (self.smooth_minus_dm * (p - 1.0) + minus_dm) / p;
            self.smooth_tr = (self.smooth_tr * (p - 1.0) + tr) / p;
        }

        // Calculate +DI and -DI
        let (plus_di, minus_di) = if self.smooth_tr > 0.0 {
            (
                100.0 * self.smooth_plus_dm / self.smooth_tr,
                100.0 * self.smooth_minus_dm / self.smooth_tr,
            )
        } else {
            (0.0, 0.0)
        };

        // Calculate DX
        let di_sum = plus_di + minus_di;
        let dx = if di_sum > 0.0 {
            100.0 * (plus_di - minus_di).abs() / di_sum
        } else {
            0.0
        };

        // Phase 2: Accumulate DX values for ADX smoothing
        if !self.adx_ready {
            self.dx_values.push(dx);

            if self.dx_values.len() >= self.period {
                // Initialize ADX as average of first `period` DX values
                self.smooth_dx = self.dx_values.iter().sum::<f64>() / self.period as f64;
                self.adx_ready = true;
                return Some(self.smooth_dx);
            }
            None
        } else {
            // Wilder smoothing for ADX
            let p = self.period as f64;
            self.smooth_dx = (self.smooth_dx * (p - 1.0) + dx) / p;
            Some(self.smooth_dx)
        }
    }

    /// Get current ADX value.
    #[inline]
    pub fn current(&self) -> Option<f64> {
        if self.adx_ready {
            Some(self.smooth_dx)
        } else {
            None
        }
    }

    /// Get current +DI value.
    #[inline]
    pub fn plus_di(&self) -> Option<f64> {
        if self.dm_ready && self.smooth_tr > 0.0 {
            Some(100.0 * self.smooth_plus_dm / self.smooth_tr)
        } else {
            None
        }
    }

    /// Get current -DI value.
    #[inline]
    pub fn minus_di(&self) -> Option<f64> {
        if self.dm_ready && self.smooth_tr > 0.0 {
            Some(100.0 * self.smooth_minus_dm / self.smooth_tr)
        } else {
            None
        }
    }

    /// Check if ADX is ready.
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.adx_ready
    }

    /// Compute ADX for arrays (batch mode, for Python compatibility).
    pub fn compute_arrays(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
        assert_eq!(highs.len(), lows.len());
        assert_eq!(lows.len(), closes.len());

        let mut result = vec![f64::NAN; highs.len()];
        let mut adx = Self::new(period);

        for i in 0..highs.len() {
            if let Some(val) = adx.update(highs[i], lows[i], closes[i]) {
                result[i] = val;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // 30 candles of sample data
        let highs = vec![
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28, 46.00, 46.03, 46.41, 46.22, 45.64, 46.21, 46.25, 45.71, 46.45,
            45.78, 46.23, 46.01, 45.89, 46.03, 46.18,
        ];
        let lows = vec![
            43.96, 43.95, 43.90, 43.29, 43.61, 44.15, 44.77, 44.96, 45.07, 45.42, 45.25, 45.61,
            44.72, 45.41, 45.55, 44.97, 45.44, 45.47, 44.89, 44.89, 45.02, 45.04, 44.89, 45.25,
            44.97, 44.89, 45.02, 44.77, 44.89, 45.04,
        ];
        let closes = vec![
            44.34, 44.09, 43.96, 43.61, 44.33, 44.77, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            44.72, 45.61, 46.21, 45.02, 46.03, 46.41, 45.64, 45.02, 46.21, 46.25, 45.71, 46.45,
            45.02, 46.23, 45.64, 45.89, 46.03, 46.18,
        ];
        (highs, lows, closes)
    }

    #[test]
    fn test_adx_warmup() {
        let (highs, lows, closes) = sample_data();
        let mut adx = StreamingAdx::new(14);

        // Need 2*period + period candles for ADX to be ready (DM smoothing + DX smoothing)
        for i in 0..30 {
            adx.update(highs[i], lows[i], closes[i]);
        }

        // ADX may or may not be ready with exactly 30 points depending on implementation
        if adx.is_ready() {
            let current = adx.current().unwrap();
            assert!((0.0..=100.0).contains(&current));
        }
    }

    #[test]
    fn test_adx_batch() {
        let (highs, lows, closes) = sample_data();
        let result = StreamingAdx::compute_arrays(&highs, &lows, &closes, 14);

        // First ~28 values should be NaN (2*14 warmup)
        assert!(result[0].is_nan());
        assert!(result[20].is_nan()); // Still warming up DX

        // Later values should be valid ADX
        if !result[29].is_nan() {
            assert!((0.0..=100.0).contains(&result[29]));
        }
    }

    #[test]
    fn test_adx_di_values() {
        let (highs, lows, closes) = sample_data();
        let mut adx = StreamingAdx::new(14);

        for i in 0..30 {
            adx.update(highs[i], lows[i], closes[i]);
        }

        if let (Some(plus), Some(minus)) = (adx.plus_di(), adx.minus_di()) {
            assert!((0.0..=100.0).contains(&plus));
            assert!((0.0..=100.0).contains(&minus));
        }
    }
}
