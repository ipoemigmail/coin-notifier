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

use crate::error::ExchangeError;
use crate::exchange::Exchange;
use crate::model::{Candle, ExchangeKind, Ticker, TimeFrame, Trade, TradeSide};

const BINANCE_BASE_URL: &str = "https://api.binance.com";
const BINANCE_WS_BASE: &str = "wss://stream.binance.com:9443/stream";
const MAX_CANDLES_PER_REQUEST: usize = 1000;
// Reconnect before 24-hour auto-disconnect (23 hours)
const WS_RECONNECT_SECS: u64 = 23 * 60 * 60;
const MAX_BACKOFF_SECS: u64 = 60;
/// Binance kline endpoint costs weight 2; limit ~2500 req/min (5000 weight/min)
/// = ~40 req/s. Use 20 for safety margin.
const BINANCE_REQUESTS_PER_SECOND: u32 = 20;

pub struct BinanceExchange {
    client: reqwest::Client,
    rate_limiter: Arc<DefaultDirectRateLimiter>,
}

impl BinanceExchange {
    pub fn new() -> Self {
        let quota = Quota::per_second(NonZeroU32::new(BINANCE_REQUESTS_PER_SECOND).unwrap());
        Self {
            client: reqwest::Client::new(),
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
        }
    }
}

impl Default for BinanceExchange {
    fn default() -> Self {
        Self::new()
    }
}

impl Exchange for BinanceExchange {
    fn kind(&self) -> ExchangeKind {
        ExchangeKind::Binance
    }

