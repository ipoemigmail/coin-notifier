# 04. 설정

## 파일 형식

TOML, 앱 시작 시 로드. 런타임 변경 시 재시작 필요.

## 파일 경로

CLI 인자로 지정: `--config <경로>` (기본값: `config.toml`)

## 스키마

```toml
[general]
log_level = "info"                   # trace, debug, info, warn, error
log_format = "text"                  # "text" | "json"
data_dir = "./data"                  # SQLite DB 파일 위치
historical_candles = 500             # 시작 시 수집할 과거 캔들 수
default_cooldown_minutes = 5         # 알림별 미지정 시 기본 쿨다운 (분)

[[exchanges]]
name = "upbit"
enabled = true
base_url = "https://api.upbit.com"
ws_url = "wss://api.upbit.com/websocket/v1"

[[exchanges]]
name = "binance"
enabled = true
base_url = "https://api.binance.com"
ws_url = "wss://stream.binance.com:9443"

[[coins]]
exchange = "upbit"                   # exchanges[].name과 일치해야 함
symbol = "KRW-BTC"                   # 거래소별 심볼 형식
timeframes = ["1m", "5m", "1h"]      # 추적할 캔들 타임프레임

[[coins]]
exchange = "binance"
symbol = "BTCUSDT"
timeframes = ["1m", "5m", "1h"]

[[alerts]]
name = "BTC RSI oversold"            # 고유 알림 이름
exchange = "upbit"                   # 대상 거래소
symbol = "KRW-BTC"                   # 대상 심볼
indicator = "rsi"                    # 지표 이름
params = { period = 14 }             # 지표별 파라미터
condition = "below"                  # 조건 타입
threshold = 30.0                     # 임계값
cooldown_minutes = 10                # 선택: general.default_cooldown_minutes 오버라이드
```

## Rust 구조체

```rust
// src/config.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    #[serde(default)]
    pub exchanges: Vec<ExchangeConfig>,
    #[serde(default)]
    pub coins: Vec<CoinConfig>,
    #[serde(default)]
    pub alerts: Vec<AlertConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,             // "text" | "json"
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_historical_candles")]
    pub historical_candles: usize,
    #[serde(default = "default_cooldown_minutes")]
    pub default_cooldown_minutes: u64,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeConfig {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub base_url: String,
    pub ws_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CoinConfig {
    pub exchange: String,
    pub symbol: String,
    pub timeframes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AlertConfig {
    pub name: String,
    pub exchange: String,
    pub symbol: String,
    pub indicator: String,
    #[serde(default)]
    pub params: toml::Table,
    pub condition: String,
    pub threshold: Option<f64>,
    pub cooldown_minutes: Option<u64>,
}
```

## 검증 규칙

1. 각 `coins[].exchange`는 `exchanges[].name` 항목과 일치해야 함
2. 각 `alerts[].exchange` + `alerts[].symbol`은 `coins[]` 항목과 일치해야 함
3. `timeframes` 값은 유효해야 함: `1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `4h`, `1d`
4. `condition`은 다음 중 하나: `above`, `below`, `cross_above`, `cross_below`, `between`
5. 알림 이름은 고유해야 함
6. `above`, `below` 조건에는 `threshold` 필수

## 기본값

| 필드 | 기본값 |
|------|--------|
| `log_level` | `"info"` |
| `log_format` | `"text"` |
| `data_dir` | `"./data"` |
| `historical_candles` | `500` |
| `default_cooldown_minutes` | `5` |
| `enabled` | `true` |

## 테스트

- 단위 테스트: 유효한 설정 파일 파싱
- 단위 테스트: 선택적 필드 누락 시 기본값 적용
- 단위 테스트: 잘못된 거래소 참조 거부
- 단위 테스트: 잘못된 타임프레임 문자열 거부
