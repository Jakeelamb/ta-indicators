//! TA-Lib warmup-exact parity for the Hilbert Transform family.
//!
//! Kept separate from `talib_parity.rs` so the candlestick port and the
//! Hilbert port can land without touching the same test file. Shares the same
//! committed fixture (`tests/fixtures/talib_parity.json`) and the same
//! warmup-exact policy: the Rust output must match TA-Lib on every bar where
//! TA-Lib emitted a value.

use serde_json::Value;
use talib_rs as ta;

const REL_TOL: f64 = 1e-6;
const ABS_TOL: f64 = 1e-6;

fn load() -> (Vec<f64>, Value) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/talib_parity.json"
    );
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read parity fixture {path}: {err}"));
    let root: Value = serde_json::from_str(&raw).expect("parse parity fixture json");
    let close = root["input"]["close"]
        .as_array()
        .expect("missing input close")
        .iter()
        .map(|value| value.as_f64().expect("finite close value"))
        .collect();
    (close, root)
}

fn expected_floats(root: &Value, key: &str) -> Vec<Option<f64>> {
    root["expected"][key]
        .as_array()
        .unwrap_or_else(|| panic!("missing expected series {key}"))
        .iter()
        .map(|value| {
            if value.is_null() {
                None
            } else {
                value.as_f64()
            }
        })
        .collect()
}

fn check(actual: &[Option<f64>], expected: &[Option<f64>]) -> Result<usize, String> {
    if actual.len() != expected.len() {
        return Err(format!(
            "length {} != expected {}",
            actual.len(),
            expected.len()
        ));
    }
    let mut compared = 0usize;
    for (idx, exp) in expected.iter().enumerate() {
        let Some(exp) = exp else { continue };
        compared += 1;
        match actual[idx] {
            Some(got) => {
                let diff = (got - exp).abs();
                let tol = ABS_TOL + REL_TOL * exp.abs();
                if diff > tol {
                    return Err(format!(
                        "idx {idx}: got {got:.10}, expected {exp:.10} (diff {diff:.3e} > tol {tol:.3e})"
                    ));
                }
            }
            None => return Err(format!("idx {idx}: got None, expected {exp:.10}")),
        }
    }
    if compared == 0 {
        return Err("no overlapping values".to_string());
    }
    Ok(compared)
}

#[test]
fn hilbert_family_matches_talib() {
    let (c, root) = load();

    let (phasor_inphase, phasor_quadrature) = ta::ht_phasor(&c);
    let (sine, leadsine) = ta::ht_sine(&c);
    let (mama, fama) = ta::mama(&c, 0.5, 0.05);

    let registry: Vec<(&str, Vec<Option<f64>>)> = vec![
        ("ht_dcperiod", ta::ht_dcperiod(&c)),
        ("ht_phasor_inphase", phasor_inphase),
        ("ht_phasor_quadrature", phasor_quadrature),
        ("ht_dcphase", ta::ht_dcphase(&c)),
        ("ht_sine", sine),
        ("ht_leadsine", leadsine),
        ("ht_trendline", ta::ht_trendline(&c)),
        ("ht_trendmode", ta::ht_trendmode(&c)),
        ("mama_0p5_0p05", mama),
        ("fama_0p5_0p05", fama),
    ];

    let mut failures = Vec::new();
    let mut passed = 0usize;
    for (key, actual) in &registry {
        let expected = expected_floats(&root, key);
        match check(actual, &expected) {
            Ok(_) => passed += 1,
            Err(msg) => failures.push(format!("{key}: {msg}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} Hilbert parity failures ({} passed):\n{}",
        failures.len(),
        passed,
        failures.join("\n")
    );
    assert_eq!(passed, registry.len());
}
