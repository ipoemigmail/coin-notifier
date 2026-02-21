CREATE TABLE IF NOT EXISTS backtest_runs (
    run_id            TEXT PRIMARY KEY,
    model_name        TEXT NOT NULL,
    exchange          TEXT NOT NULL,
    symbol            TEXT NOT NULL,
    timeframe         TEXT NOT NULL,
    start_time        TEXT NOT NULL,
    end_time          TEXT NOT NULL,
    initial_capital   REAL NOT NULL,
    final_equity      REAL NOT NULL,
    total_return_pct  REAL NOT NULL,
    max_drawdown_pct  REAL NOT NULL,
    win_rate_pct      REAL NOT NULL,
    trade_count       INTEGER NOT NULL,
    created_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS backtest_trades (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id       TEXT NOT NULL,
    exchange     TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    entry_time   TEXT NOT NULL,
    exit_time    TEXT NOT NULL,
    entry_price  REAL NOT NULL,
    exit_price   REAL NOT NULL,
    quantity     REAL NOT NULL,
    gross_pnl    REAL NOT NULL,
    net_pnl      REAL NOT NULL,
    fee_paid     REAL NOT NULL,
    reason       TEXT NOT NULL,
    FOREIGN KEY(run_id) REFERENCES backtest_runs(run_id)
);

CREATE INDEX IF NOT EXISTS idx_backtest_runs_created_at
    ON backtest_runs(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_backtest_trades_run_id
    ON backtest_trades(run_id);
