use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use error_stack::{Report, ResultExt};
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::ExchangeError;
use crate::exchange::Exchange;
use crate::model::{Candle, ExchangeKind, Ticker, TradeSide, TimeFrame, Trade};

const UPBIT_BASE_URL: &str = "https://api.upbit.com";
const UPBIT_WS_URL: &str = "wss://api.upbit.com/websocket/v1";
const MAX_CANDLES_PER_REQUEST: usize = 200;
const WS_PING_INTERVAL_SECS: u64 = 60;
const MAX_BACKOFF_SECS: u64 = 60;
/// Upbit allows 10 req/s; use 8 for safety margin
const UPBIT_REQUESTS_PER_SECOND: u32 = 8;

pub struct UpbitExchange {
    client: reqwest::Client,
    rate_limiter: Arc<DefaultDirectRateLimiter>,
}

impl UpbitExchange {
    pub fn new() -> Self {
        let quota = Quota::per_second(NonZeroU32::new(UPBIT_REQUESTS_PER_SECOND).unwrap());
        Self {
            client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
        }
    }

    async fn fetch_candles_page(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        count: usize,
        to: Option<DateTime<Utc>>,
    ) -> Result<Vec<UpbitCandle>, Report<ExchangeError>> {
        // Wait for rate limiter before making the request
        self.rate_limiter.until_ready().await;

        let endpoint = timeframe.upbit_endpoint();
        let url = format!("{}{}", UPBIT_BASE_URL, endpoint);

        let mut params = vec![
            ("market".to_owned(), symbol.to_owned()),
            ("count".to_owned(), count.to_string()),
        ];
        if let Some(to_dt) = to {
            params.push((
                "to".to_owned(),
                to_dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
            ));
        }

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .change_context(ExchangeError::Request {
                exchange: "upbit".into(),
            })?;

        if !response.status().is_success() {
            return Err(Report::new(ExchangeError::Request {
                exchange: "upbit".into(),
            })
            .attach(format!("HTTP status: {}", response.status())));
        }

        let candles: Vec<UpbitCandle> = response
            .json()
            .await
            .change_context(ExchangeError::ResponseParse {
                exchange: "upbit".into(),
            })?;

        Ok(candles)
    }
}

impl Default for UpbitExchange {
    fn default() -> Self {
        Self::new()
    }
}

impl Exchange for UpbitExchange {
    fn kind(&self) -> ExchangeKind {
        ExchangeKind::Upbit
    }

    fn fetch_candles(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<ExchangeError>>> {
        let symbol = symbol.to_owned();
        Box::pin(async move {
            let mut all_candles: Vec<Candle> = Vec::with_capacity(limit);
            let mut to: Option<DateTime<Utc>> = None;
            let mut remaining = limit;

            while remaining > 0 {
                let count = remaining.min(MAX_CANDLES_PER_REQUEST);
                let page = self
                    .fetch_candles_page(&symbol, timeframe, count, to)
                    .await?;

                if page.is_empty() {
                    break;
                }

                let oldest_time = page.last().and_then(|c| {
                    DateTime::parse_from_rfc3339(&c.candle_date_time_utc)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                });

                let fetched = page.len();
                for raw in page {
                    all_candles.push(raw.into_candle(&symbol, timeframe));
                }

                remaining = remaining.saturating_sub(fetched);

                if fetched < count {
                    break;
                }

                to = oldest_time;

                info!(
                    symbol = %symbol,
                    timeframe = %timeframe,
                    fetched = all_candles.len(),
                    total = limit,
                    "upbit candle fetch progress"
                );
            }

            // Upbit returns newest-first; reverse to oldest-first
            all_candles.reverse();
            Ok(all_candles)
        })
    }

    fn subscribe_ticker(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Ticker>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>> {
        let symbols = symbols.to_vec();
        Box::pin(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                match run_ticker_ws(&symbols, &tx, &cancel).await {
                    Ok(()) => break,
                    Err(e) => {
                        warn!(error = %e, "upbit ticker ws disconnected, retrying...");
                        sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(MAX_BACKOFF_SECS));
                    }
                }
            }
            Ok(())
        })
    }

    fn subscribe_trades(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Trade>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>> {
        let symbols = symbols.to_vec();
        Box::pin(async move {
            let mut backoff = Duration::from_secs(1);

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                match run_trades_ws(&symbols, &tx, &cancel).await {
                    Ok(()) => break,
                    Err(e) => {
                        warn!(error = %e, "upbit trades ws disconnected, retrying...");
                        sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(MAX_BACKOFF_SECS));
                    }
                }
            }
            Ok(())
        })
    }
}

