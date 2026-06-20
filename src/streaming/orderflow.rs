//! Synthetic orderflow from candle geometry.
//!
//! No L2 data needed — OHLCV encodes orderflow:
//! - Buying pressure: (close - low) / (high - low)
//! - Cumulative delta proxy: EMA of (close - open) * volume
//! - Absorption: high volume + small body = orders absorbed without moving price

/// Streaming orderflow estimator from candle geometry.
///
/// Computes buying/selling pressure, cumulative delta proxy via EMA,
/// and absorption detection per candle. All O(1) per update.
#[derive(Debug, Clone)]
pub struct StreamingOrderflow {
    delta_alpha: f64,
    delta_period: usize,
    delta_count: usize,
    delta_sum: f64,
    delta_ema: f64,
    vol_buffer: Vec<f64>,
    vol_index: usize,
    vol_count: usize,
    vol_sum: f64,
    last_buying_pressure: f64,
    last_delta: f64,
    last_absorption: bool,
    last_demand_score: f64,
    last_supply_score: f64,
    ready: bool,
}

impl Default for StreamingOrderflow {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl StreamingOrderflow {
    pub fn new(ema_period: usize, vol_sma_period: usize) -> Self {
        Self {
            delta_alpha: 2.0 / (ema_period as f64 + 1.0),
            delta_period: ema_period,
            delta_count: 0,
            delta_sum: 0.0,
            delta_ema: 0.0,
            vol_buffer: vec![0.0; vol_sma_period],
            vol_index: 0,
            vol_count: 0,
            vol_sum: 0.0,
            last_buying_pressure: 0.5,
            last_delta: 0.0,
            last_absorption: false,
            last_demand_score: 0.0,
            last_supply_score: 0.0,
            ready: false,
        }
    }

    /// Default: 14-period EMA for delta, 20-period SMA for volume.
    pub fn with_defaults() -> Self {
        Self::new(14, 20)
    }

    /// Update with a new candle.
    pub fn update(&mut self, open: f64, high: f64, low: f64, close: f64, volume: f64) {
        let range = high - low;
        let body_pct = if range > 0.0 {
            self.last_buying_pressure = (close - low) / range;
            ((close - open).abs() / range) * 100.0
        } else {
            self.last_buying_pressure = 0.5;
            0.0
        };

        let delta_raw = (close - open) * volume;
        if self.delta_count < self.delta_period {
            self.delta_count += 1;
            self.delta_sum += delta_raw;
            if self.delta_count == self.delta_period {
                self.delta_ema = self.delta_sum / self.delta_period as f64;
            }
        } else {
            self.delta_ema =
                self.delta_alpha * delta_raw + (1.0 - self.delta_alpha) * self.delta_ema;
        }
        self.last_delta = if self.delta_count >= self.delta_period {
            self.delta_ema
        } else {
            0.0
        };

        if self.vol_count < self.vol_buffer.len() {
            self.vol_buffer[self.vol_index] = volume;
            self.vol_sum += volume;
            self.vol_count += 1;
        } else {
            let old_volume = self.vol_buffer[self.vol_index];
            self.vol_buffer[self.vol_index] = volume;
            self.vol_sum += volume - old_volume;
        }
        self.vol_index = (self.vol_index + 1) % self.vol_buffer.len();

        let avg_vol = if self.vol_count >= self.vol_buffer.len() {
            self.ready = true;
            self.vol_sum / self.vol_buffer.len() as f64
        } else {
            self.ready = false;
            0.0
        };

        self.last_absorption = self.ready && volume > avg_vol * 1.5 && body_pct < 30.0;

        if self.ready {
            let pressure_score = (self.last_buying_pressure - 0.5) * 2.0;
            let demand_delta_score = if self.last_delta > 0.0 { 0.2 } else { -0.2 };
            let supply_delta_score = if self.last_delta < 0.0 { 0.2 } else { -0.2 };
            let absorption_bonus = if self.last_absorption { 0.2 } else { 0.0 };
            self.last_demand_score =
                (pressure_score * 0.6 + demand_delta_score + absorption_bonus).clamp(-1.0, 1.0);
            self.last_supply_score =
                (-pressure_score * 0.6 + supply_delta_score + absorption_bonus).clamp(-1.0, 1.0);
        } else {
            self.last_demand_score = 0.0;
            self.last_supply_score = 0.0;
        }
    }

    /// Flow score for a given direction.
    pub fn flow_score(&self, is_demand: bool) -> f64 {
        if is_demand {
            self.last_demand_score
        } else {
            self.last_supply_score
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strong_bullish_candle() {
        let mut of = StreamingOrderflow::new(3, 3);
        for i in 0..5 {
            let base = 100.0 + i as f64;
            of.update(base, base + 2.0, base - 0.5, base + 1.8, 1000.0);
        }
        of.update(100.0, 110.0, 99.0, 109.0, 1500.0);
        let score = of.flow_score(true);
        assert!(
            score > 0.0,
            "Bullish candle should have positive demand flow: {score}"
        );
    }

    #[test]
    fn test_strong_bearish_candle() {
        let mut of = StreamingOrderflow::new(3, 3);
        for i in 0..5 {
            let base = 100.0 + i as f64;
            of.update(base, base + 2.0, base - 0.5, base - 0.3, 1000.0);
        }
        of.update(110.0, 111.0, 99.0, 100.0, 1500.0);
        let score = of.flow_score(false);
        assert!(
            score > 0.0,
            "Bearish candle should have positive supply flow: {score}"
        );
    }

    #[test]
    fn test_absorption_detection() {
        let mut of = StreamingOrderflow::new(3, 3);
        for _ in 0..5 {
            of.update(100.0, 102.0, 99.0, 100.5, 1000.0);
        }
        of.update(100.0, 105.0, 95.0, 100.2, 3000.0);
        assert!(
            of.last_absorption,
            "Should detect absorption: high vol + small body"
        );
    }

    #[test]
    fn test_flow_score_disabled_when_not_ready() {
        let of = StreamingOrderflow::new(20, 20);
        assert_eq!(of.flow_score(true), 0.0);
    }
}
