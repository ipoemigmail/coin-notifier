use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExchangeKind {
    Upbit,
    Binance,
}

impl fmt::Display for ExchangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Upbit => write!(f, "upbit"),
            Self::Binance => write!(f, "binance"),
        }
    }
}

/// Candle timeframe supported by the application.
///
/// String representations match the config file format (e.g. `"1m"`, `"1h"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeFrame {
    Min1,
    Min3,
    Min5,
    Min15,
    Min30,
    Hour1,
    Hour4,
    Day1,
}

impl TimeFrame {
    /// Parse a config-format string into a `TimeFrame`.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "1m" => Some(Self::Min1),
            "3m" => Some(Self::Min3),
            "5m" => Some(Self::Min5),
            "15m" => Some(Self::Min15),
            "30m" => Some(Self::Min30),
            "1h" => Some(Self::Hour1),
            "4h" => Some(Self::Hour4),
            "1d" => Some(Self::Day1),
            _ => None,
        }
    }

    /// Return the config-format string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Min1 => "1m",
            Self::Min3 => "3m",
            Self::Min5 => "5m",
            Self::Min15 => "15m",
            Self::Min30 => "30m",
            Self::Hour1 => "1h",
            Self::Hour4 => "4h",
            Self::Day1 => "1d",
        }
    }

    /// Return the Upbit REST endpoint path segment for this timeframe.
    pub fn upbit_endpoint(self) -> &'static str {
        match self {
            Self::Min1 => "/v1/candles/minutes/1",
            Self::Min3 => "/v1/candles/minutes/3",
            Self::Min5 => "/v1/candles/minutes/5",
            Self::Min15 => "/v1/candles/minutes/15",
            Self::Min30 => "/v1/candles/minutes/30",
            Self::Hour1 => "/v1/candles/minutes/60",
            Self::Hour4 => "/v1/candles/minutes/240",
            Self::Day1 => "/v1/candles/days",
        }
    }

    /// Return the Binance kline interval string for this timeframe.
    pub fn binance_interval(self) -> &'static str {
        match self {
            Self::Min1 => "1m",
            Self::Min3 => "3m",
            Self::Min5 => "5m",
            Self::Min15 => "15m",
            Self::Min30 => "30m",
            Self::Hour1 => "1h",
            Self::Hour4 => "4h",
            Self::Day1 => "1d",
        }
    }
}

impl fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Candle {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub timeframe: TimeFrame,
    pub open_time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone)]
pub struct Ticker {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub price: f64,
    // Reserved for future analytics
    #[allow(dead_code)]
    pub volume: f64,
    #[allow(dead_code)]
    pub timestamp: DateTime<Utc>,
}

// TradeSide and Trade are used by subscribe_trades (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Trade {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub price: f64,
    pub volume: f64,
    pub side: TradeSide,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BacktestRun {
    pub run_id: String,
    pub model_name: String,
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub timeframe: TimeFrame,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub initial_capital: f64,
    pub final_equity: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub win_rate_pct: f64,
    pub trade_count: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct BacktestTrade {
    pub run_id: String,
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub gross_pnl: f64,
    pub net_pnl: f64,
    pub fee_paid: f64,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeframe_round_trip() {
        let frames = [
            ("1m", TimeFrame::Min1),
            ("3m", TimeFrame::Min3),
            ("5m", TimeFrame::Min5),
            ("15m", TimeFrame::Min15),
            ("30m", TimeFrame::Min30),
            ("1h", TimeFrame::Hour1),
            ("4h", TimeFrame::Hour4),
            ("1d", TimeFrame::Day1),
        ];
        for (s, tf) in frames {
            assert_eq!(TimeFrame::from_str(s), Some(tf));
            assert_eq!(tf.as_str(), s);
        }
    }

    #[test]
    fn timeframe_invalid_string_returns_none() {
        assert_eq!(TimeFrame::from_str("2m"), None);
        assert_eq!(TimeFrame::from_str(""), None);
    }

    #[test]
    fn exchange_kind_display() {
        assert_eq!(ExchangeKind::Upbit.to_string(), "upbit");
        assert_eq!(ExchangeKind::Binance.to_string(), "binance");
    }

    #[test]
    fn exchange_kind_serde_round_trip() {
        let json = serde_json::to_string(&ExchangeKind::Upbit).unwrap();
        let parsed: ExchangeKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ExchangeKind::Upbit);
    }

    #[test]
    fn trade_side_serde_round_trip() {
        let json = serde_json::to_string(&TradeSide::Buy).unwrap();
        let parsed: TradeSide = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TradeSide::Buy);
    }

    #[test]
    fn timeframe_serde_round_trip() {
        let json = serde_json::to_string(&TimeFrame::Hour4).unwrap();
        let parsed: TimeFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TimeFrame::Hour4);
    }
}
