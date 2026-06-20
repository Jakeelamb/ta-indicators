//! Choppiness Index indicator.
//!
//! CI = 100 * LOG10(SUM(ATR, n) / (Highest High - Lowest Low)) / LOG10(n)
//!
//! CI < 38.2: Strong trend
//! CI 38.2-61.8: Transitional
//! CI > 61.8: Choppy/ranging

use std::collections::VecDeque;

/// Choppiness Index calculator.
///
/// Uses a sliding window for efficient updates.
#[derive(Debug, Clone)]
pub struct ChoppinessIndex {
    period: usize,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    closes: VecDeque<f64>,
    tr_sum: f64,
    tr_values: VecDeque<f64>,
}

impl ChoppinessIndex {
    /// Create a new Choppiness Index calculator.
    pub fn new(period: usize) -> Self {
        Self {
            period,
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
            closes: VecDeque::with_capacity(period + 1),
            tr_sum: 0.0,
            tr_values: VecDeque::with_capacity(period),
        }
    }

    /// Calculate True Range.
    #[inline]
    fn true_range(high: f64, low: f64, prev_close: f64) -> f64 {
        (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs())
    }

    /// Update with a new candle and return Choppiness Index.
    ///
    /// Returns None until we have `period + 1` candles.
    pub fn update(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        self.highs.push_back(high);
        self.lows.push_back(low);
        self.closes.push_back(close);

        // Calculate TR for this candle
        if self.closes.len() > 1 {
            let prev_close = self.closes[self.closes.len() - 2];
            let tr = Self::true_range(high, low, prev_close);

            self.tr_values.push_back(tr);
            self.tr_sum += tr;

            // Remove oldest TR if we have too many
            if self.tr_values.len() > self.period {
                if let Some(oldest_tr) = self.tr_values.pop_front() {
                    self.tr_sum -= oldest_tr;
                }
            }
        }

        // Remove oldest OHLC if we have too many
        if self.highs.len() > self.period + 1 {
            self.highs.pop_front();
            self.lows.pop_front();
            self.closes.pop_front();
        }

        // Check if we have enough data
        if self.tr_values.len() < self.period {
            return None;
        }

        // Calculate highest high and lowest low over period (excluding first bar)
        let highest: f64 = self
            .highs
            .iter()
            .skip(1)
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let lowest: f64 = self
            .lows
            .iter()
            .skip(1)
            .cloned()
            .fold(f64::INFINITY, f64::min);

        let range_diff = highest - lowest;
        if range_diff <= 0.0 {
            return Some(100.0); // Maximum choppiness if no range
        }

        // Choppiness Index formula
        let ci = 100.0 * (self.tr_sum / range_diff).log10() / (self.period as f64).log10();

        // Clamp to valid range
        Some(ci.clamp(0.0, 100.0))
    }

    /// Check if ready.
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.tr_values.len() >= self.period
    }

    /// Compute Choppiness Index for arrays (batch mode).
    pub fn compute_arrays(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
        assert_eq!(highs.len(), lows.len());
        assert_eq!(lows.len(), closes.len());

        let mut result = vec![f64::NAN; highs.len()];
        let mut ci = Self::new(period);

        for i in 0..highs.len() {
            if let Some(val) = ci.update(highs[i], lows[i], closes[i]) {
                result[i] = val;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_choppiness_ranging() {
        // Ranging market: price oscillates in a narrow range
        let highs = vec![
            100.0, 100.5, 100.3, 100.7, 100.2, 100.6, 100.4, 100.8, 100.1, 100.5, 100.3, 100.6,
            100.2, 100.7, 100.4, 100.5,
        ];
        let lows = vec![
            99.0, 99.5, 99.3, 99.7, 99.2, 99.6, 99.4, 99.8, 99.1, 99.5, 99.3, 99.6, 99.2, 99.7,
            99.4, 99.5,
        ];
        let closes = vec![
            99.5, 100.0, 99.8, 100.2, 99.7, 100.1, 99.9, 100.3, 99.6, 100.0, 99.8, 100.1, 99.7,
            100.2, 99.9, 100.0,
        ];

        let result = ChoppinessIndex::compute_arrays(&highs, &lows, &closes, 14);

        // Ranging market should have high choppiness (>50)
        if !result[15].is_nan() {
            assert!(
                result[15] > 40.0,
                "Expected high CI for ranging market, got {}",
                result[15]
            );
        }
    }

    #[test]
    fn test_choppiness_trending() {
        // Strong uptrend
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut closes = Vec::new();

        for i in 0..20 {
            let base = 100.0 + (i as f64) * 2.0;
            highs.push(base + 1.0);
            lows.push(base - 0.5);
            closes.push(base + 0.8);
        }

        let result = ChoppinessIndex::compute_arrays(&highs, &lows, &closes, 14);

        // Strong trend should have lower choppiness
        if !result[19].is_nan() {
            // In strong trends, CI is typically lower
            assert!(
                result[19] < 80.0,
                "Expected moderate CI for trending market, got {}",
                result[19]
            );
        }
    }

    #[test]
    fn test_choppiness_bounds() {
        let highs = vec![110.0; 20];
        let lows = vec![90.0; 20];
        let closes = vec![100.0; 20];

        let result = ChoppinessIndex::compute_arrays(&highs, &lows, &closes, 14);

        for val in result.iter() {
            if !val.is_nan() {
                assert!(*val >= 0.0 && *val <= 100.0, "CI out of bounds: {}", val);
            }
        }
    }
}
