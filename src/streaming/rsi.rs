//! Relative Strength Index (RSI) with Wilder's smoothing.
//!
//! RSI measures the speed and magnitude of price changes to evaluate
//! overbought or oversold conditions.

/// Incremental RSI calculator using Wilder's smoothing.
///
/// RSI = 100 - (100 / (1 + RS))
/// where RS = Average Gain / Average Loss
#[derive(Debug, Clone)]
pub struct IncrementalRsi {
    period: usize,
    avg_gain: f64,
    avg_loss: f64,
    prev_close: Option<f64>,
    count: usize,
    // Legacy serialized warmup fields. New instances use avg_gain/avg_loss as
    // running sums during warmup, but old snapshots may still contain values.
    gains: Vec<f64>,
    losses: Vec<f64>,
}

impl IncrementalRsi {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            avg_gain: 0.0,
            avg_loss: 0.0,
            prev_close: None,
            count: 0,
            gains: Vec::new(),
            losses: Vec::new(),
        }
    }

    /// Update with a new close price and return RSI value.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        let result = if let Some(prev) = self.prev_close {
            let change = close - prev;
            let gain = if change > 0.0 { change } else { 0.0 };
            let loss = if change < 0.0 { -change } else { 0.0 };

            self.count += 1;

            if self.count < self.period {
                self.migrate_legacy_warmup_sums();
                self.avg_gain += gain;
                self.avg_loss += loss;
                None
            } else if self.count == self.period {
                self.migrate_legacy_warmup_sums();
                self.avg_gain = (self.avg_gain + gain) / self.period as f64;
                self.avg_loss = (self.avg_loss + loss) / self.period as f64;
                Some(self.calculate_rsi())
            } else {
                // Subsequent RSI: use Wilder's smoothing
                self.avg_gain =
                    (self.avg_gain * (self.period - 1) as f64 + gain) / self.period as f64;
                self.avg_loss =
                    (self.avg_loss * (self.period - 1) as f64 + loss) / self.period as f64;
                Some(self.calculate_rsi())
            }
        } else {
            None
        };

        self.prev_close = Some(close);
        result
    }

    fn migrate_legacy_warmup_sums(&mut self) {
        if self.gains.is_empty() && self.losses.is_empty() {
            return;
        }
        self.avg_gain += self.gains.iter().sum::<f64>();
        self.avg_loss += self.losses.iter().sum::<f64>();
        self.gains.clear();
        self.losses.clear();
    }

    #[inline]
    fn calculate_rsi(&self) -> f64 {
        if self.avg_loss == 0.0 {
            100.0
        } else {
            100.0 * self.avg_gain / (self.avg_gain + self.avg_loss)
        }
    }

    /// Compute RSI for an entire array of close prices.
    pub fn compute_batch(closes: &[f64], period: usize) -> Vec<f64> {
        let mut result = vec![f64::NAN; closes.len()];
        let mut rsi = Self::new(period);

        for (i, &close) in closes.iter().enumerate() {
            if let Some(val) = rsi.update(close) {
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
    fn test_rsi_basic() {
        // Test with simple ascending data - should show high RSI
        let data: Vec<f64> = (0..30).map(|i| 100.0 + i as f64).collect();
        let result = IncrementalRsi::compute_batch(&data, 14);

        // First 14 values should be NaN (period-1 for changes + period for avg)
        assert!(result[13].is_nan());
        assert!(!result[14].is_nan());

        // All gains, no losses - RSI should be 100
        assert!((result[14] - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_rsi_descending() {
        // Test with descending data - should show low RSI
        let data: Vec<f64> = (0..30).map(|i| 100.0 - i as f64).collect();
        let result = IncrementalRsi::compute_batch(&data, 14);

        // All losses, no gains - RSI should be 0
        assert!(!result[14].is_nan());
        assert!(result[14] < 1.0);
    }

    #[test]
    fn test_rsi_range() {
        // RSI should always be between 0 and 100
        let data: Vec<f64> = (0..100)
            .map(|i| 100.0 + (i as f64 * 0.1).sin() * 10.0)
            .collect();
        let result = IncrementalRsi::compute_batch(&data, 14);

        for val in result.iter().filter(|v| !v.is_nan()) {
            assert!(*val >= 0.0 && *val <= 100.0);
        }
    }

    #[test]
    fn test_rsi_incremental_matches_batch() {
        let data: Vec<f64> = (0..50)
            .map(|i| 100.0 + (i as f64 * 0.3).sin() * 5.0)
            .collect();

        let batch = IncrementalRsi::compute_batch(&data, 14);

        let mut rsi = IncrementalRsi::new(14);
        for (i, &close) in data.iter().enumerate() {
            let val = rsi.update(close);
            if let Some(v) = val {
                assert!((v - batch[i]).abs() < 1e-10, "Mismatch at index {}", i);
            } else {
                assert!(batch[i].is_nan());
            }
        }
    }

    #[test]
    fn test_rsi_warmup_does_not_allocate_vectors() {
        let mut rsi = IncrementalRsi::new(14);

        for close in 100..120 {
            rsi.update(close as f64);
        }

        assert_eq!(rsi.gains.capacity(), 0);
        assert_eq!(rsi.losses.capacity(), 0);
    }

    #[test]
    fn test_rsi_migrates_legacy_warmup_vectors() {
        let mut rsi = IncrementalRsi {
            period: 3,
            avg_gain: 0.0,
            avg_loss: 0.0,
            prev_close: Some(102.0),
            count: 2,
            gains: vec![1.0, 1.0],
            losses: vec![0.0, 0.0],
        };

        let value = rsi.update(103.0).unwrap();

        assert!((value - 100.0).abs() < 1e-10);
        assert!(rsi.gains.is_empty());
        assert!(rsi.losses.is_empty());
    }
}
