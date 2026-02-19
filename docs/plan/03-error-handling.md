# 03. 에러 처리

## 사용 크레이트

- `error-stack` 0.6 — `Report`를 통한 컨텍스트 기반 에러 전파
- `derive_more` 2 — `Display` + `Error` derive 매크로 (thiserror 대체)

**미사용**: `anyhow`, `thiserror`, `async_trait`

## 에러 타입

`src/error.rs`에 정의:

```rust
use derive_more::{Display, Error};

#[derive(Debug, Display, Error)]
pub enum ConfigError {
    #[display("failed to read config file")]
    ReadFile,
    #[display("failed to parse config: {reason}")]
    Parse { reason: String },
    #[display("invalid config: {field}")]
    Validation { field: String },
}

#[derive(Debug, Display, Error)]
pub enum ExchangeError {
    #[display("failed to connect to {exchange}")]
    Connection { exchange: String },
    #[display("request to {exchange} failed")]
    Request { exchange: String },
    #[display("failed to parse response from {exchange}")]
    ResponseParse { exchange: String },
    #[display("rate limit exceeded for {exchange}")]
    RateLimit { exchange: String },
}

#[derive(Debug, Display, Error)]
pub enum StorageError {
    #[display("database migration failed")]
    Migration,
    #[display("failed to insert data")]
    Insert,
    #[display("failed to query data")]
    Query,
}

#[derive(Debug, Display, Error)]
pub enum IndicatorError {
    #[display("insufficient data: need {required}, got {available}")]
    InsufficientData { required: usize, available: usize },
    #[display("invalid parameter: {name}")]
    InvalidParameter { name: String },
}
```

## 사용 패턴

### Report 생성

```rust
use error_stack::Report;

fn connect(url: &str) -> Result<(), Report<ExchangeError>> {
    Err(Report::new(ExchangeError::Connection {
        exchange: "upbit".into(),
    }))
}
```

### 컨텍스트 변환 (모듈 경계 횡단 시)

```rust
use error_stack::ResultExt;

fn load_config(path: &Path) -> Result<AppConfig, Report<ConfigError>> {
    let content = std::fs::read_to_string(path)
        .change_context(ConfigError::ReadFile)
        .attach_with(|| format!("path: {}", path.display()))?;

    let config: AppConfig = toml::from_str(&content)
        .change_context(ConfigError::Parse {
            reason: "invalid TOML syntax".into(),
        })?;

    Ok(config)
}
```

### 디버그 컨텍스트 첨부

```rust
let candles = exchange
    .fetch_candles(symbol, timeframe, limit)
    .await
    .attach_with(|| format!("symbol={symbol}, timeframe={timeframe:?}"))
    .attach_with(|| format!("limit={limit}"))?;
```

### 외부 에러를 change_context로 변환 (원본 체인 보존)

```rust
// 외부 라이브러리(tungstenite, reqwest 등)의 에러를 앱 에러로 변환할 때
// Report::new(원본_에러).change_context(앱_에러)로 원본 에러 체인 보존
let (ws_stream, _) = connect_async(WS_URL)
    .await
    .change_context(ExchangeError::Connection {
        exchange: "upbit".into(),
    })?;

// 원본 에러가 없는 경우에만 Report::new(앱_에러) + attach 사용
if !response.status().is_success() {
    return Err(Report::new(ExchangeError::Request {
        exchange: "binance".into(),
    })
    .attach(format!("HTTP status: {}", response.status())));
}
```

### bail! 및 ensure! 매크로

```rust
use error_stack::{bail, ensure};

fn validate_period(period: usize) -> Result<(), Report<IndicatorError>> {
    ensure!(
        period > 0,
        IndicatorError::InvalidParameter {
            name: "period".into(),
        }
    );
    Ok(())
}
```

## 가이드라인

1. 각 모듈 경계에서 `change_context`로 해당 모듈의 에러 타입으로 변환
2. `attach` / `attach_with`로 디버깅 정보 추가 (URL, 심볼, 타임스탬프 등)
3. 에러를 절대 무시하지 않음 — 처리하거나 명시적으로 전파
4. error-stack 0.6: `Context` trait과 `error_stack::Result`는 deprecated; `derive_more`를 통해 `std::error::Error`를 직접 사용
5. **`map_err` 대신 `change_context` + `attach` 패턴 우선 사용**
   - 외부 라이브러리 에러(`std::error::Error` 구현체)는 `Report::new(원본_에러).change_context(앱_에러)`로 변환하여 원본 에러 체인을 보존
   - 추가 디버깅 정보(HTTP 상태 코드, URL 등)는 `attach`로 별도 첨부
   - 원본 에러가 없는 경우(HTTP 상태 코드 체크 등)에만 `Report::new(앱_에러).attach(...)` 직접 생성
   - `map_err(|e| Report::new(AppError { msg: format!("... {e}") }))` 패턴은 원본 에러 체인을 소실시키므로 지양
6. error-stack 0.6 API 변경: `attach_printable`은 deprecated → `attach` 사용
