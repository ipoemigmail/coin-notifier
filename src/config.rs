use std::path::Path;

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

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub exchanges: Vec<ExchangeConfig>,
    #[serde(default)]
    pub coins: Vec<CoinConfig>,
    #[serde(default)]
    pub alerts: Vec<AlertConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Accepted values: `"text"` | `"json"`
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
    // Reserved for future use (custom base URL / WebSocket URL overrides)
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

/// Load and validate an `AppConfig` from a TOML file at `path`.
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
const THRESHOLD_REQUIRED_CONDITIONS: &[&str] = &["above", "below"];

fn validate(config: &AppConfig) -> Result<(), Report<ConfigError>> {
    validate_timeframes(config)?;
    validate_coin_exchanges(config)?;
    validate_alert_references(config)?;
    validate_alert_names_unique(config)?;
    validate_alert_conditions(config)?;
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
    let exchange_names: std::collections::HashSet<&str> =
        config.exchanges.iter().map(|e| e.name.as_str()).collect();

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
    let mut seen = std::collections::HashSet::new();
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

        if THRESHOLD_REQUIRED_CONDITIONS.contains(&alert.condition.as_str())
            && alert.threshold.is_none()
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> AppConfig {
        toml::from_str(toml).expect("parse failed")
    }

    #[test]
    fn valid_full_config_parses() {
        let toml = r#"
[general]
log_level = "debug"
log_format = "json"
data_dir = "/tmp/data"
historical_candles = 200
default_cooldown_minutes = 10

[[exchanges]]
name = "upbit"
enabled = true
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[coins]]
exchange = "upbit"
symbol = "KRW-BTC"
timeframes = ["1m", "5m"]

[[alerts]]
name = "BTC RSI oversold"
exchange = "upbit"
symbol = "KRW-BTC"
indicator = "rsi"
params = { period = 14 }
condition = "below"
threshold = 30.0
cooldown_minutes = 10
"#;
        let config = parse(toml);
        assert_eq!(config.general.log_level, "debug");
        assert_eq!(config.exchanges.len(), 1);
        assert_eq!(config.coins.len(), 1);
        assert_eq!(config.alerts.len(), 1);
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
    }

    #[test]
    fn invalid_exchange_reference_rejected() {
        let toml = r#"
[general]

[[exchanges]]
name = "upbit"
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[coins]]
exchange = "binance"
symbol = "BTCUSDT"
timeframes = ["1m"]
"#;
        let config = parse(toml);
        let result = validate(&config);
        assert!(result.is_err());
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
        let result = validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_alert_names_rejected() {
        let toml = r#"
[general]

[[exchanges]]
name = "upbit"
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[coins]]
exchange = "upbit"
symbol = "KRW-BTC"
timeframes = ["1m"]

[[alerts]]
name = "dup"
exchange = "upbit"
symbol = "KRW-BTC"
indicator = "rsi"
condition = "below"
threshold = 30.0

[[alerts]]
name = "dup"
exchange = "upbit"
symbol = "KRW-BTC"
indicator = "rsi"
condition = "above"
threshold = 70.0
"#;
        let config = parse(toml);
        let result = validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn threshold_required_for_above_below_conditions() {
        let toml = r#"
[general]

[[exchanges]]
name = "upbit"
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[coins]]
exchange = "upbit"
symbol = "KRW-BTC"
timeframes = ["1m"]

[[alerts]]
name = "missing threshold"
exchange = "upbit"
symbol = "KRW-BTC"
indicator = "rsi"
condition = "above"
"#;
        let config = parse(toml);
        let result = validate(&config);
        assert!(result.is_err());
    }
}