    fn fetch_candles(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<ExchangeError>>> {
        let symbol = symbol.to_owned();
        Box::pin(async move {
            // Wait for rate limiter before making the request
            self.rate_limiter.until_ready().await;

            let url = format!("{}/api/v3/klines", BINANCE_BASE_URL);
            let interval = timeframe.binance_interval();
            let fetch_limit = limit.min(MAX_CANDLES_PER_REQUEST);

            let limit_str = fetch_limit.to_string();
            let params = [
                ("symbol", symbol.as_str()),
                ("interval", interval),
                ("limit", limit_str.as_str()),
            ];

            let response = self
                .client
                .get(&url)
                .query(&params)
                .send()
                .await
                .change_context(ExchangeError::Request {
                    exchange: "binance".into(),
                })?;

            if !response.status().is_success() {
                return Err(Report::new(ExchangeError::Request {
                    exchange: "binance".into(),
                })
                .attach(format!("HTTP status: {}", response.status())));
            }

            let raw: Vec<BinanceKlineRow> =
                response
                    .json()
                    .await
                    .change_context(ExchangeError::ResponseParse {
                        exchange: "binance".into(),
                    })?;

            info!(
                symbol = %symbol,
                timeframe = %timeframe,
                fetched = raw.len(),
                "binance candle fetch complete"
            );

            let candles = raw
                .into_iter()
                .map(|row: BinanceKlineRow| row.into_candle(&symbol, timeframe))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(candles)
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
                        warn!(error = %e, "binance ticker ws disconnected, retrying...");
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
                        warn!(error = %e, "binance trades ws disconnected, retrying...");
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
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@ticker", s.to_lowercase()))
        .collect();
    let ws_url = format!("{}?streams={}", BINANCE_WS_BASE, streams.join("/"));

    let (ws_stream, _) =
        connect_async(&ws_url)
            .await
            .change_context(ExchangeError::Connection {
                exchange: "binance".into(),
            })?;

    let (mut write, mut read) = ws_stream.split();

    info!(symbols = ?symbols, "binance ticker ws connected");

    // Reconnect after 23h to avoid Binance's 24h auto-disconnect
    let reconnect_timer = tokio::time::sleep(Duration::from_secs(WS_RECONNECT_SECS));
    tokio::pin!(reconnect_timer);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!("binance ticker ws cancelled");
                break;
            }
            _ = &mut reconnect_timer => {
                info!("binance ticker ws 23h limit reached, reconnecting");
                return Err(Report::new(ExchangeError::Connection {
                    exchange: "binance (scheduled reconnect)".into(),
                }));
            }
            msg = read.next() => {
                match msg {
                    None => break,
                    Some(Err(e)) => return Err(Report::new(e)
                        .change_context(ExchangeError::Connection {
                            exchange: "binance".into(),
                        })),
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<BinanceCombinedMsg<BinanceTickerData>>(&text) {
                            Ok(combined) => {
                                let ticker = combined.data.into_ticker();
                                let _ = tx.send(ticker).await;
                            }
                            Err(e) => {
                                warn!(error = %e, raw = %text, "binance ticker parse error");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        // Server sends ping every 20s; must pong within 60s
                        let _ = write.send(Message::Pong(data)).await;
                    }
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
    let streams: Vec<String> = symbols
        .iter()
        .map(|s| format!("{}@trade", s.to_lowercase()))
        .collect();
    let ws_url = format!("{}?streams={}", BINANCE_WS_BASE, streams.join("/"));

    let (ws_stream, _) =
        connect_async(&ws_url)
            .await
            .change_context(ExchangeError::Connection {
                exchange: "binance".into(),
            })?;

    let (mut write, mut read) = ws_stream.split();

    info!(symbols = ?symbols, "binance trades ws connected");

    let reconnect_timer = tokio::time::sleep(Duration::from_secs(WS_RECONNECT_SECS));
    tokio::pin!(reconnect_timer);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!("binance trades ws cancelled");
                break;
            }
            _ = &mut reconnect_timer => {
                info!("binance trades ws 23h limit reached, reconnecting");
                return Err(Report::new(ExchangeError::Connection {
                    exchange: "binance (scheduled reconnect)".into(),
                }));
            }
            msg = read.next() => {
                match msg {
                    None => break,
                    Some(Err(e)) => return Err(Report::new(e)
                        .change_context(ExchangeError::Connection {
                            exchange: "binance".into(),
                        })),
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<BinanceCombinedMsg<BinanceTradeData>>(&text) {
                            Ok(combined) => {
                                let trade = combined.data.into_trade();
                                let _ = tx.send(trade).await;
                            }
                            Err(e) => {
                                warn!(error = %e, raw = %text, "binance trade parse error");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    Ok(())
}

// ── REST response types ───────────────────────────────────────────────────────

/// Binance kline row: 12-element array
/// [open_time, open, high, low, close, volume, close_time, ...]
#[derive(Debug, Deserialize)]
struct BinanceKlineRow(
    i64,                        // 0: open_time (ms)
    String,                     // 1: open
    String,                     // 2: high
    String,                     // 3: low
    String,                     // 4: close
    String,                     // 5: volume
    #[allow(dead_code)] i64,    // 6: close_time
    #[allow(dead_code)] String, // 7: quote asset volume
    #[allow(dead_code)] i64,    // 8: number of trades
    #[allow(dead_code)] String, // 9: taker buy base volume
    #[allow(dead_code)] String, // 10: taker buy quote volume
    #[allow(dead_code)] String, // 11: ignore
);

impl BinanceKlineRow {
    fn into_candle(
        self,
        symbol: &str,
        timeframe: TimeFrame,
    ) -> Result<Candle, Report<ExchangeError>> {
        let parse_f64 = |s: &str| -> Result<f64, Report<ExchangeError>> {
            s.parse::<f64>()
                .change_context(ExchangeError::ResponseParse {
                    exchange: "binance".into(),
                })
        };

        let open_time = DateTime::from_timestamp_millis(self.0).unwrap_or_else(Utc::now);

        Ok(Candle {
            exchange: ExchangeKind::Binance,
            symbol: symbol.to_owned(),
            timeframe,
            open_time,
            open: parse_f64(&self.1)?,
            high: parse_f64(&self.2)?,
            low: parse_f64(&self.3)?,
            close: parse_f64(&self.4)?,
            volume: parse_f64(&self.5)?,
        })
    }
}

// ── WebSocket message types ───────────────────────────────────────────────────

/// Combined stream wrapper: `{ "stream": "...", "data": { ... } }`
#[derive(Debug, Deserialize)]
struct BinanceCombinedMsg<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct BinanceTickerData {
    #[serde(rename = "s")]
    symbol: String,
    /// Last price
    #[serde(rename = "c")]
    price: String,
    /// Total traded base asset volume
    #[serde(rename = "v")]
    volume: String,
    /// Statistics close time (ms epoch)
    #[serde(rename = "C")]
    close_time: i64,
}

impl BinanceTickerData {
    fn into_ticker(self) -> Ticker {
        let price = self.price.parse::<f64>().unwrap_or(0.0);
        let volume = self.volume.parse::<f64>().unwrap_or(0.0);
        let timestamp = DateTime::from_timestamp_millis(self.close_time).unwrap_or_else(Utc::now);

        Ticker {
            exchange: ExchangeKind::Binance,
            symbol: self.symbol,
            price,
            volume,
            timestamp,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct BinanceTradeData {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    /// true = market maker (sell), false = market taker (buy)
    #[serde(rename = "m")]
    is_buyer_maker: bool,
    #[serde(rename = "T")]
    trade_time: i64,
}

impl BinanceTradeData {
    fn into_trade(self) -> Trade {
        let price = self.price.parse::<f64>().unwrap_or(0.0);
        let volume = self.quantity.parse::<f64>().unwrap_or(0.0);
        let timestamp = DateTime::from_timestamp_millis(self.trade_time).unwrap_or_else(Utc::now);
        // buyer_maker = true means the buyer is the maker (limit buy was matched) → sell side aggressor
        let side = if self.is_buyer_maker {
            TradeSide::Sell
        } else {
            TradeSide::Buy
        };

        Trade {
            exchange: ExchangeKind::Binance,
            symbol: self.symbol,
            price,
            volume,
            side,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binance_kline_row_parses_into_candle() {
        let row = BinanceKlineRow(
            1704067200000,
            "42000.0".into(),
            "43000.0".into(),
            "41500.0".into(),
            "42500.0".into(),
            "100.5".into(),
            1704067259999,
            "0".into(),
            10,
            "0".into(),
            "0".into(),
            "0".into(),
        );
        let candle = row.into_candle("BTCUSDT", TimeFrame::Min1).unwrap();
        assert_eq!(candle.exchange, ExchangeKind::Binance);
        assert_eq!(candle.symbol, "BTCUSDT");
        assert_eq!(candle.open, 42000.0);
        assert_eq!(candle.close, 42500.0);
        assert_eq!(candle.volume, 100.5);
    }

    #[test]
    fn binance_trade_buyer_maker_is_sell() {
        let data = BinanceTradeData {
            symbol: "BTCUSDT".into(),
            price: "42000.0".into(),
            quantity: "0.5".into(),
            is_buyer_maker: true,
            trade_time: 1704067200000,
        };
        let trade = data.into_trade();
        assert_eq!(trade.side, TradeSide::Sell);
    }

    #[test]
    fn binance_trade_taker_buy_is_buy() {
        let data = BinanceTradeData {
            symbol: "BTCUSDT".into(),
            price: "42000.0".into(),
            quantity: "0.5".into(),
            is_buyer_maker: false,
            trade_time: 1704067200000,
        };
        let trade = data.into_trade();
        assert_eq!(trade.side, TradeSide::Buy);
    }

    /// Integration test: requires network access. Run with `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn integration_fetch_candles() {
        let exchange = BinanceExchange::new();
        let candles = exchange
            .fetch_candles("BTCUSDT", TimeFrame::Min1, 10)
            .await
            .unwrap();
        assert!(!candles.is_empty());
        assert!(candles.len() <= 10);
    }

    /// Integration test: requires network access. Run with `cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn integration_subscribe_ticker() {
        let exchange = BinanceExchange::new();
        let (tx, mut rx) = mpsc::channel(10);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            exchange
                .subscribe_ticker(&["BTCUSDT".to_owned()], tx, cancel_clone)
                .await
                .unwrap();
        });

        let ticker = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert_eq!(ticker.exchange, ExchangeKind::Binance);
        cancel.cancel();
    }
}
