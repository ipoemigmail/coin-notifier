mod backtest;
mod config;
mod error;
mod exchange;
mod indicator;
mod model;
mod notifier;
mod signal_input;
mod signal_model;
mod storage;
mod strategy;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use derive_more::{Display, Error};
use error_stack::{Report, ResultExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::AppConfig;
use error::ExchangeError;
use exchange::Exchange;
use exchange::binance::BinanceExchange;
use exchange::upbit::UpbitExchange;
use indicator::Indicator;
use indicator::bollinger::BollingerBands;
use indicator::ma::{Ema, Sma};
use indicator::macd::Macd;
use indicator::rsi::Rsi;
use indicator::volume::VolumeMA;
use model::{Candle, ExchangeKind, Ticker, TimeFrame, Trade};
use notifier::Notifier;
use notifier::terminal::TerminalNotifier;
use storage::Storage;
use storage::sqlite::SqliteStorage;
use strategy::AlertRule;
use strategy::condition::{evaluate, should_alert};

#[derive(Debug, Display, Error)]
pub enum AppError {
    #[display("configuration error")]
    Config,
    #[display("storage error")]
    Storage,
    #[display("exchange error")]
    Exchange,
    #[display("runtime error")]
    Runtime,
}

#[derive(Parser)]
#[command(name = "coin-notifier", about = "Coin trading signal notifier")]
struct Cli {
    /// Path to the TOML configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run real-time signal watcher
    Live,
    /// Run backtest commands
    Backtest {
        #[command(subcommand)]
        command: Option<BacktestCommand>,
    },
}

#[derive(Subcommand)]
enum BacktestCommand {
    /// Run historical backtest and save result into SQLite
    Run,
    /// Show backtest result summary from SQLite
    Report {
        /// Specific run id to inspect
        #[arg(long)]
        run_id: Option<String>,
        /// Number of runs to list when --run-id is omitted
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Number of trades to show for --run-id detail mode
        #[arg(long, default_value_t = 20)]
        trades_limit: usize,
    },
}

#[tokio::main]
async fn main() {
    if let Err(report) = run().await {
        eprintln!("{report:?}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Report<AppError>> {
    let cli = Cli::parse();
    let config = config::load(Path::new(&cli.config)).change_context(AppError::Config)?;

    init_tracing(&config);

    match cli.command.unwrap_or(Command::Live) {
        Command::Live => run_live(&config).await,
        Command::Backtest { command } => match command.unwrap_or(BacktestCommand::Run) {
            BacktestCommand::Run => run_backtest(&config).await,
            BacktestCommand::Report {
                run_id,
                limit,
                trades_limit,
            } => run_backtest_report(&config, run_id, limit, trades_limit).await,
        },
    }
}

async fn run_backtest(config: &AppConfig) -> Result<(), Report<AppError>> {
    let storage = open_storage(config).await?;
    let output = backtest::run(config, storage.as_ref())
        .await
        .map_err(|e| Report::new(AppError::Runtime).attach(e))?;

    info!(
        run_id = %output.run.run_id,
        model = %output.run.model_name,
        symbol = %output.run.symbol,
        trades = output.run.trade_count,
        trade_rows = output.trades.len(),
        final_equity = output.run.final_equity,
        total_return_pct = output.run.total_return_pct,
        max_drawdown_pct = output.run.max_drawdown_pct,
        win_rate_pct = output.run.win_rate_pct,
        "backtest finished"
    );

    println!(
        "run_id={} trades={} final_equity={:.2} return={:.2}% mdd={:.2}% win_rate={:.2}%",
        output.run.run_id,
        output.run.trade_count,
        output.run.final_equity,
        output.run.total_return_pct,
        output.run.max_drawdown_pct,
        output.run.win_rate_pct
    );

    Ok(())
}

async fn run_backtest_report(
    config: &AppConfig,
    run_id: Option<String>,
    limit: usize,
    trades_limit: usize,
) -> Result<(), Report<AppError>> {
    let storage = open_storage(config).await?;

    if let Some(run_id) = run_id {
        let run = storage
            .get_backtest_run(&run_id)
            .await
            .change_context(AppError::Storage)?;

        let Some(run) = run else {
            println!("no backtest run found for run_id={run_id}");
            return Ok(());
        };

        println!(
            "run_id={} model={} exchange={} symbol={} timeframe={} trades={} final_equity={:.2} return={:.2}% mdd={:.2}% win_rate={:.2}%",
            run.run_id,
            run.model_name,
            run.exchange,
            run.symbol,
            run.timeframe,
            run.trade_count,
            run.final_equity,
            run.total_return_pct,
            run.max_drawdown_pct,
            run.win_rate_pct
        );

        let trades = storage
            .list_backtest_trades(&run.run_id, trades_limit)
            .await
            .change_context(AppError::Storage)?;

        if trades.is_empty() {
            println!("no trades stored for run_id={}", run.run_id);
            return Ok(());
        }

        for trade in trades {
            println!(
                "exit_time={} entry={:.4} exit={:.4} qty={:.6} net_pnl={:.4} fee={:.4} reason={}",
                trade.exit_time,
                trade.entry_price,
                trade.exit_price,
                trade.quantity,
                trade.net_pnl,
                trade.fee_paid,
                trade.reason
            );
        }

        return Ok(());
    }

    let runs = storage
        .list_backtest_runs(limit)
        .await
        .change_context(AppError::Storage)?;

    if runs.is_empty() {
        println!("no backtest runs found");
        return Ok(());
    }

    for run in runs {
        println!(
            "run_id={} created_at={} model={} exchange={} symbol={} timeframe={} trades={} return={:.2}% mdd={:.2}% final_equity={:.2}",
            run.run_id,
            run.created_at,
            run.model_name,
            run.exchange,
            run.symbol,
            run.timeframe,
            run.trade_count,
            run.total_return_pct,
            run.max_drawdown_pct,
            run.final_equity
        );
    }

    Ok(())
}

async fn run_live(config: &AppConfig) -> Result<(), Report<AppError>> {
    let storage = open_storage(config).await?;

    match config.live.risk.max_entries_per_position {
        Some(limit) => info!(max_entries_per_position = limit, "live risk policy loaded"),
        None => info!("live risk policy loaded: unlimited entries"),
    }

    let exchanges: Vec<Arc<dyn Exchange>> = build_exchanges(config);

    if exchanges.is_empty() {
        tracing::warn!("no exchanges enabled; nothing to do");
        return Ok(());
    }

    // ── Rules ─────────────────────────────────────────────────────────────────
    let rules: Arc<Vec<AlertRule>> = Arc::new(AlertRule::from_config(config));
    let historical_limit = config.general.historical_candles;

    // ── Historical data fetch ─────────────────────────────────────────────────
    // Both exchanges use governor rate limiters internally, so all jobs
    // can be spawned in parallel. The rate limiter in each Exchange ensures
    // requests stay within API limits.
    let mut historical_handles = Vec::new();
    for exchange in &exchanges {
        let exchange_kind = exchange.kind();
        let coins_for_exchange: Vec<_> = config
            .coins
            .iter()
            .filter(|c| c.exchange == exchange_kind.to_string())
            .collect();

        let jobs: Vec<(String, TimeFrame)> = coins_for_exchange
            .iter()
            .flat_map(|coin| {
                coin.timeframes
                    .iter()
                    .filter_map(|tf| TimeFrame::from_str(tf).map(|t| (coin.symbol.clone(), t)))
            })
            .collect();

        for (symbol, timeframe) in jobs {
            let exchange = Arc::clone(exchange);
            let storage: Arc<dyn Storage> = Arc::clone(&storage);
            let handle = tokio::spawn(async move {
                if let Err(e) = fetch_and_store_historical(
                    exchange.as_ref(),
                    storage.as_ref(),
                    &symbol,
                    timeframe,
                    historical_limit,
                )
                .await
                {
                    tracing::warn!(error = ?e, "historical fetch failed (continuing)");
                }
            });
            historical_handles.push(handle);
        }
    }

    // Wait for all historical fetches to complete before starting WebSocket
    for handle in historical_handles {
        handle.await.change_context(AppError::Runtime)?;
    }

    info!("historical data fetch complete, starting WebSocket streams");

    // ── WebSocket channels ────────────────────────────────────────────────────
    let cancel = CancellationToken::new();
    let (ticker_tx, ticker_rx) = mpsc::channel::<Ticker>(1024);
    let (trade_tx, trade_rx) = mpsc::channel::<Trade>(4096);

    let mut task_handles = Vec::new();

    // WebSocket ticker/trade subscriptions
    for exchange in &exchanges {
        let exchange_kind = exchange.kind();
        let symbols: Vec<String> = config
            .coins
            .iter()
            .filter(|c| c.exchange == exchange_kind.to_string())
            .map(|c| c.symbol.clone())
            .collect();

        if symbols.is_empty() {
            continue;
        }

        let ticker_exchange = Arc::clone(exchange);
        let ticker_tx_clone = ticker_tx.clone();
        let ticker_cancel = cancel.clone();
        let ticker_symbols = symbols.clone();

        let ticker_handle = tokio::spawn(async move {
            if let Err(e) = ticker_exchange
                .subscribe_ticker(&ticker_symbols, ticker_tx_clone, ticker_cancel)
                .await
            {
                tracing::error!(error = ?e, "ticker subscription failed");
            }
        });
        task_handles.push(ticker_handle);

        let trade_exchange = Arc::clone(exchange);
        let trade_tx_clone = trade_tx.clone();
        let trade_cancel = cancel.clone();

        let trade_handle = tokio::spawn(async move {
            if let Err(e) = trade_exchange
                .subscribe_trades(&symbols, trade_tx_clone, trade_cancel)
                .await
            {
                tracing::error!(error = ?e, "trade subscription failed");
            }
        });
        task_handles.push(trade_handle);
    }

    // Drop the original sender so the receiver closes when all spawned senders drop
    drop(ticker_tx);
    drop(trade_tx);

    // Sync real-time 1m candles from trades
    let candle_sync_handle = tokio::spawn(sync_realtime_candles_from_trades(
        trade_rx,
        Arc::clone(&storage),
    ));
    task_handles.push(candle_sync_handle);

    // ── Analysis loop ─────────────────────────────────────────────────────────
    let notifier: Arc<dyn Notifier> = Arc::new(TerminalNotifier);
    let analysis_handle = tokio::spawn(analysis_loop(
        ticker_rx,
        Arc::clone(&storage),
        Arc::clone(&rules),
        Arc::clone(&notifier),
    ));
    task_handles.push(analysis_handle);

    // ── Shutdown ──────────────────────────────────────────────────────────────
    tokio::signal::ctrl_c()
        .await
        .change_context(AppError::Runtime)?;

    info!("ctrl+c received, shutting down");
    cancel.cancel();

    for handle in task_handles {
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    }

    info!("shutdown complete");
    Ok(())
}

async fn open_storage(config: &AppConfig) -> Result<Arc<dyn Storage>, Report<AppError>> {
    let data_dir = &config.general.data_dir;
    std::fs::create_dir_all(data_dir)
        .change_context(AppError::Storage)
        .attach_with(|| format!("data_dir: {data_dir}"))?;

    let db_path = format!("{data_dir}/coin-notifier.db");
    let storage: Arc<dyn Storage> = Arc::new(
        SqliteStorage::open(Path::new(&db_path))
            .await
            .change_context(AppError::Storage)?,
    );
    Ok(storage)
}

fn init_tracing(config: &AppConfig) {
    let filter = EnvFilter::new(&config.general.log_level);
    match config.general.log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .init();
        }
        _ => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

fn build_exchanges(config: &AppConfig) -> Vec<Arc<dyn Exchange>> {
    config
        .exchanges
        .iter()
        .filter(|e| e.enabled)
        .filter_map(|e| match e.name.as_str() {
            "upbit" => Some(Arc::new(UpbitExchange::new()) as Arc<dyn Exchange>),
            "binance" => Some(Arc::new(BinanceExchange::new()) as Arc<dyn Exchange>),
            other => {
                tracing::warn!(name = other, "unknown exchange in config, skipping");
                None
            }
        })
        .collect()
}

async fn fetch_and_store_historical(
    exchange: &dyn Exchange,
    storage: &dyn Storage,
    symbol: &str,
    timeframe: TimeFrame,
    limit: usize,
) -> Result<(), Report<ExchangeError>> {
    info!(
        exchange = %exchange.kind(),
        symbol,
        timeframe = %timeframe,
        limit,
        "fetching historical candles"
    );

    let candles = exchange.fetch_candles(symbol, timeframe, limit).await?;

    storage
        .upsert_candles(&candles)
        .await
        .change_context(ExchangeError::Request {
            exchange: exchange.kind().to_string(),
        })?;

    info!(
        exchange = %exchange.kind(),
        symbol,
        timeframe = %timeframe,
        fetched = candles.len(),
        "historical candles stored"
    );

    Ok(())
}

async fn analysis_loop(
    mut rx: mpsc::Receiver<Ticker>,
    storage: Arc<dyn Storage>,
    rules: Arc<Vec<AlertRule>>,
    notifier: Arc<dyn Notifier>,
) {
    while let Some(ticker) = rx.recv().await {
        process_ticker(&ticker, storage.as_ref(), &rules, notifier.as_ref()).await;
    }
}

async fn sync_realtime_candles_from_trades(
    mut rx: mpsc::Receiver<Trade>,
    storage: Arc<dyn Storage>,
) {
    let mut latest_candles: HashMap<(ExchangeKind, String), Candle> = HashMap::new();

    while let Some(trade) = rx.recv().await {
        let Some(candle) = merge_trade_into_minute_candle(&mut latest_candles, &trade) else {
            continue;
        };

        if let Err(e) = storage.upsert_candles(&[candle]).await {
            tracing::warn!(
                error = ?e,
                exchange = %trade.exchange,
                symbol = %trade.symbol,
                "failed to upsert realtime 1m candle"
            );
        }
    }
}

fn merge_trade_into_minute_candle(
    latest_candles: &mut HashMap<(ExchangeKind, String), Candle>,
    trade: &Trade,
) -> Option<Candle> {
    let key = (trade.exchange, trade.symbol.clone());
    let minute_open = minute_open_time(trade.timestamp);

    match latest_candles.get_mut(&key) {
        Some(candle) if candle.open_time < minute_open => {
            *candle = new_minute_candle(trade, minute_open);
            Some(candle.clone())
        }
        Some(candle) if candle.open_time == minute_open => {
            candle.high = candle.high.max(trade.price);
            candle.low = candle.low.min(trade.price);
            candle.close = trade.price;
            candle.volume += trade.volume;
            Some(candle.clone())
        }
        Some(_) => None,
        None => {
            let candle = new_minute_candle(trade, minute_open);
            latest_candles.insert(key, candle.clone());
            Some(candle)
        }
    }
}

fn new_minute_candle(trade: &Trade, open_time: DateTime<Utc>) -> Candle {
    Candle {
        exchange: trade.exchange,
        symbol: trade.symbol.clone(),
        timeframe: TimeFrame::Min1,
        open_time,
        open: trade.price,
        high: trade.price,
        low: trade.price,
        close: trade.price,
        volume: trade.volume,
    }
}

fn minute_open_time(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    let unix_seconds = timestamp.timestamp();
    let bucket_seconds = unix_seconds - unix_seconds.rem_euclid(60);
    DateTime::from_timestamp(bucket_seconds, 0).unwrap_or(timestamp)
}

async fn process_ticker(
    ticker: &Ticker,
    storage: &dyn Storage,
    rules: &[AlertRule],
    notifier: &dyn Notifier,
) {
    let matching_rules: Vec<&AlertRule> = rules
        .iter()
        .filter(|r| r.exchange == ticker.exchange && r.symbol == ticker.symbol)
        .collect();

    if matching_rules.is_empty() {
        return;
    }

    for rule in matching_rules {
        // Find an appropriate timeframe for this rule (use first available from DB)
        // We try 1m candles for signal computation
        let timeframe = TimeFrame::Min1;
        let indicator = build_indicator(rule);
        let required = indicator.required_candles();

        // Fetch enough candles for the indicator (need +1 for previous value)
        let candles = match storage
            .get_recent_candles(ticker.exchange, &ticker.symbol, timeframe, required + 1)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = ?e, rule = %rule.name, "failed to fetch candles");
                continue;
            }
        };

        if candles.len() < required {
            tracing::debug!(
                rule = %rule.name,
                available = candles.len(),
                required,
                "insufficient candles for indicator"
            );
            continue;
        }

        let values = match indicator.calculate(&candles) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = ?e, rule = %rule.name, "indicator calculation failed");
                continue;
            }
        };

        if values.is_empty() {
            continue;
        }

        let current = *values.last().unwrap();
        let previous = values.len().checked_sub(2).map(|i| values[i]);

        let result = evaluate(rule, current, previous);
        if !result.triggered {
            continue;
        }

        match should_alert(storage, rule).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(rule = %rule.name, "alert suppressed by cooldown");
                continue;
            }
            Err(e) => {
                tracing::warn!(error = ?e, rule = %rule.name, "cooldown check failed");
                continue;
            }
        }

        notifier.notify(ticker.exchange, &ticker.symbol, ticker.price, &result);

        if let Err(e) = storage
            .log_alert(
                &rule.name,
                ticker.exchange,
                &ticker.symbol,
                result.indicator_value,
                &result.message,
            )
            .await
        {
            tracing::warn!(error = ?e, "failed to log alert");
        }
    }
}

