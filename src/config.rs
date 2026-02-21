use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Utc};
use error_stack::{Report, ResultExt};
use serde::Deserialize;

use crate::error::ConfigError;
use crate::model::TimeFrame;

fn default_log_level() -> String {
    "info".into()
}

fn default_log_format() -> String {
    "text".into()
}

fn default_data_dir() -> String {
    "./data".into()
}

fn default_historical_candles() -> usize {
    500
}

fn default_cooldown_minutes() -> u64 {
    5
}

fn default_true() -> bool {
    true
}

fn default_initial_capital() -> f64 {
    1_000_000.0
}

fn default_entry_size_percent() -> f64 {
    10.0
}

fn default_slippage_bps() -> f64 {
    10.0
}

fn default_backtest_max_entries() -> usize {
    3
}

fn default_cooldown_bars() -> usize {
    3
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub exchanges: Vec<ExchangeConfig>,
    #[serde(default)]
    pub coins: Vec<CoinConfig>,
    #[serde(default)]
    pub alerts: Vec<AlertConfig>,
    #[serde(default)]
    pub inputs: Vec<InputConfig>,
    #[serde(default)]
    pub models: Vec<TradingModelConfig>,
    pub backtest: Option<BacktestConfig>,
    #[serde(default)]
    pub live: LiveConfig,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_historical_candles")]
    pub historical_candles: usize,
    #[serde(default = "default_cooldown_minutes")]
    pub default_cooldown_minutes: u64,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeConfig {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[allow(dead_code)]
    pub base_url: String,
    #[allow(dead_code)]
    pub ws_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CoinConfig {
    pub exchange: String,
    pub symbol: String,
    pub timeframes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AlertConfig {
    pub name: String,
    pub exchange: String,
    pub symbol: String,
    pub indicator: String,
    #[serde(default)]
    pub params: toml::Table,
    pub condition: String,
    pub threshold: Option<f64>,
    pub cooldown_minutes: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct InputConfig {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub params: toml::Table,
}

#[derive(Debug, Deserialize)]
pub struct TradingModelConfig {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub params: toml::Table,
}

#[derive(Debug, Deserialize)]
pub struct BacktestConfig {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub model: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    #[serde(default = "default_initial_capital")]
    pub initial_capital: f64,
    #[serde(default = "default_entry_size_percent")]
    pub entry_size_percent: f64,
    #[serde(default)]
    pub costs: BacktestCostConfig,
    #[serde(default)]
    pub risk: RiskPolicyConfig,
}

#[derive(Debug, Deserialize)]
pub struct BacktestCostConfig {
    #[serde(default = "default_slippage_bps")]
    pub slippage_bps: f64,
    #[serde(default)]
    pub fee_bps_overrides: HashMap<String, f64>,
}

impl Default for BacktestCostConfig {
    fn default() -> Self {
        Self {
            slippage_bps: default_slippage_bps(),
            fee_bps_overrides: HashMap::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RiskPolicyConfig {
    #[serde(default = "default_backtest_max_entries")]
    pub max_entries_per_position: usize,
    #[serde(default = "default_cooldown_bars")]
    pub cooldown_bars: usize,
}

impl Default for RiskPolicyConfig {
    fn default() -> Self {
        Self {
            max_entries_per_position: default_backtest_max_entries(),
            cooldown_bars: default_cooldown_bars(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct LiveConfig {
    #[serde(default)]
    pub risk: LiveRiskConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct LiveRiskConfig {
    pub max_entries_per_position: Option<usize>,
}

pub fn load(path: &Path) -> Result<AppConfig, Report<ConfigError>> {
    let content = std::fs::read_to_string(path)
        .change_context(ConfigError::ReadFile)
        .attach_with(|| format!("path: {}", path.display()))?;

    let config: AppConfig = toml::from_str(&content).change_context(ConfigError::Parse {
        reason: "invalid TOML syntax or schema mismatch".into(),
    })?;

    validate(&config)?;
    Ok(config)
}

const VALID_CONDITIONS: &[&str] = &["above", "below", "cross_above", "cross_below", "between"];

fn validate(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    validate_timeframes(config)?;
    validate_coin_exchanges(config)?;
    validate_alert_references(config)?;
    validate_alert_names_unique(config)?;
    validate_alert_conditions(config)?;
    validate_input_and_model_names(config)?;
    validate_model_input_references(config)?;
    validate_backtest(config)?;
    Ok(())
}

fn validate_timeframes(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    for coin in &config.coins {
        for tf in &coin.timeframes {
            if TimeFrame::from_str(tf).is_none() {
                return Err(Report::new(ConfigError::Validation {
                    field: format!(
                        "coins[exchange={}, symbol={}].timeframes: unknown timeframe \"{}\"",
                        coin.exchange, coin.symbol, tf
                    ),
                }));
            }
        }
    }
    Ok(())
}

fn validate_coin_exchanges(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    let exchange_names: HashSet<&str> = config.exchanges.iter().map(|e| e.name.as_str()).collect();

    for coin in &config.coins {
        if !exchange_names.contains(coin.exchange.as_str()) {
            return Err(Report::new(ConfigError::Validation {
                field: format!(
                    "coins[symbol={}].exchange \"{}\" does not match any exchange name",
                    coin.symbol, coin.exchange
                ),
            }));
        }
    }
    Ok(())
}

fn validate_alert_references(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    for alert in &config.alerts {
        let found = config
            .coins
            .iter()
            .any(|c| c.exchange == alert.exchange && c.symbol == alert.symbol);

        if !found {
            return Err(Report::new(ConfigError::Validation {
                field: format!(
                    "alerts[\"{}\"].exchange+symbol ({}, {}) does not match any coin entry",
                    alert.name, alert.exchange, alert.symbol
                ),
            }));
        }
    }
    Ok(())
}

fn validate_alert_names_unique(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    let mut seen = HashSet::new();
    for alert in &config.alerts {
        if !seen.insert(alert.name.as_str()) {
            return Err(Report::new(ConfigError::Validation {
                field: format!("alerts: duplicate name \"{}\"", alert.name),
            }));
        }
    }
    Ok(())
}

fn validate_alert_conditions(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    for alert in &config.alerts {
        if !VALID_CONDITIONS.contains(&alert.condition.as_str()) {
            return Err(Report::new(ConfigError::Validation {
                field: format!(
                    "alerts[\"{}\"].condition \"{}\" is not valid",
                    alert.name, alert.condition
                ),
            }));
        }

        let threshold_required = matches!(
            alert.condition.as_str(),
            "above" | "below" | "cross_above" | "cross_below"
        );

        if threshold_required && alert.threshold.is_none() {
            return Err(Report::new(ConfigError::Validation {
                field: format!(
                    "alerts[\"{}\"].threshold is required for condition \"{}\"",
                    alert.name, alert.condition
                ),
            }));
        }
    }
    Ok(())
}

fn validate_input_and_model_names(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    let mut input_names = HashSet::new();
    for input in &config.inputs {
        if !input_names.insert(input.name.as_str()) {
            return Err(Report::new(ConfigError::Validation {
                field: format!("inputs: duplicate name \"{}\"", input.name),
            }));
        }
    }

    let mut model_names = HashSet::new();
    for model in &config.models {
        if !model_names.insert(model.name.as_str()) {
            return Err(Report::new(ConfigError::Validation {
                field: format!("models: duplicate name \"{}\"", model.name),
            }));
        }
    }

    Ok(())
}

fn validate_model_input_references(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    if config.inputs.is_empty() || config.models.is_empty() {
        return Ok(());
    }

    let input_names: HashSet<&str> = config
        .inputs
        .iter()
        .map(|input| input.name.as_str())
        .collect();
    for model in &config.models {
        for input_name in &model.inputs {
            if !input_names.contains(input_name.as_str()) {
                return Err(Report::new(ConfigError::Validation {
                    field: format!(
                        "models[\"{}\"].inputs contains unknown input \"{}\"",
                        model.name, input_name
                    ),
                }));
            }
        }
    }
    Ok(())
}

fn validate_backtest(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    let Some(backtest) = &config.backtest else {
        return Ok(());
    };

    if TimeFrame::from_str(&backtest.timeframe).is_none() {
        return Err(Report::new(ConfigError::Validation {
            field: format!("backtest.timeframe \"{}\" is not valid", backtest.timeframe),
        }));
    }

    if backtest.start_time >= backtest.end_time {
        return Err(Report::new(ConfigError::Validation {
            field: "backtest.start_time must be before backtest.end_time".into(),
        }));
    }

    if backtest.entry_size_percent <= 0.0 || backtest.entry_size_percent > 100.0 {
        return Err(Report::new(ConfigError::Validation {
            field: "backtest.entry_size_percent must be in (0, 100]".into(),
        }));
    }

    if backtest.initial_capital <= 0.0 {
        return Err(Report::new(ConfigError::Validation {
            field: "backtest.initial_capital must be > 0".into(),
        }));
    }

    if backtest.costs.slippage_bps < 0.0 {
        return Err(Report::new(ConfigError::Validation {
            field: "backtest.costs.slippage_bps must be >= 0".into(),
        }));
    }

    if !config.models.is_empty() && !config.models.iter().any(|m| m.name == backtest.model) {
        return Err(Report::new(ConfigError::Validation {
            field: format!("backtest.model \"{}\" is not defined", backtest.model),
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> AppConfig {
        toml::from_str(toml).expect("parse failed")
    }

    #[test]
    fn defaults_applied_when_fields_omitted() {
        let toml = r#"
[general]
"#;
        let config = parse(toml);
        assert_eq!(config.general.log_level, "info");
        assert_eq!(config.general.log_format, "text");
        assert_eq!(config.general.data_dir, "./data");
        assert_eq!(config.general.historical_candles, 500);
        assert_eq!(config.general.default_cooldown_minutes, 5);
        assert!(config.exchanges.is_empty());
        assert!(config.coins.is_empty());
        assert!(config.alerts.is_empty());
        assert!(config.inputs.is_empty());
        assert!(config.models.is_empty());
        assert!(config.backtest.is_none());
        assert!(config.live.risk.max_entries_per_position.is_none());
    }

    #[test]
    fn invalid_timeframe_string_rejected() {
        let toml = r#"
[general]

[[exchanges]]
name = "upbit"
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[coins]]
exchange = "upbit"
symbol = "KRW-BTC"
timeframes = ["2m"]
"#;
        let config = parse(toml);
        assert!(validate(&config).is_err());
    }

    #[test]
    fn duplicate_input_name_rejected() {
        let toml = r#"
[general]

[[inputs]]
name = "rsi_14"
kind = "rsi"

[[inputs]]
name = "rsi_14"
kind = "rsi"
"#;
        let config = parse(toml);
        assert!(validate(&config).is_err());
    }

    #[test]
    fn backtest_defaults_applied() {
        let toml = r#"
[general]

[[models]]
name = "rsi-reversion"
kind = "rsi_reversion"

[backtest]
exchange = "upbit"
symbol = "KRW-BTC"
timeframe = "1m"
model = "rsi-reversion"
start_time = "2025-01-01T00:00:00Z"
end_time = "2025-01-02T00:00:00Z"
"#;
        let config = parse(toml);
        let backtest = config.backtest.unwrap();
        assert_eq!(backtest.entry_size_percent, 10.0);
        assert_eq!(backtest.costs.slippage_bps, 10.0);
        assert_eq!(backtest.risk.cooldown_bars, 3);
    }
}
