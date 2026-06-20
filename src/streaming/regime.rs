//! Regime detection combining Hurst, ADX, and Choppiness.
//!
//! Provides the same interface as Python's regime module but with
//! incremental/cached computation for massive performance gains.

use super::{
    adx::StreamingAdx, choppiness::ChoppinessIndex, hurst::CachedHurst, sma::IncrementalSma,
};

/// Regime state with all indicator values.
#[derive(Debug, Clone, Default)]
pub struct RegimeState {
    pub hurst: Option<f64>,
    pub adx: Option<f64>,
    pub choppiness: Option<f64>,
    pub rvol: Option<f64>,
    pub is_favorable: bool,
    pub label: String,
}

/// Incremental regime detector.
///
/// Combines cached Hurst, streaming ADX, and Choppiness Index
/// for efficient per-candle updates.
#[derive(Debug, Clone)]
pub struct RegimeDetector {
    // Indicator instances
    hurst: CachedHurst,
    adx: StreamingAdx,
    choppiness: ChoppinessIndex,
    volume_sma: IncrementalSma,

    // Thresholds (matching Python defaults)
    hurst_threshold: f64,
    adx_threshold: f64,
    choppiness_threshold: f64,
}

impl Default for RegimeDetector {
    fn default() -> Self {
        Self::new(0.45, 25.0, 55.0)
    }
}

impl RegimeDetector {
    /// Create a new regime detector with custom thresholds.
    ///
    /// Favorable regime for S&D patterns:
    /// - Hurst < hurst_threshold (mean-reverting)
    /// - ADX < adx_threshold (non-trending)
    /// - Choppiness > choppiness_threshold (ranging)
    pub fn new(hurst_threshold: f64, adx_threshold: f64, choppiness_threshold: f64) -> Self {
        Self {
            hurst: CachedHurst::new(100, 20), // Recompute every 20 candles
            adx: StreamingAdx::new(14),
            choppiness: ChoppinessIndex::new(14),
            volume_sma: IncrementalSma::new(20),
            hurst_threshold,
            adx_threshold,
            choppiness_threshold,
        }
    }

    /// Update regime state with a new candle.
    ///
    /// This is O(1) for ADX and Choppiness, O(n) for Hurst but only every N candles.
    pub fn update(&mut self, high: f64, low: f64, close: f64, volume: f64) -> RegimeState {
        // Update all indicators
        let hurst = self.hurst.update(close);
        let adx = self.adx.update(high, low, close);
        let choppiness = self.choppiness.update(high, low, close);

        // Calculate RVOL
        self.volume_sma.update(volume);
        let rvol = self.volume_sma.current().map(|avg| volume / avg);

        // Determine if regime is favorable
        let is_favorable = self.is_favorable_regime(hurst, adx, choppiness);
        let label = self.get_regime_label(hurst, adx, choppiness);

        RegimeState {
            hurst,
            adx,
            choppiness,
            rvol,
            is_favorable,
            label,
        }
    }

    /// Check if current regime is favorable for S&D patterns.
    fn is_favorable_regime(
        &self,
        hurst: Option<f64>,
        adx: Option<f64>,
        choppiness: Option<f64>,
    ) -> bool {
        // If all indicators are None, default to favorable
        if hurst.is_none() && adx.is_none() && choppiness.is_none() {
            return true;
        }

        // Check each available indicator
        if let Some(h) = hurst {
            if h >= self.hurst_threshold {
                return false;
            }
        }

        if let Some(a) = adx {
            if a >= self.adx_threshold {
                return false;
            }
        }

        if let Some(c) = choppiness {
            if c <= self.choppiness_threshold {
                return false;
            }
        }

        true
    }

    /// Get a descriptive label for the current regime.
    fn get_regime_label(
        &self,
        hurst: Option<f64>,
        adx: Option<f64>,
        choppiness: Option<f64>,
    ) -> String {
        if hurst.is_none() && adx.is_none() && choppiness.is_none() {
            return "unknown".to_string();
        }

        let mut is_trending = false;
        let mut is_mean_reverting = false;
        let mut is_choppy = false;

        if let Some(h) = hurst {
            if h > 0.55 {
                is_trending = true;
            } else if h < 0.45 {
                is_mean_reverting = true;
            }
        }

        if let Some(a) = adx {
            if a > 30.0 {
                is_trending = true;
            } else if a < 20.0 {
                is_choppy = true;
            }
        }

        if let Some(c) = choppiness {
            if c > 61.8 {
                is_choppy = true;
            } else if c < 38.2 {
                is_trending = true;
            }
        }

        if is_trending && !is_choppy {
            "trending".to_string()
        } else if is_mean_reverting || (is_choppy && !is_trending) {
            "ranging".to_string()
        } else if is_trending && is_choppy {
            "transitional".to_string()
        } else {
            "neutral".to_string()
        }
    }

    /// Check if all indicators are ready.
    pub fn is_ready(&self) -> bool {
        self.hurst.is_ready() && self.adx.is_ready() && self.choppiness.is_ready()
    }

    /// Compute regime state for entire arrays (batch mode).
    ///
    /// Returns vector of RegimeState for each candle.
    pub fn compute_batch(
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        volumes: &[f64],
        hurst_threshold: f64,
        adx_threshold: f64,
        choppiness_threshold: f64,
    ) -> Vec<RegimeState> {
        assert_eq!(highs.len(), lows.len());
        assert_eq!(lows.len(), closes.len());
        assert_eq!(closes.len(), volumes.len());

        let mut detector = Self::new(hurst_threshold, adx_threshold, choppiness_threshold);
        let mut result = Vec::with_capacity(highs.len());

        for i in 0..highs.len() {
            result.push(detector.update(highs[i], lows[i], closes[i], volumes[i]));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ranging_data() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut closes = Vec::new();
        let mut volumes = Vec::new();

        for i in 0..200 {
            let phase = (i as f64 * 0.1).sin();
            let base = 100.0 + phase * 2.0;
            highs.push(base + 0.5);
            lows.push(base - 0.5);
            closes.push(base);
            volumes.push(1000.0 + (i % 10) as f64 * 10.0);
        }

        (highs, lows, closes, volumes)
    }

    #[test]
    fn test_regime_detector_update() {
        let (highs, lows, closes, volumes) = sample_ranging_data();
        let mut detector = RegimeDetector::default();

        for i in 0..150 {
            detector.update(highs[i], lows[i], closes[i], volumes[i]);
        }

        assert!(detector.is_ready() || detector.adx.is_ready());
    }

    #[test]
    fn test_regime_batch() {
        let (highs, lows, closes, volumes) = sample_ranging_data();
        let results =
            RegimeDetector::compute_batch(&highs, &lows, &closes, &volumes, 0.45, 25.0, 55.0);

        assert_eq!(results.len(), highs.len());

        // Later results should have valid data
        let last = &results[199];
        // At least some indicators should be ready by now
        assert!(last.adx.is_some() || last.choppiness.is_some());
    }

    #[test]
    fn test_favorable_regime_logic() {
        let detector = RegimeDetector::default();

        // Mean-reverting + non-trending + choppy = favorable
        assert!(detector.is_favorable_regime(Some(0.40), Some(20.0), Some(60.0)));

        // Trending Hurst = unfavorable
        assert!(!detector.is_favorable_regime(Some(0.55), Some(20.0), Some(60.0)));

        // High ADX = unfavorable
        assert!(!detector.is_favorable_regime(Some(0.40), Some(30.0), Some(60.0)));

        // Low choppiness = unfavorable
        assert!(!detector.is_favorable_regime(Some(0.40), Some(20.0), Some(50.0)));
    }
}
