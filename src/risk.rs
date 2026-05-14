// Risk mathematics. Pure functions with no I/O or async.
// All calculations are deterministic and side-effect-free.
//
// `TigerStyle`: assertion density ≥ 2 per function.
// Every function asserts preconditions (finite, in-domain inputs)
// and postconditions (output invariants).
//
// Design rationale:
// - Half-Kelly: Full Kelly assumes perfect probability estimates.
//   Halving the fraction protects against estimation error and
//   reduces drawdown risk while retaining most of the growth.
// - Volatility cap at 2.0×: Prevents a single volatility spike from
//   inflating position size beyond reasonable leverage. A 2× cap
//   keeps sizing conservative even in extreme vol compression.
// - Conservative wins (min across models): No model is always right.
//   Taking the minimum size across Kelly, Fixed Fractional, and
//   Volatility-Adjusted ensures we never exceed what any model
//   considers prudent.

#![allow(clippy::float_cmp)]

use std::fmt;

/// Exhaustive error variants for risk evaluation.
/// `TigerStyle`: typed errors, never stringly-typed.
/// Callers match on variants; strings are derived via `Display`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskError {
    /// The `request_id` field was empty.
    RequestIdEmpty,
    /// Balance is zero or negative — no capital to allocate.
    BalanceZero,
    /// Entry price is zero or negative — invalid market data.
    EntryPriceZero,
    /// Stop distance is zero — cannot calculate position size.
    StopDistanceZero,
    /// Asset volatility is zero or negative — cannot scale position.
    InvalidVolatility,
    /// Portfolio state bytes could not be decoded.
    PortfolioDecodeError,
    /// Order details bytes could not be decoded.
    OrderDecodeError,
    /// All sizing models produced zero — position rejected.
    RiskThresholdViolation,
}

impl fmt::Display for RiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            RiskError::RequestIdEmpty => "request_id must not be empty",
            RiskError::BalanceZero => "balance must be greater than zero",
            RiskError::EntryPriceZero => "entry_price must be greater than zero",
            RiskError::StopDistanceZero => "stop distance must be greater than zero",
            RiskError::InvalidVolatility => "volatility must be greater than zero",
            RiskError::PortfolioDecodeError => "failed to decode portfolio_state bytes",
            RiskError::OrderDecodeError => "failed to decode order_details bytes",
            RiskError::RiskThresholdViolation => "risk threshold violation: position size is zero",
        };
        f.write_str(message)
    }
}

#[derive(Debug, Clone)]
pub struct InternalPortfolio {
    pub balance: f64,
    pub win_rate: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
}

#[derive(Debug, Clone)]
pub struct InternalOrder {
    pub entry_price: f64,
    pub stop_price: f64,
    pub asset_volatility: f64,
}

#[derive(Debug, Clone)]
pub struct RiskParams {
    pub risk_percent: f64,
    pub target_vol: f64,
    pub max_position_cap: f64,
}

pub struct RiskResult {
    pub request_id: String,
    pub is_approved: bool,
    pub size: f64,
    pub reason: Option<RiskError>,
}

/// Evaluate risk for a proposed order against a portfolio.
///
/// # Panics
///
/// Panics if any input is `NaN`, infinite, or outside domain range.
#[must_use]
pub fn evaluate(
    portfolio: &InternalPortfolio,
    order: &InternalOrder,
    params: &RiskParams,
    request_id: String,
) -> RiskResult {
    // Preconditions: all numeric inputs must be finite and in domain.
    assert!(portfolio.balance.is_finite());
    assert!(portfolio.balance >= 0.0);
    assert!((0.0..=1.0).contains(&portfolio.win_rate));
    assert!(portfolio.win_rate.is_finite());
    assert!(portfolio.avg_win.is_finite());
    assert!(portfolio.avg_win >= 0.0);
    assert!(portfolio.avg_loss.is_finite());
    assert!(portfolio.avg_loss >= 0.0);

    assert!(order.entry_price.is_finite());
    assert!(order.entry_price > 0.0);
    assert!(order.stop_price.is_finite());
    assert!(order.stop_price >= 0.0);
    assert!(order.asset_volatility.is_finite());
    assert!(order.asset_volatility >= 0.0);

    assert!(params.risk_percent.is_finite());
    assert!(params.risk_percent > 0.0);
    assert!(params.risk_percent <= 100.0);
    assert!(params.target_vol.is_finite());
    assert!(params.target_vol >= 0.0);
    assert!(params.max_position_cap.is_finite());
    assert!(params.max_position_cap > 0.0);

    let final_size = calculate_final_size(
        portfolio.balance,
        params.risk_percent,
        order.entry_price,
        order.stop_price,
        portfolio.win_rate,
        portfolio.avg_win,
        portfolio.avg_loss,
        params.target_vol,
        order.asset_volatility,
        params.max_position_cap,
    );

    // Postcondition: final_size must be non-negative and bounded.
    assert!(final_size.is_finite());
    assert!(final_size >= 0.0);
    assert!(final_size <= params.max_position_cap);

    RiskResult {
        request_id,
        is_approved: final_size > 0.0,
        size: final_size,
        reason: if final_size > 0.0 {
            None
        } else {
            Some(RiskError::RiskThresholdViolation)
        },
    }
}

