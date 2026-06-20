//! Incremental O(1)-per-bar indicators for live detectors and feature caches.
//!
//! Batch TA-Lib ports live in the crate root; this module holds streaming state
//! machines used by strategy detectors (RSI, SMA, ADX, regime, orderflow, etc.).

pub mod adx;
pub mod choppiness;
pub mod hurst;
pub mod orderflow;
pub mod regime;
pub mod rsi;
pub mod sma;

pub use adx::StreamingAdx;
pub use choppiness::ChoppinessIndex;
pub use hurst::CachedHurst;
pub use orderflow::StreamingOrderflow;
pub use regime::{RegimeDetector, RegimeState};
pub use rsi::IncrementalRsi;
pub use sma::IncrementalSma;
