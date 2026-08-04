//! Machine-readable contracts for indicator functions admitted to a research lane.
//!
//! The crate contains many more functions than this initial inventory. An
//! indicator appears here only after its input, warmup, continuity, and oracle
//! semantics have been made explicit. Absence from this inventory does not
//! reject an indicator from future certification.

use core::fmt;

pub const INDICATOR_CONTRACT_VERSION: &str = "indicator_contract_v1";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IndicatorInput {
    High,
    Low,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IndicatorOracle {
    TaLib,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ContinuityRequirement {
    /// Every input row belongs to the next eligible exchange session.
    ///
    /// Callers must split or restart a lane at a missing/ineligible session.
    /// They must not bridge the gap by inserting synthetic OHLC values.
    DenseEligibleSessions,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum WarmupRule {
    PeriodMinusOne,
    Period,
}

impl WarmupRule {
    pub fn first_output_index(self, period: usize) -> Result<usize, IndicatorContractError> {
        if period == 0 {
            return Err(IndicatorContractError::ZeroPeriod);
        }
        Ok(match self {
            Self::PeriodMinusOne => period - 1,
            Self::Period => period,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IndicatorContract {
    id: &'static str,
    function: &'static str,
    inputs: &'static [IndicatorInput],
    oracle: IndicatorOracle,
    warmup: WarmupRule,
    continuity: ContinuityRequirement,
}

impl IndicatorContract {
    const fn new(
        id: &'static str,
        function: &'static str,
        inputs: &'static [IndicatorInput],
        oracle: IndicatorOracle,
        warmup: WarmupRule,
        continuity: ContinuityRequirement,
    ) -> Self {
        Self {
            id,
            function,
            inputs,
            oracle,
            warmup,
            continuity,
        }
    }

    pub const fn id(self) -> &'static str {
        self.id
    }

    pub const fn function(self) -> &'static str {
        self.function
    }

    pub const fn inputs(self) -> &'static [IndicatorInput] {
        self.inputs
    }

    pub const fn oracle(self) -> IndicatorOracle {
        self.oracle
    }

    pub const fn warmup(self) -> WarmupRule {
        self.warmup
    }

    pub const fn continuity(self) -> ContinuityRequirement {
        self.continuity
    }
}

const CLOSE_INPUT: &[IndicatorInput] = &[IndicatorInput::Close];
const HLC_INPUTS: &[IndicatorInput] = &[
    IndicatorInput::High,
    IndicatorInput::Low,
    IndicatorInput::Close,
];

pub const EMA: IndicatorContract = IndicatorContract::new(
    "ta_lib.ema.v1",
    "ema",
    CLOSE_INPUT,
    IndicatorOracle::TaLib,
    WarmupRule::PeriodMinusOne,
    ContinuityRequirement::DenseEligibleSessions,
);

pub const WILDER_ATR: IndicatorContract = IndicatorContract::new(
    "ta_lib.atr_wilder.v1",
    "atr",
    HLC_INPUTS,
    IndicatorOracle::TaLib,
    WarmupRule::Period,
    ContinuityRequirement::DenseEligibleSessions,
);

/// Contracts admitted to the first MAUR population preflight.
///
/// This inventory certifies formula semantics only. The caller still owns
/// causal session selection, price-basis lineage, and prior-completed
/// availability.
pub const MAUR_POPULATION_PREFLIGHT_INDICATORS: &[IndicatorContract] = &[EMA, WILDER_ATR];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndicatorContractError {
    ZeroPeriod,
}

impl fmt::Display for IndicatorContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPeriod => formatter.write_str("indicator period must be greater than zero"),
        }
    }
}

impl std::error::Error for IndicatorContractError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn maur_preflight_inventory_has_unique_stable_ids() {
        let ids = MAUR_POPULATION_PREFLIGHT_INDICATORS
            .iter()
            .map(|contract| contract.id())
            .collect::<BTreeSet<_>>();

        assert_eq!(ids.len(), MAUR_POPULATION_PREFLIGHT_INDICATORS.len());
        assert_eq!(MAUR_POPULATION_PREFLIGHT_INDICATORS, &[EMA, WILDER_ATR]);
    }

    #[test]
    fn ema20_and_wilder_atr14_publish_exact_warmup_indices() -> Result<(), IndicatorContractError> {
        assert_eq!(EMA.warmup().first_output_index(20)?, 19);
        assert_eq!(WILDER_ATR.warmup().first_output_index(14)?, 14);
        assert_eq!(
            EMA.warmup().first_output_index(0),
            Err(IndicatorContractError::ZeroPeriod)
        );
        Ok(())
    }

    #[test]
    fn admitted_indicators_require_real_dense_session_inputs() {
        assert_eq!(EMA.inputs(), &[IndicatorInput::Close]);
        assert_eq!(
            WILDER_ATR.inputs(),
            &[
                IndicatorInput::High,
                IndicatorInput::Low,
                IndicatorInput::Close
            ]
        );
        for contract in MAUR_POPULATION_PREFLIGHT_INDICATORS {
            assert_eq!(
                contract.continuity(),
                ContinuityRequirement::DenseEligibleSessions
            );
            assert_eq!(contract.oracle(), IndicatorOracle::TaLib);
        }
    }
}