/// Compute the final position size using all available sizing models.
/// Takes the conservative minimum across Kelly, Fixed Fractional,
/// and Volatility-Adjusted sizing.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn calculate_final_size(
    balance: f64,
    risk_percent: f64,
    entry_price: f64,
    stop_price: f64,
    win_rate: f64,
    avg_win: f64,
    avg_loss: f64,
    target_vol: f64,
    current_vol: f64,
    max_position_cap: f64,
) -> f64 {
    // Preconditions: all inputs validated by caller (evaluate).
    // Internal invariants still checked for defense in depth.
    debug_assert!(balance.is_finite());
    debug_assert!(balance >= 0.0);
    debug_assert!(max_position_cap.is_finite());
    debug_assert!(max_position_cap > 0.0);

    let mut min_size = max_position_cap;

    let ff_size = fixed_fractional(balance, risk_percent, entry_price, stop_price);
    debug_assert!(ff_size.is_finite());
    debug_assert!(ff_size >= 0.0);
    if ff_size > 0.0 {
        min_size = min_size.min(ff_size);
    }

    let kelly_frac = kelly_criterion(win_rate, avg_win, avg_loss);
    debug_assert!(kelly_frac.is_finite());
    debug_assert!((0.0..=1.0).contains(&kelly_frac));
    if kelly_frac > 0.0 && entry_price > 0.0 {
        let kelly_size = (balance * (kelly_frac / 2.0)) / entry_price;
        debug_assert!(kelly_size.is_finite());
        debug_assert!(kelly_size >= 0.0);
        min_size = min_size.min(kelly_size);
    }

    if current_vol > 0.0 && target_vol > 0.0 && ff_size > 0.0 {
        let vol_size = volatility_adjusted(ff_size, target_vol, current_vol);
        debug_assert!(vol_size.is_finite());
        debug_assert!(vol_size >= 0.0);
        min_size = min_size.min(vol_size);
    }

    // Postcondition: result is non-negative and bounded by max_position_cap.
    let result = min_size.max(0.0);
    debug_assert!(result.is_finite());
    debug_assert!(result >= 0.0);
    debug_assert!(result <= max_position_cap);
    result
}

/// Compute position size using fixed-fractional money management.
/// `size = (balance * risk_percent / 100) / stop_distance`.
#[must_use]
pub fn fixed_fractional(balance: f64, risk_percent: f64, entry_price: f64, stop_price: f64) -> f64 {
    debug_assert!(balance.is_finite());
    debug_assert!(balance >= 0.0);
    debug_assert!(risk_percent.is_finite());
    debug_assert!(risk_percent > 0.0);
    debug_assert!(entry_price.is_finite());
    debug_assert!(entry_price > 0.0);
    debug_assert!(stop_price.is_finite());
    debug_assert!(stop_price >= 0.0);

    let stop_distance = (entry_price - stop_price).abs();
    if stop_distance <= 0.0 {
        return 0.0;
    }
    let size = (balance * (risk_percent / 100.0)) / stop_distance;
    debug_assert!(size.is_finite());
    debug_assert!(size >= 0.0);
    size
}

