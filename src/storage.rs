pub mod sqlite;

use chrono::{DateTime, Utc};
use error_stack::Report;
use futures::future::BoxFuture;

use crate::error::StorageError;
use crate::model::{Candle, ExchangeKind, TimeFrame, Trade};

pub trait Storage: Send + Sync {
    fn upsert_candles(&self, candles: &[Candle])
        -> BoxFuture<'_, Result<(), Report<StorageError>>>;

    // Reserved for future trade-level analytics
    #[allow(dead_code)]
    fn insert_trades(&self, trades: &[Trade]) -> BoxFuture<'_, Result<(), Report<StorageError>>>;

    fn get_recent_candles(
        &self,
        exchange: ExchangeKind,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<StorageError>>>;

    fn log_alert(
        &self,
        alert_name: &str,
        exchange: ExchangeKind,
        symbol: &str,
        indicator_value: f64,
        message: &str,
    ) -> BoxFuture<'_, Result<(), Report<StorageError>>>;

    fn last_alert_time(
        &self,
        alert_name: &str,
    ) -> BoxFuture<'_, Result<Option<DateTime<Utc>>, Report<StorageError>>>;
}
