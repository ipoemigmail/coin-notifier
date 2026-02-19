pub mod binance;
pub mod upbit;

use error_stack::Report;
use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::ExchangeError;
use crate::model::{Candle, ExchangeKind, Ticker, TimeFrame, Trade};

/// Abstraction over a cryptocurrency exchange.
///
/// Uses `BoxFuture` (from `futures` crate) instead of `async fn` in trait
/// to keep the trait object-safe (`dyn Exchange`).
pub trait Exchange: Send + Sync {
    fn kind(&self) -> ExchangeKind;

    /// Fetch historical candle data via REST API.
    fn fetch_candles(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<ExchangeError>>>;

    /// Subscribe to real-time ticker updates via WebSocket.
    ///
    /// Sends `Ticker` values into `tx` until `cancel` is triggered.
    fn subscribe_ticker(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Ticker>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>>;

    /// Subscribe to real-time trade updates via WebSocket.
    ///
    /// Sends `Trade` values into `tx` until `cancel` is triggered.
    #[allow(dead_code)]
    fn subscribe_trades(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Trade>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>>;
}