async fn run_ticker_ws(
    symbols: &[String],
    tx: &mpsc::Sender<Ticker>,
    cancel: &CancellationToken,
) -> Result<(), Report<ExchangeError>> {
    // Use connect_async with URL string so tungstenite auto-generates
    // the required WebSocket handshake headers (sec-websocket-key, etc.).
    // Do NOT include an Origin header — it triggers Upbit's strict 1 req/10s limit.
    let (ws_stream, _) = connect_async(UPBIT_WS_URL)
        .await
        .change_context(ExchangeError::Connection {
            exchange: "upbit".into(),
        })?;

    let (mut write, mut read) = ws_stream.split();

    // Send subscription message
    let codes: Vec<String> = symbols.to_vec();
    let subscribe_msg = build_ticker_subscribe(&codes);
    write
        .send(Message::Text(subscribe_msg.into()))
        .await
        .change_context(ExchangeError::Connection {
            exchange: "upbit".into(),
        })?;

    info!(symbols = ?symbols, "upbit ticker ws subscribed");

    let ping_interval = Duration::from_secs(WS_PING_INTERVAL_SECS);
    let mut ping_timer = tokio::time::interval(ping_interval);
    ping_timer.tick().await; // skip immediate first tick

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!("upbit ticker ws cancelled");
                break;
            }
            _ = ping_timer.tick() => {
                write.send(Message::Ping(vec![].into())).await
                    .change_context(ExchangeError::Connection { exchange: "upbit".into() })?;
            }
            msg = read.next() => {
                match msg {
                    None => break,
                    Some(Err(e)) => return Err(Report::new(e)
                        .change_context(ExchangeError::Connection {
                            exchange: "upbit".into(),
                        })),
                    Some(Ok(Message::Binary(data))) => {
                        match serde_json::from_slice::<UpbitTickerMsg>(&data) {
                            Ok(raw) => {
                                let ticker = raw.into_ticker();
                                let _ = tx.send(ticker).await;
                            }
                            Err(e) => {
                                warn!(error = %e, "upbit ticker parse error");
                            }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
async fn run_trades_ws(
    symbols: &[String],
    tx: &mpsc::Sender<Trade>,
    cancel: &CancellationToken,
) -> Result<(), Report<ExchangeError>> {
    let (ws_stream, _) = connect_async(UPBIT_WS_URL)
        .await
        .change_context(ExchangeError::Connection {
            exchange: "upbit".into(),
        })?;

    let (mut write, mut read) = ws_stream.split();

    let codes: Vec<String> = symbols.to_vec();
    let subscribe_msg = build_trades_subscribe(&codes);
    write
        .send(Message::Text(subscribe_msg.into()))
        .await
        .change_context(ExchangeError::Connection {
            exchange: "upbit".into(),
        })?;

    info!(symbols = ?symbols, "upbit trades ws subscribed");

    let ping_interval = Duration::from_secs(WS_PING_INTERVAL_SECS);
    let mut ping_timer = tokio::time::interval(ping_interval);
    ping_timer.tick().await;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!("upbit trades ws cancelled");
                break;
            }
            _ = ping_timer.tick() => {
                write.send(Message::Ping(vec![].into())).await
                    .change_context(ExchangeError::Connection { exchange: "upbit".into() })?;
            }
            msg = read.next() => {
                match msg {
                    None => break,
                    Some(Err(e)) => return Err(Report::new(e)
                        .change_context(ExchangeError::Connection {
                            exchange: "upbit".into(),
                        })),
                    Some(Ok(Message::Binary(data))) => {
                        match serde_json::from_slice::<UpbitTradeMsg>(&data) {
                            Ok(raw) => {
                                let trade = raw.into_trade();
                                let _ = tx.send(trade).await;
                            }
                            Err(e) => {
                                warn!(error = %e, "upbit trade parse error");
                            }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    Ok(())
}

fn build_ticker_subscribe(codes: &[String]) -> String {
    let ticket = Uuid::new_v4().to_string();
    let codes_json: Vec<serde_json::Value> = codes
        .iter()
        .map(|c| serde_json::Value::String(c.clone()))
        .collect();

    serde_json::json!([
        { "ticket": ticket },
        {
            "type": "ticker",
            "codes": codes_json,
            "is_only_realtime": true
        },
        { "format": "DEFAULT" }
    ])
    .to_string()
}

#[allow(dead_code)]
fn build_trades_subscribe(codes: &[String]) -> String {
    let ticket = Uuid::new_v4().to_string();
    let codes_json: Vec<serde_json::Value> = codes
        .iter()
        .map(|c| serde_json::Value::String(c.clone()))
        .collect();

    serde_json::json!([
        { "ticket": ticket },
        {
            "type": "trade",
            "codes": codes_json,
            "is_only_realtime": true
        },
        { "format": "DEFAULT" }
    ])
    .to_string()
}

// ── REST response types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct UpbitCandle {
    candle_date_time_utc: String,
    opening_price: f64,
    high_price: f64,
    low_price: f64,
    trade_price: f64,
    candle_acc_trade_volume: f64,
}

impl UpbitCandle {
    fn into_candle(self, symbol: &str, timeframe: TimeFrame) -> Candle {
        let open_time = DateTime::parse_from_rfc3339(&self.candle_date_time_utc)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Candle {
            exchange: ExchangeKind::Upbit,
            symbol: symbol.to_owned(),
            timeframe,
            open_time,
            open: self.opening_price,
            high: self.high_price,
            low: self.low_price,
            close: self.trade_price,
            volume: self.candle_acc_trade_volume,
        }
    }
}

// ── WebSocket message types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct UpbitTickerMsg {
    code: String,
    trade_price: f64,
    acc_trade_volume_24h: f64,
    timestamp: i64,
}

impl UpbitTickerMsg {
    fn into_ticker(self) -> Ticker {
        let timestamp = DateTime::from_timestamp_millis(self.timestamp).unwrap_or_else(Utc::now);
        Ticker {
            exchange: ExchangeKind::Upbit,
            symbol: self.code,
            price: self.trade_price,
            volume: self.acc_trade_volume_24h,
            timestamp,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct UpbitTradeMsg {
    code: String,
    trade_price: f64,
    trade_volume: f64,
    ask_bid: String,
    timestamp: i64,
}

impl UpbitTradeMsg {
    fn into_trade(self) -> Trade {
        let timestamp = DateTime::from_timestamp_millis(self.timestamp).unwrap_or_else(Utc::now);
        let side = if self.ask_bid == "BID" {
            TradeSide::Buy
        } else {
            TradeSide::Sell
        };

        Trade {
            exchange: ExchangeKind::Upbit,
            symbol: self.code,
            price: self.trade_price,
            volume: self.trade_volume,
            side,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ticker_subscribe_contains_codes() {
        let codes = vec!["KRW-BTC".to_owned(), "KRW-ETH".to_owned()];
        let msg = build_ticker_subscribe(&codes);
        assert!(msg.contains("ticker"));
        assert!(msg.contains("KRW-BTC"));
        assert!(msg.contains("KRW-ETH"));
    }

    #[test]
    fn build_trades_subscribe_contains_codes() {
        let codes = vec!["KRW-BTC".to_owned()];
        let msg = build_trades_subscribe(&codes);
        assert!(msg.contains("trade"));
        assert!(msg.contains("KRW-BTC"));
    }

    #[test]
    fn upbit_candle_parses_into_candle() {
        let raw = UpbitCandle {
            candle_date_time_utc: "2024-01-01T00:00:00".to_owned(),
            opening_price: 50000.0,
            high_price: 51000.0,
            low_price: 49000.0,
            trade_price: 50500.0,
            candle_acc_trade_volume: 10.5,
        };
        let candle = raw.into_candle("KRW-BTC", TimeFrame::Min1);
        assert_eq!(candle.exchange, ExchangeKind::Upbit);
        assert_eq!(candle.symbol, "KRW-BTC");
        assert_eq!(candle.open, 50000.0);
        assert_eq!(candle.close, 50500.0);
        assert_eq!(candle.volume, 10.5);
    }

    #[test]
    fn upbit_ticker_msg_parses_side() {
        let msg = UpbitTradeMsg {
            code: "KRW-BTC".to_owned(),
            trade_price: 50000.0,
            trade_volume: 0.1,
            ask_bid: "BID".to_owned(),
            timestamp: 1704067200000,
        };
        let trade = msg.into_trade();
        assert_eq!(trade.side, TradeSide::Buy);

        let msg2 = UpbitTradeMsg {
            code: "KRW-BTC".to_owned(),
            trade_price: 50000.0,
            trade_volume: 0.1,
            ask_bid: "ASK".to_owned(),
            timestamp: 1704067200000,
        };
        let trade2 = msg2.into_trade();
        assert_eq!(trade2.side, TradeSide::Sell);
    }

    /// Integration test: requires network access. Run with `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn integration_fetch_candles() {
        let exchange = UpbitExchange::new();
        let candles = exchange
            .fetch_candles("KRW-BTC", TimeFrame::Min1, 10)
            .await
            .unwrap();
        assert!(!candles.is_empty());
        assert!(candles.len() <= 10);
    }

    /// Integration test: requires network access. Run with `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn integration_subscribe_ticker() {
        let exchange = UpbitExchange::new();
        let (tx, mut rx) = mpsc::channel(10);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            exchange
                .subscribe_ticker(&["KRW-BTC".to_owned()], tx, cancel_clone)
                .await
                .unwrap();
        });

        // Wait for at least one ticker
        let ticker = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert_eq!(ticker.exchange, ExchangeKind::Upbit);
        assert_eq!(ticker.symbol, "KRW-BTC");
        cancel.cancel();
    }
}
