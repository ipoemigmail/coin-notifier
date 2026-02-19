mod config;
mod error;
mod exchange;
mod indicator;
mod model;
mod notifier;
mod storage;
mod strategy;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
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
use model::{Ticker, TimeFrame};
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

    // ── Storage ───────────────────────────────────────────────────────────────
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

    // ── Exchanges ─────────────────────────────────────────────────────────────
    let exchanges: Vec<Arc<dyn Exchange>> = build_exchanges(&config);

    if exchanges.is_empty() {
        tracing::warn!("no exchanges enabled; nothing to do");
        return Ok(());
    }

    // ── Rules ─────────────────────────────────────────────────────────────────
    let rules: Arc<Vec<AlertRule>> = Arc::new(AlertRule::from_config(&config));
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
                coin.timeframes.iter().filter_map(|tf| {
                    TimeFrame::from_str(tf).map(|t| (coin.symbol.clone(), t))
                })
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

    let mut task_handles = Vec::new();

    // WebSocket ticker subscriptions
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

        let exchange = Arc::clone(exchange);
        let tx = ticker_tx.clone();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = exchange.subscribe_ticker(&symbols, tx, cancel_clone).await {
                tracing::error!(error = ?e, "ticker subscription failed");
            }
        });
        task_handles.push(handle);
    }

    // Drop the original sender so the receiver closes when all spawned senders drop
    drop(ticker_tx);

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
