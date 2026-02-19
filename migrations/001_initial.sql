CREATE TABLE IF NOT EXISTS candles (
    exchange    TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    timeframe   TEXT NOT NULL,
    open_time   TEXT NOT NULL,
    open        REAL NOT NULL,
    high        REAL NOT NULL,
    low         REAL NOT NULL,
    close       REAL NOT NULL,
    volume      REAL NOT NULL,
    UNIQUE(exchange, symbol, timeframe, open_time)
);

CREATE INDEX IF NOT EXISTS idx_candles_lookup
    ON candles(exchange, symbol, timeframe, open_time DESC);

CREATE TABLE IF NOT EXISTS trades (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    exchange    TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    timestamp   TEXT NOT NULL,
    price       REAL NOT NULL,
    volume      REAL NOT NULL,
    side        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_trades_lookup
    ON trades(exchange, symbol, timestamp DESC);

CREATE TABLE IF NOT EXISTS alerts_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    alert_name      TEXT NOT NULL,
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    triggered_at    TEXT NOT NULL,
    indicator_value REAL,
    message         TEXT
);

CREATE INDEX IF NOT EXISTS idx_alerts_log_lookup
    ON alerts_log(alert_name, triggered_at DESC);
