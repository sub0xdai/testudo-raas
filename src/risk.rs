// RISK MATHEMATICS: PURE FUNCTIONS
// NO I/O. NO ASYNC. HIGH-PERFORMANCE.

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
    pub reason: String,
}

pub fn evaluate(
    portfolio: &InternalPortfolio,
    order: &InternalOrder,
    params: &RiskParams,
    request_id: String,
) -> RiskResult {
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

    RiskResult {
        request_id,
        is_approved: final_size > 0.0,
        size: final_size,
        reason: if final_size > 0.0 {
            String::new()
        } else {
            "RISK_THRESHOLD_VIOLATION".to_string()
        },
    }
}

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
    let mut min_size = max_position_cap;

    let ff_size = fixed_fractional(balance, risk_percent, entry_price, stop_price);
    if ff_size > 0.0 {
        min_size = min_size.min(ff_size);
    }

    let kelly_frac = kelly_criterion(win_rate, avg_win, avg_loss);
    if kelly_frac > 0.0 && entry_price > 0.0 {
        let kelly_size = (balance * (kelly_frac / 2.0)) / entry_price;
        min_size = min_size.min(kelly_size);
    }

    if current_vol > 0.0 && target_vol > 0.0 && ff_size > 0.0 {
        let vol_size = volatility_adjusted(ff_size, target_vol, current_vol);
        min_size = min_size.min(vol_size);
    }

    min_size.max(0.0)
}

pub fn fixed_fractional(balance: f64, risk_percent: f64, entry_price: f64, stop_price: f64) -> f64 {
    let stop_distance = (entry_price - stop_price).abs();
    if stop_distance <= 0.0 { return 0.0; }
    (balance * (risk_percent / 100.0)) / stop_distance
}

pub fn kelly_criterion(win_rate: f64, avg_win: f64, avg_loss: f64) -> f64 {
    if avg_loss <= 0.0 || avg_win <= 0.0 { return 0.0; }
    let win_rate = win_rate.clamp(0.0, 1.0);
    let win_loss_ratio = avg_win / avg_loss;
    (win_rate - ((1.0 - win_rate) / win_loss_ratio)).max(0.0)
}

pub fn volatility_adjusted(base_size: f64, target_vol: f64, current_vol: f64) -> f64 {
    if current_vol <= 0.0 || target_vol <= 0.0 { return base_size; }
    base_size * (target_vol / current_vol).min(2.0)
}
