use chrono::{DateTime, Utc};
use error_stack::{Report, ResultExt};
use futures::future::BoxFuture;
use sqlx::{SqlitePool, sqlite::{SqliteConnectOptions, SqliteJournalMode}};
use std::path::Path;
use std::str::FromStr;

use crate::error::StorageError;
use crate::model::{Candle, ExchangeKind, TimeFrame, Trade, TradeSide};
use crate::storage::Storage;

pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at `path` and run migrations.
    pub async fn open(path: &Path) -> Result<Self, Report<StorageError>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .change_context(StorageError::Migration)
                .attach_with(|| format!("cannot create data directory: {}", parent.display()))?;
        }

        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .change_context(StorageError::Migration)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePool::connect_with(opts)
            .await
            .change_context(StorageError::Migration)
            .attach_with(|| format!("database path: {}", path.display()))?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .change_context(StorageError::Migration)?;

        Ok(Self { pool })
    }
}

impl Storage for SqliteStorage {
    fn upsert_candles(
        &self,
        candles: &[Candle],
    ) -> BoxFuture<'_, Result<(), Report<StorageError>>> {
        let candles = candles.to_vec();
        Box::pin(async move {
            let mut tx = self
                .pool
                .begin()
                .await
                .change_context(StorageError::Insert)?;

            for c in &candles {
                sqlx::query(
                    "INSERT OR REPLACE INTO candles \
                     (exchange, symbol, timeframe, open_time, open, high, low, close, volume) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(c.exchange.to_string())
                .bind(&c.symbol)
                .bind(c.timeframe.as_str())
                .bind(c.open_time.to_rfc3339())
                .bind(c.open)
                .bind(c.high)
                .bind(c.low)
                .bind(c.close)
                .bind(c.volume)
                .execute(&mut *tx)
                .await
                .change_context(StorageError::Insert)?;
            }

            tx.commit().await.change_context(StorageError::Insert)?;
            Ok(())
        })
    }

    fn insert_trades(&self, trades: &[Trade]) -> BoxFuture<'_, Result<(), Report<StorageError>>> {
        let trades = trades.to_vec();
        Box::pin(async move {
            let mut tx = self
                .pool
                .begin()
                .await
                .change_context(StorageError::Insert)?;

            for t in &trades {
                let side = match t.side {
                    TradeSide::Buy => "buy",
                    TradeSide::Sell => "sell",
                };
                sqlx::query(
                    "INSERT INTO trades (exchange, symbol, timestamp, price, volume, side) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(t.exchange.to_string())
                .bind(&t.symbol)
                .bind(t.timestamp.to_rfc3339())
                .bind(t.price)
                .bind(t.volume)
                .bind(side)
                .execute(&mut *tx)
                .await
                .change_context(StorageError::Insert)?;
            }

            tx.commit().await.change_context(StorageError::Insert)?;
            Ok(())
        })
    }