/// Compute the Kelly criterion fraction for optimal position sizing.
/// Returns the fraction of capital to risk, always in `[0, 1]`.
#[must_use]
pub fn kelly_criterion(win_rate: f64, avg_win: f64, avg_loss: f64) -> f64 {
    debug_assert!(win_rate.is_finite());
    debug_assert!((0.0..=1.0).contains(&win_rate),
        "win_rate {win_rate} must be in [0.0, 1.0]");
    debug_assert!(avg_win.is_finite());
    debug_assert!(avg_win >= 0.0);
    debug_assert!(avg_loss.is_finite());
    debug_assert!(avg_loss >= 0.0);

    if avg_loss <= 0.0 || avg_win <= 0.0 {
        return 0.0;
    }
    // Clamp win_rate to [0, 1] as defense against upstream estimation drift.
    // A win rate outside this range indicates a data quality issue upstream;
    // clamping is a last-resort safety net, not a correction.
    let win_rate = win_rate.clamp(0.0, 1.0);
    let win_loss_ratio = avg_win / avg_loss;
    debug_assert!(win_loss_ratio.is_finite());
    debug_assert!(win_loss_ratio >= 0.0);

    let fraction = win_rate - ((1.0 - win_rate) / win_loss_ratio);
    let result = fraction.max(0.0);
    debug_assert!(result.is_finite());
    debug_assert!((0.0..=1.0).contains(&result),
        "kelly fraction {result} must be in [0.0, 1.0]");
    result
}

