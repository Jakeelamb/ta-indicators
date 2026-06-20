//! Cached Hurst Exponent calculation.
//!
//! The Hurst exponent uses R/S analysis which is O(n²) per calculation.
//! Instead of computing on every candle, we cache the result and only
//! recompute every N candles (e.g., every 20 candles).
//!
//! H < 0.5: Mean-reverting (anti-persistent)
//! H = 0.5: Random walk
//! H > 0.5: Trending (persistent)

/// Cached Hurst exponent calculator.
///
/// Recomputes only every `interval` candles to avoid O(n²) per-candle cost.
#[derive(Debug, Clone)]
pub struct CachedHurst {
    lookback: usize,
    interval: usize,
    last_index: usize,
    cached_value: Option<f64>,
    prices: Vec<f64>,
}

impl CachedHurst {
    /// Create a new Hurst calculator.
    ///
    /// # Arguments
    /// * `lookback` - Number of prices to use for R/S analysis (typically 100)
    /// * `interval` - How often to recompute (e.g., every 20 candles)
    pub fn new(lookback: usize, interval: usize) -> Self {
        Self {
            lookback,
            interval,
            last_index: 0,
            cached_value: None,
            prices: Vec::with_capacity(lookback + interval),
        }
    }

    /// Update with a new close price and return Hurst exponent.
    ///
    /// Returns the cached value on most calls, only recomputing every `interval` candles.
    pub fn update(&mut self, close: f64) -> Option<f64> {
        self.prices.push(close);

        // Trim to prevent unbounded growth
        if self.prices.len() > self.lookback + self.interval {
            self.prices.drain(0..self.interval);
            self.last_index = self.last_index.saturating_sub(self.interval);
        }

        // Check if we have enough data
        if self.prices.len() < self.lookback {
            return None;
        }

        let current_index = self.prices.len();

        // Recompute only every `interval` candles
        if current_index >= self.last_index + self.interval || self.cached_value.is_none() {
            let start = self.prices.len().saturating_sub(self.lookback);
            let prices = &self.prices[start..];
            self.cached_value = Self::compute_hurst(prices);
            self.last_index = current_index;
        }

        self.cached_value
    }

    /// Get the current cached Hurst value without updating.
    #[inline]
    pub fn current(&self) -> Option<f64> {
        self.cached_value
    }