fn build_indicator(rule: &AlertRule) -> Box<dyn Indicator> {
    let params = &rule.indicator_params;
    let period = params.period.unwrap_or(14);

    match rule.indicator_name.as_str() {
        "rsi" => Rsi::new(period)
            .map(|i| Box::new(i) as Box<dyn Indicator>)
            .unwrap_or_else(|_| Box::new(Rsi::new(14).unwrap())),
        "sma" => Sma::new(period)
            .map(|i| Box::new(i) as Box<dyn Indicator>)
            .unwrap_or_else(|_| Box::new(Sma::new(14).unwrap())),
        "ema" => Ema::new(period)
            .map(|i| Box::new(i) as Box<dyn Indicator>)
            .unwrap_or_else(|_| Box::new(Ema::new(14).unwrap())),
        "macd" => {
            let fast = params.fast_period.unwrap_or(12);
            let slow = params.slow_period.unwrap_or(26);
            let signal = params.signal_period.unwrap_or(9);
            Macd::new(fast, slow, signal)
                .map(|i| Box::new(i) as Box<dyn Indicator>)
                .unwrap_or_else(|_| Box::new(Macd::new(12, 26, 9).unwrap()))
        }
        "bollinger" => {
            let mult = params.std_dev_multiplier.unwrap_or(2.0);
            BollingerBands::new(period, mult)
                .map(|i| Box::new(i) as Box<dyn Indicator>)
                .unwrap_or_else(|_| Box::new(BollingerBands::new(20, 2.0).unwrap()))
        }
        "volume" => VolumeMA::new(period)
            .map(|i| Box::new(i) as Box<dyn Indicator>)
            .unwrap_or_else(|_| Box::new(VolumeMA::new(20).unwrap())),
        _ => {
            tracing::warn!(indicator = %rule.indicator_name, "unknown indicator, defaulting to RSI(14)");
            Box::new(Rsi::new(14).unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TradeSide;

    fn make_trade(timestamp: i64, price: f64, volume: f64) -> Trade {
        Trade {
            exchange: ExchangeKind::Upbit,
            symbol: "KRW-SOL".into(),
            price,
            volume,
            side: TradeSide::Buy,
            timestamp: DateTime::from_timestamp(timestamp, 0).unwrap(),
        }
    }

    #[test]
    fn minute_open_time_rounds_down_to_minute() {
        let timestamp = DateTime::from_timestamp(125, 999_000_000).unwrap();
        assert_eq!(minute_open_time(timestamp).timestamp(), 120);
    }

    #[test]
    fn merge_trade_updates_existing_minute_candle() {
        let mut latest = HashMap::new();
        let first = make_trade(180, 100.0, 1.2);
        let second = make_trade(185, 110.0, 0.8);

        let _ = merge_trade_into_minute_candle(&mut latest, &first);
        let updated = merge_trade_into_minute_candle(&mut latest, &second).unwrap();

        assert_eq!(updated.open, 100.0);
        assert_eq!(updated.high, 110.0);
        assert_eq!(updated.low, 100.0);
        assert_eq!(updated.close, 110.0);
        assert_eq!(updated.volume, 2.0);
        assert_eq!(updated.open_time.timestamp(), 180);
    }

    #[test]
    fn merge_trade_rolls_over_to_new_minute_candle() {
        let mut latest = HashMap::new();
        let first = make_trade(180, 100.0, 1.0);
        let second = make_trade(240, 95.0, 0.5);

        let _ = merge_trade_into_minute_candle(&mut latest, &first);
        let rolled = merge_trade_into_minute_candle(&mut latest, &second).unwrap();

        assert_eq!(rolled.open, 95.0);
        assert_eq!(rolled.high, 95.0);
        assert_eq!(rolled.low, 95.0);
        assert_eq!(rolled.close, 95.0);
        assert_eq!(rolled.volume, 0.5);
        assert_eq!(rolled.open_time.timestamp(), 240);
    }

    #[test]
    fn merge_trade_ignores_out_of_order_old_minute() {
        let mut latest = HashMap::new();
        let recent = make_trade(240, 100.0, 1.0);
        let stale = make_trade(180, 90.0, 0.5);

        let _ = merge_trade_into_minute_candle(&mut latest, &recent);
        let ignored = merge_trade_into_minute_candle(&mut latest, &stale);

        assert!(ignored.is_none());
        let candle = latest
            .get(&(ExchangeKind::Upbit, "KRW-SOL".to_string()))
            .unwrap();
        assert_eq!(candle.open_time.timestamp(), 240);
        assert_eq!(candle.close, 100.0);
        assert_eq!(candle.volume, 1.0);
    }
}
