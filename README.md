# talib-rs

Warmup-exact Rust port of [TA-Lib](https://ta-lib.org/) batch indicators. Outputs use
`Option<f64>` with `None` on warmup bars, matching TA-Lib's emitted-value semantics bar
for bar (not just post-warmup tails).

Designed as a standalone library: zero runtime dependencies, deterministic parity tests
against committed TA-Lib reference fixtures.

## Layout

```
src/lib.rs       Overlap, momentum, volatility, volume, stats, Hilbert transforms
src/candles.rs   Shared candle-settings framework + 61 CDL pattern functions
tests/           Warmup-exact parity harness (121 fixture keys, two test files)
scripts/         Fixture regeneration via Python TA-Lib (dev-only)
```

### `src/lib.rs`

Single-crate API for non-pattern indicators: moving averages, MACD family, RSI,
Bollinger bands, ADX family, Aroon, SAR/SAREXT, linear regression, BETA/CORREL,
Hilbert stack (`ht_*`, `mama`), and Sabertooth-adjacent helpers (`price_context`,
`bop`, etc.).

Multi-output functions return small structs (`Macd`, `AdxFamily`, `BollingerBands`, …)
or tuples (`ht_phasor`, `ht_sine`, `mama`).

### `src/candles.rs`

TA-Lib-compatible candle pattern recognition:

- `CandleSetting` / `RangeType` — shared body/shadow thresholds (TA-Lib defaults).
- `Candles` — OHLCV wrapper with `real_body`, shadows, `color`, `range`, `average`.
- `cdl_*` — 61 pattern detectors returning `Vec<i32>` (`0`, `±100`, `±200`).

### Tests

| File | Keys | Lookback groups |
| --- | ---: | --- |
| `tests/talib_parity.rs` | 111 | Momentum, overlap, stats, SAR, CDL, … |
| `tests/talib_parity_ht.rs` | 10 | Hilbert / MAMA family |

Policy: **warmup-exact** — Rust must match TA-Lib wherever the fixture has a non-null
reference value, including the warmup region.

Regenerate fixtures (requires Python TA-Lib):

```bash
python3 -m venv artifacts/venv-talib
artifacts/venv-talib/bin/pip install TA-Lib numpy
artifacts/venv-talib/bin/python scripts/gen_parity_fixtures.py
cargo test
```

## Usage

Add to `Cargo.toml`:

```toml
talib-rs = { git = "https://github.com/Jakeelamb/talib-rs.git", branch = "main" }
```

```rust
use talib_rs::{rsi, macd, ht_dcperiod, cdl_engulfing};

let rsi_14 = rsi(&closes, 14);
let macd_out = macd(&closes, 12, 26, 9);
let patterns = cdl_engulfing(&opens, &highs, &lows, &closes);
```

## Coverage

See [research coverage notes](https://github.com/Jakeelamb/sabertooth/blob/main/research/ta_lib_rust_coverage.md)
in Sabertooth for the full TA-Lib function map. Remaining gaps are mostly matype
variants, generic binary operators, and combined min/max exports.

## License

MIT