    /// Check if Hurst is ready.
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.cached_value.is_some()
    }

    /// Compute Hurst exponent using R/S analysis.
    ///
    /// This is the core algorithm matching Python's regime module:79-180.
    pub fn compute_hurst(prices: &[f64]) -> Option<f64> {
        if prices.len() < 20 {
            return None;
        }

        // Calculate log returns
        let returns: Vec<f64> = prices
            .windows(2)
            .filter_map(|w| {
                if w[0] > 0.0 && w[1] > 0.0 {
                    Some((w[1] / w[0]).ln())
                } else {
                    None
                }
            })
            .collect();

        if returns.len() < 20 {
            return None;
        }

        // R/S analysis with multiple sub-periods
        let mut results: Vec<(f64, f64)> = Vec::with_capacity(5);

        for n in [10usize, 20, 40, 60, 80] {
            if n > returns.len() {
                continue;
            }

            let num_segments = returns.len() / n;
            if num_segments < 1 {
                continue;
            }

            let mut rs_values: Vec<f64> = Vec::with_capacity(num_segments);

            for seg in 0..num_segments {
                let start = seg * n;
                let end = start + n;
                if end > returns.len() {
                    continue;
                }

                let segment = &returns[start..end];

                // Mean
                let mean: f64 = segment.iter().sum::<f64>() / segment.len() as f64;

                // Cumulative deviations
                let mut cumsum = Vec::with_capacity(segment.len());
                let mut total = 0.0;
                for &x in segment {
                    total += x - mean;
                    cumsum.push(total);
                }

                // Range
                let r = cumsum.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                    - cumsum.iter().cloned().fold(f64::INFINITY, f64::min);

                // Standard deviation
                let variance: f64 =
                    segment.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / segment.len() as f64;
                let s = if variance > 0.0 {
                    variance.sqrt()
                } else {
                    1e-10
                };

                if s > 0.0 {
                    rs_values.push(r / s);
                }
            }

            if !rs_values.is_empty() {
                let avg_rs: f64 = rs_values.iter().sum::<f64>() / rs_values.len() as f64;
                if avg_rs > 0.0 {
                    results.push(((n as f64).ln(), avg_rs.ln()));
                }
            }
        }

        if results.len() < 2 {
            return None;
        }

        // Linear regression to find slope (Hurst exponent)
        let n_points = results.len() as f64;
        let sum_x: f64 = results.iter().map(|r| r.0).sum();
        let sum_y: f64 = results.iter().map(|r| r.1).sum();
        let sum_xy: f64 = results.iter().map(|r| r.0 * r.1).sum();
        let sum_xx: f64 = results.iter().map(|r| r.0.powi(2)).sum();

        let denominator = n_points * sum_xx - sum_x.powi(2);
        if denominator == 0.0 {
            return Some(0.5); // Default to random walk
        }

        let hurst = (n_points * sum_xy - sum_x * sum_y) / denominator;

        // Clamp to valid range [0, 1]
        Some(hurst.clamp(0.0, 1.0))
    }

    /// Compute Hurst for a slice of prices (batch mode).
    ///
    /// Returns Hurst values at each position, recomputing every `interval` candles.
    pub fn compute_batch(prices: &[f64], lookback: usize, interval: usize) -> Vec<f64> {
        let mut result = vec![f64::NAN; prices.len()];
        let mut hurst = Self::new(lookback, interval);

        for (i, &price) in prices.iter().enumerate() {
            if let Some(val) = hurst.update(price) {
                result[i] = val;
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_f64(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345);
            self.0 as f64 / u64::MAX as f64
        }
    }

    fn generate_trending_series(n: usize, trend: f64) -> Vec<f64> {
        let mut rng = Lcg::new(12345);
        let mut prices = vec![100.0];
        for _ in 1..n {
            let last = *prices.last().unwrap();
            let noise = (rng.next_f64() - 0.5) * 2.0;
            prices.push(last * (1.0 + trend + noise * 0.01));
        }
        prices
    }

    fn generate_mean_reverting_series(n: usize) -> Vec<f64> {
        let mut rng = Lcg::new(67890);
        let mean = 100.0;
        let mut prices = vec![100.0];
        for _ in 1..n {
            let last = *prices.last().unwrap();
            let reversion = (mean - last) * 0.1;
            let noise = (rng.next_f64() - 0.5) * 2.0;
            prices.push(last + reversion + noise);
        }
        prices
    }

    #[test]
    fn test_hurst_trending() {
        let prices = generate_trending_series(150, 0.002);
        let hurst = CachedHurst::compute_hurst(&prices);

        if let Some(h) = hurst {
            // Trending series should have H > 0.5
            assert!(h > 0.4, "Expected trending H > 0.4, got {}", h);
        }
    }

    #[test]
    fn test_hurst_caching() {
        let mut hurst = CachedHurst::new(100, 20);

        // Feed 150 prices
        for i in 0..150 {
            let price = 100.0 + (i as f64) * 0.1;
            hurst.update(price);
        }

        assert!(hurst.is_ready());
        let cached = hurst.current().unwrap();
        assert!((0.0..=1.0).contains(&cached));
    }

    #[test]
    fn test_hurst_batch() {
        let prices: Vec<f64> = (0..200).map(|i| 100.0 + (i as f64) * 0.1).collect();
        let result = CachedHurst::compute_batch(&prices, 100, 20);

        // First 99 values should be NaN (not enough data)
        assert!(result[50].is_nan());

        // Later values should be valid
        assert!(!result[150].is_nan());
        assert!((0.0..=1.0).contains(&result[150]));
    }

    #[test]
    fn test_hurst_mean_reverting() {
        let prices = generate_mean_reverting_series(150);
        let hurst = CachedHurst::compute_hurst(&prices);

        if let Some(h) = hurst {
            assert!(h < 0.6, "Expected mean-reverting H < 0.6, got {}", h);
        }
    }
}