/// Adjust position size inversely to current volatility.
/// Caps the adjustment at 2× to prevent over-leveraging on
/// transient volatility compression.
#[must_use]
pub fn volatility_adjusted(base_size: f64, target_vol: f64, current_vol: f64) -> f64 {
    debug_assert!(base_size.is_finite());
    debug_assert!(base_size >= 0.0);
    debug_assert!(target_vol.is_finite());
    debug_assert!(target_vol >= 0.0);
    debug_assert!(current_vol.is_finite());
    debug_assert!(current_vol >= 0.0);

    if current_vol <= 0.0 || target_vol <= 0.0 {
        return base_size;
    }
    // Cap the volatility adjustment factor at 2.0.
    // If current volatility is half of target, position size doubles.
    // Beyond 2×, we risk over-leveraging on a transient vol compression.
    let result = base_size * (target_vol / current_vol).min(2.0);
    debug_assert!(result.is_finite());
    debug_assert!(result >= 0.0);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Strategy: valid balance in a reasonable range.
    fn valid_balance() -> impl Strategy<Value = f64> {
        (1.0_f64..1_000_000.0_f64).prop_map(|b| b)
    }

    // Strategy: valid risk percent in (0, 100].
    fn valid_risk_percent() -> impl Strategy<Value = f64> {
        (0.01_f64..100.0_f64).prop_map(|r| r)
    }

    // Strategy: valid entry price.
    fn valid_entry_price() -> impl Strategy<Value = f64> {
        (1.0_f64..1_000_000.0_f64).prop_map(|p| p)
    }

    // Strategy: stop price below entry (positive stop distance).
    fn valid_stop_below_entry(entry: f64) -> impl Strategy<Value = f64> {
        (0.0_f64..entry).prop_map(|s| s)
    }

    // Strategy: valid win rate in [0, 1].
    fn valid_win_rate() -> impl Strategy<Value = f64> {
        (0.0_f64..=1.0_f64).prop_map(|w| w)
    }

    // Strategy: valid avg_win > 0.
    fn valid_avg_win() -> impl Strategy<Value = f64> {
        (0.01_f64..10.0_f64).prop_map(|w| w)
    }

    // Strategy: valid avg_loss > 0.
    fn valid_avg_loss() -> impl Strategy<Value = f64> {
        (0.01_f64..10.0_f64).prop_map(|l| l)
    }

    // Strategy: valid volatility.
    fn valid_volatility() -> impl Strategy<Value = f64> {
        (1.0_f64..10_000.0_f64).prop_map(|v| v)
    }

    // Strategy: valid max_position_cap.
    fn valid_max_cap() -> impl Strategy<Value = f64> {
        (0.01_f64..100.0_f64).prop_map(|c| c)
    }

    // ── fixed_fractional property tests ──

    proptest! {
        #[test]
        fn ff_size_non_negative(balance in valid_balance(),
                                risk_pct in valid_risk_percent(),
                                entry in valid_entry_price()) {
            let stop = entry * 0.99; // 1% stop distance, always positive
            let size = fixed_fractional(balance, risk_pct, entry, stop);
            prop_assert!(size.is_finite());
            prop_assert!(size >= 0.0);
        }

        #[test]
        fn ff_zero_distance_is_zero(balance in valid_balance(),
                                     risk_pct in valid_risk_percent(),
                                     entry in valid_entry_price()) {
            let size = fixed_fractional(balance, risk_pct, entry, entry);
            prop_assert_eq!(size, 0.0);
        }

        #[test]
        fn ff_size_zero_when_balance_zero(risk_pct in valid_risk_percent(),
                                           entry in valid_entry_price()) {
            let stop = entry * 0.95;
            let size = fixed_fractional(0.0, risk_pct, entry, stop);
            prop_assert_eq!(size, 0.0);
        }
    }

    // ── kelly_criterion property tests ──

    proptest! {
        #[test]
        fn kelly_in_unit_range(win_rate in valid_win_rate(),
                               avg_win in valid_avg_win(),
                               avg_loss in valid_avg_loss()) {
            let k = kelly_criterion(win_rate, avg_win, avg_loss);
            prop_assert!(k.is_finite());
            prop_assert!(k >= 0.0);
            prop_assert!(k <= 1.0);
        }

        #[test]
        fn kelly_zero_when_avg_loss_zero(win_rate in valid_win_rate(),
                                          avg_win in valid_avg_win()) {
            let k = kelly_criterion(win_rate, avg_win, 0.0);
            prop_assert_eq!(k, 0.0);
        }

        #[test]
        fn kelly_zero_when_avg_win_zero(win_rate in valid_win_rate(),
                                         avg_loss in valid_avg_loss()) {
            let k = kelly_criterion(win_rate, 0.0, avg_loss);
            prop_assert_eq!(k, 0.0);
        }

        #[test]
        fn kelly_positive_for_good_edge(win_rate in 0.51_f64..=1.0_f64,
                                         avg_win in valid_avg_win(),
                                         avg_loss in valid_avg_loss()) {
            let k = kelly_criterion(win_rate, avg_win, avg_loss);
            prop_assert!(k.is_finite());
            // With edge > 0, kelly should be non-negative (could still be 0 near breakeven).
            prop_assert!(k >= 0.0);
        }
    }

    // ── volatility_adjusted property tests ──

    proptest! {
        #[test]
        fn vol_adj_non_negative(base in (0.01_f64..100.0_f64),
                                target in valid_volatility(),
                                current in valid_volatility()) {
            let size = volatility_adjusted(base, target, current);
            prop_assert!(size.is_finite());
            prop_assert!(size >= 0.0);
        }

        #[test]
        fn vol_adj_identity_when_equal(base in (0.01_f64..100.0_f64),
                                        vol in valid_volatility()) {
            let size = volatility_adjusted(base, vol, vol);
            let diff = (size - base).abs();
            // Allow tiny floating-point error.
            prop_assert!(diff < base * 1e-10);
        }

        #[test]
        fn vol_adj_capped_at_double(base in (0.01_f64..100.0_f64),
                                     target in valid_volatility(),
                                     current in valid_volatility()) {
            let size = volatility_adjusted(base, target, current);
            prop_assert!(size <= base * 2.0 + f64::EPSILON);
        }

        #[test]
        fn vol_adj_zero_vol_returns_base(base in (0.01_f64..100.0_f64),
                                          target in valid_volatility()) {
            let size = volatility_adjusted(base, target, 0.0);
            prop_assert!((size - base).abs() < f64::EPSILON * 100.0);
        }
    }

    // ── calculate_final_size property tests ──

    proptest! {
        #[test]
        fn final_size_always_non_negative(
            balance in valid_balance(),
            risk_pct in valid_risk_percent(),
            entry in valid_entry_price(),
            stop in valid_stop_below_entry(1_000_000.0),
            win_rate in valid_win_rate(),
            avg_win in valid_avg_win(),
            avg_loss in valid_avg_loss(),
            target_vol in valid_volatility(),
            current_vol in valid_volatility(),
            max_cap in valid_max_cap(),
        ) {
            let size = calculate_final_size(
                balance, risk_pct, entry, stop,
                win_rate, avg_win, avg_loss,
                target_vol, current_vol, max_cap,
            );
            prop_assert!(size.is_finite());
            prop_assert!(size >= 0.0);
            prop_assert!(size <= max_cap + f64::EPSILON);
        }

        #[test]
        fn final_size_respects_max_cap(
            balance in valid_balance(),
            risk_pct in valid_risk_percent(),
            entry in valid_entry_price(),
            stop in valid_stop_below_entry(1_000_000.0),
            win_rate in valid_win_rate(),
            avg_win in valid_avg_win(),
            avg_loss in valid_avg_loss(),
            target_vol in valid_volatility(),
            current_vol in valid_volatility(),
            max_cap in valid_max_cap(),
        ) {
            let size = calculate_final_size(
                balance, risk_pct, entry, stop,
                win_rate, avg_win, avg_loss,
                target_vol, current_vol, max_cap,
            );
            prop_assert!(size <= max_cap + f64::EPSILON);
        }
    }

    // ── evaluate end-to-end property tests ──

    proptest! {
        #[test]
        fn evaluate_approved_iff_size_positive(
            balance in valid_balance(),
            risk_pct in valid_risk_percent(),
            entry in valid_entry_price(),
            win_rate in valid_win_rate(),
            avg_win in valid_avg_win(),
            avg_loss in valid_avg_loss(),
            target_vol in valid_volatility(),
            current_vol in valid_volatility(),
            max_cap in valid_max_cap(),
        ) {
            let portfolio = InternalPortfolio {
                balance, win_rate, avg_win, avg_loss,
            };
            let order = InternalOrder {
                entry_price: entry,
                stop_price: entry * 0.95, // 5% stop distance
                asset_volatility: current_vol,
            };
            let params = RiskParams {
                risk_percent: risk_pct,
                target_vol,
                max_position_cap: max_cap,
            };
            let result = evaluate(&portfolio, &order, &params, "test-id".into());
            prop_assert_eq!(result.is_approved, result.size > 0.0);
            if !result.is_approved {
                prop_assert_eq!(result.reason, Some(RiskError::RiskThresholdViolation));
            } else {
                prop_assert_eq!(result.reason, None);
            }
        }

        #[test]
        fn evaluate_request_id_roundtrips(
            balance in valid_balance(),
            risk_pct in valid_risk_percent(),
            entry in valid_entry_price(),
            win_rate in valid_win_rate(),
            avg_win in valid_avg_win(),
            avg_loss in valid_avg_loss(),
        ) {
            let portfolio = InternalPortfolio { balance, win_rate, avg_win, avg_loss };
            let order = InternalOrder {
                entry_price: entry,
                stop_price: entry * 0.95,
                asset_volatility: 1000.0,
            };
            let params = RiskParams {
                risk_percent: risk_pct,
                target_vol: 1000.0,
                max_position_cap: 10.0,
            };
            let result = evaluate(&portfolio, &order, &params, "req-42".into());
            prop_assert_eq!(result.request_id, "req-42");
        }
    }

    // ── Deterministic unit tests (edge cases) ──

    #[test]
    fn ff_zero_distance_returns_zero() {
        let size = fixed_fractional(10000.0, 2.0, 50000.0, 50000.0);
        assert_eq!(size, 0.0);
    }

    #[test]
    fn vol_adj_caps_at_double_edge() {
        // Extreme: target = 10000, current = 1 → ratio = 10000, cap at 2.0
        let size = volatility_adjusted(1.0, 10000.0, 1.0);
        assert!((size - 2.0).abs() < f64::EPSILON * 10.0);
    }

    #[test]
    fn risk_error_display_non_empty() {
        for err in &[
            RiskError::RequestIdEmpty,
            RiskError::BalanceZero,
            RiskError::EntryPriceZero,
            RiskError::StopDistanceZero,
            RiskError::InvalidVolatility,
            RiskError::PortfolioDecodeError,
            RiskError::OrderDecodeError,
            RiskError::RiskThresholdViolation,
        ] {
            let msg = format!("{}", err);
            assert!(!msg.is_empty(), "Display empty for {:?}", err);
        }
    }

    #[test]
    fn risk_result_uses_typed_error() {
        let result = RiskResult {
            request_id: "test".into(),
            is_approved: false,
            size: 0.0,
            reason: Some(RiskError::RiskThresholdViolation),
        };
        assert!(result.reason.is_some());
        assert_eq!(result.reason.unwrap(), RiskError::RiskThresholdViolation);
    }

    #[test]
    fn risk_result_approved_has_no_error() {
        let result = RiskResult {
            request_id: "test".into(),
            is_approved: true,
            size: 1.0,
            reason: None,
        };
        assert!(result.is_approved);
        assert!(result.reason.is_none());
    }
}
