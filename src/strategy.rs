pub mod condition;

use crate::config::{AlertConfig, AppConfig};
use crate::model::ExchangeKind;

#[derive(Debug, Clone)]
pub enum ConditionType {
    Above(f64),
    Below(f64),
    CrossAbove(f64),
    CrossBelow(f64),
    Between { low: f64, high: f64 },
}

#[derive(Debug, Clone)]
pub struct IndicatorParams {
    pub period: Option<usize>,
    pub fast_period: Option<usize>,
    pub slow_period: Option<usize>,
    pub signal_period: Option<usize>,
    pub std_dev_multiplier: Option<f64>,
    #[allow(dead_code)]
    pub surge_multiplier: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct AlertRule {
    pub name: String,
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub indicator_name: String,
    pub indicator_params: IndicatorParams,
    pub condition: ConditionType,
    pub cooldown_minutes: u64,
}

impl AlertRule {
    /// Build all `AlertRule`s from a validated `AppConfig`.
    pub fn from_config(config: &AppConfig) -> Vec<Self> {
        config
            .alerts
            .iter()
            .filter_map(|alert| build_rule(alert, config.general.default_cooldown_minutes))
            .collect()
    }
}

fn build_rule(alert: &AlertConfig, default_cooldown: u64) -> Option<AlertRule> {
    let exchange = match alert.exchange.as_str() {
        "upbit" => ExchangeKind::Upbit,
        "binance" => ExchangeKind::Binance,
        _ => return None,
    };

    let condition = parse_condition(alert)?;
    let params = parse_indicator_params(alert);
    let cooldown = alert.cooldown_minutes.unwrap_or(default_cooldown);

    Some(AlertRule {
        name: alert.name.clone(),
        exchange,
        symbol: alert.symbol.clone(),
        indicator_name: alert.indicator.clone(),
        indicator_params: params,
        condition,
        cooldown_minutes: cooldown,
    })
}

fn parse_condition(alert: &AlertConfig) -> Option<ConditionType> {
    match alert.condition.as_str() {
        "above" => Some(ConditionType::Above(alert.threshold?)),
        "below" => Some(ConditionType::Below(alert.threshold?)),
        "cross_above" => Some(ConditionType::CrossAbove(alert.threshold?)),
        "cross_below" => Some(ConditionType::CrossBelow(alert.threshold?)),
        "between" => {
            let low = alert.threshold?;
            let high = alert
                .params
                .get("threshold_high")
                .and_then(|v| v.as_float())?;
            Some(ConditionType::Between { low, high })
        }
        _ => None,
    }
}

fn parse_indicator_params(alert: &AlertConfig) -> IndicatorParams {
    let get_usize = |key: &str| -> Option<usize> {
        alert
            .params
            .get(key)
            .and_then(|v| v.as_integer())
            .map(|n| n as usize)
    };
    let get_f64 = |key: &str| -> Option<f64> { alert.params.get(key).and_then(|v| v.as_float()) };

    IndicatorParams {
        period: get_usize("period"),
        fast_period: get_usize("fast_period"),
        slow_period: get_usize("slow_period"),
        signal_period: get_usize("signal_period"),
        std_dev_multiplier: get_f64("std_dev_multiplier"),
        surge_multiplier: get_f64("surge_multiplier"),
    }
}