    fn get_recent_candles(
        &self,
        exchange: ExchangeKind,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<StorageError>>> {
        let symbol = symbol.to_string();
        Box::pin(async move {
            #[allow(clippy::type_complexity)]
            let rows: Vec<(String, String, String, String, f64, f64, f64, f64, f64)> =
                sqlx::query_as(
                    "SELECT exchange, symbol, timeframe, open_time, open, high, low, close, volume \
                     FROM candles \
                     WHERE exchange = ? AND symbol = ? AND timeframe = ? \
                     ORDER BY open_time DESC \
                     LIMIT ?",
                )
                .bind(exchange.to_string())
                .bind(&symbol)
                .bind(timeframe.as_str())
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
                .change_context(StorageError::Query)?;

            let mut candles: Vec<Candle> = rows
                .into_iter()
                .map(|(exch, sym, tf, ot, open, high, low, close, volume)| {
                    let exchange = if exch == "upbit" {
                        ExchangeKind::Upbit
                    } else {
                        ExchangeKind::Binance
                    };
                    let timeframe = TimeFrame::from_str(&tf).unwrap_or(TimeFrame::Min1);
                    let open_time = DateTime::parse_from_rfc3339(&ot)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    Candle {
                        exchange,
                        symbol: sym,
                        timeframe,
                        open_time,
                        open,
                        high,
                        low,
                        close,
                        volume,
                    }
                })
                .collect();

            // Return in ascending chronological order (oldest first)
            candles.reverse();
            Ok(candles)
        })
    }

    fn log_alert(
        &self,
        alert_name: &str,
        exchange: ExchangeKind,
        symbol: &str,
        indicator_value: f64,
        message: &str,
    ) -> BoxFuture<'_, Result<(), Report<StorageError>>> {
        let alert_name = alert_name.to_string();
        let symbol = symbol.to_string();
        let message = message.to_string();
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO alerts_log \
                 (alert_name, exchange, symbol, triggered_at, indicator_value, message) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&alert_name)
            .bind(exchange.to_string())
            .bind(&symbol)
            .bind(Utc::now().to_rfc3339())
            .bind(indicator_value)
            .bind(&message)
            .execute(&self.pool)
            .await
            .change_context(StorageError::Insert)?;
            Ok(())
        })
    }

    fn last_alert_time(
        &self,
        alert_name: &str,
    ) -> BoxFuture<'_, Result<Option<DateTime<Utc>>, Report<StorageError>>> {
        let alert_name = alert_name.to_string();
        Box::pin(async move {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT triggered_at FROM alerts_log \
                 WHERE alert_name = ? \
                 ORDER BY triggered_at DESC \
                 LIMIT 1",
            )
            .bind(&alert_name)
            .fetch_optional(&self.pool)
            .await
            .change_context(StorageError::Query)?;

            let dt = row.and_then(|(ts,)| {
                DateTime::parse_from_rfc3339(&ts)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            Ok(dt)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TradeSide;

    async fn in_memory_storage() -> SqliteStorage {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        SqliteStorage { pool }
    }

    fn make_candle(symbol: &str, open_time: DateTime<Utc>, close: f64) -> Candle {
        Candle {
            exchange: ExchangeKind::Upbit,
            symbol: symbol.to_string(),
            timeframe: TimeFrame::Min1,
            open_time,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        }
    }

    #[tokio::test]
    async fn upsert_and_query_candles() {
        let storage = in_memory_storage().await;
        let t = Utc::now();
        let candles = vec![
            make_candle("KRW-BTC", t, 100.0),
            make_candle("KRW-BTC", t + chrono::Duration::minutes(1), 110.0),
        ];
        storage.upsert_candles(&candles).await.unwrap();

        let result = storage
            .get_recent_candles(ExchangeKind::Upbit, "KRW-BTC", TimeFrame::Min1, 10)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        // Ascending order: oldest first
        assert_eq!(result[0].close, 100.0);
        assert_eq!(result[1].close, 110.0);
    }

    #[tokio::test]
    async fn upsert_deduplication() {
        let storage = in_memory_storage().await;
        let t = Utc::now();
        let candle = make_candle("KRW-BTC", t, 100.0);
        storage.upsert_candles(&[candle.clone()]).await.unwrap();

        // Upsert same candle with different close price -> should replace
        let updated = Candle { close: 200.0, ..candle };
        storage.upsert_candles(&[updated]).await.unwrap();

        let result = storage
            .get_recent_candles(ExchangeKind::Upbit, "KRW-BTC", TimeFrame::Min1, 10)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].close, 200.0);
    }

    #[tokio::test]
    async fn insert_trade() {
        let storage = in_memory_storage().await;
        let trade = Trade {
            exchange: ExchangeKind::Binance,
            symbol: "BTCUSDT".into(),
            price: 50000.0,
            volume: 0.5,
            side: TradeSide::Buy,
            timestamp: Utc::now(),
        };
        // Verify no error on insert
        storage.insert_trades(&[trade]).await.unwrap();
    }

    #[tokio::test]
    async fn alert_log_and_last_alert_time() {
        let storage = in_memory_storage().await;

        let last = storage.last_alert_time("test-alert").await.unwrap();
        assert!(last.is_none());

        storage
            .log_alert("test-alert", ExchangeKind::Upbit, "KRW-BTC", 28.5, "RSI oversold")
            .await
            .unwrap();

        let last = storage.last_alert_time("test-alert").await.unwrap();
        assert!(last.is_some());
    }
}
