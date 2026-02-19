# 07. 저장소

## 개요

`sqlx`를 통한 SQLite 데이터베이스. 비동기 런타임, WAL 모드, 내장 마이그레이션 사용.

위치: `src/storage.rs` (trait) + `src/storage/sqlite.rs` (구현)

## 데이터베이스 파일

`{general.data_dir}/coin-notifier.db` 에 저장 (기본: `./data/coin-notifier.db`).

## 마이그레이션 스키마

`sqlx::migrate!` 매크로 사용, 마이그레이션 파일은 `migrations/` 디렉토리에 위치.

### 001_initial.sql

```sql
PRAGMA journal_mode = WAL;

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
```

## Storage Trait

```rust
// src/storage.rs
pub mod sqlite;

pub trait Storage: Send + Sync {
    fn upsert_candles(
        &self, candles: &[Candle],
    ) -> BoxFuture<'_, Result<(), Report<StorageError>>>;

    fn insert_trades(
        &self, trades: &[Trade],
    ) -> BoxFuture<'_, Result<(), Report<StorageError>>>;

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
```

## 주요 연산

### 캔들 Upsert

```sql
INSERT OR REPLACE INTO candles (exchange, symbol, timeframe, open_time, open, high, low, close, volume)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
```

### 최근 캔들 조회

```sql
SELECT * FROM candles
WHERE exchange = ? AND symbol = ? AND timeframe = ?
ORDER BY open_time DESC
LIMIT ?
```

**중요**: 결과는 내림차순으로 반환됨. 지표 계산을 위해 오름차순이 필요하면 애플리케이션 코드에서 역순 정렬.

### 알림 쿨다운 확인

```sql
SELECT triggered_at FROM alerts_log
WHERE alert_name = ?
ORDER BY triggered_at DESC
LIMIT 1
```

## 설정

- 커넥션 풀: `sqlx::SqlitePool` + `SqliteConnectOptions`
- WAL 모드: 마이그레이션의 PRAGMA로 활성화
- `create_if_missing(true)` 옵션 사용

## 테스트

- 단위 테스트: 캔들 upsert + 조회 (인메모리 SQLite `:memory:`)
- 단위 테스트: 체결 insert + 조회
- 단위 테스트: 알림 로그 insert + last_alert_time 조회
- 단위 테스트: upsert 중복 제거 (동일 캔들 2회 삽입)
