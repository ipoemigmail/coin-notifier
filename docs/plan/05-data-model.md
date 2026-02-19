# 05. 데이터 모델

## 위치

`src/model.rs`

## 타입

### ExchangeKind

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExchangeKind {
    Upbit,
    Binance,
}

impl fmt::Display for ExchangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Upbit => write!(f, "upbit"),
            Self::Binance => write!(f, "binance"),
        }
    }
}
```

### TimeFrame

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeFrame {
    Min1,
    Min3,
    Min5,
    Min15,
    Min30,
    Hour1,
    Hour4,
    Day1,
}
```

설정 문자열 변환:

| 문자열 | 변형 |
|--------|------|
| `"1m"` | `Min1` |
| `"3m"` | `Min3` |
| `"5m"` | `Min5` |
| `"15m"` | `Min15` |
| `"30m"` | `Min30` |
| `"1h"` | `Hour1` |
| `"4h"` | `Hour4` |
| `"1d"` | `Day1` |

거래소별 값 변환:

| TimeFrame | Upbit REST 엔드포인트 | Binance kline interval |
|-----------|----------------------|----------------------|
| Min1 | `/v1/candles/minutes/1` | `1m` |
| Min3 | `/v1/candles/minutes/3` | `3m` |
| Min5 | `/v1/candles/minutes/5` | `5m` |
| Min15 | `/v1/candles/minutes/15` | `15m` |
| Min30 | `/v1/candles/minutes/30` | `30m` |
| Hour1 | `/v1/candles/minutes/60` | `1h` |
| Hour4 | `/v1/candles/minutes/240` | `4h` |
| Day1 | `/v1/candles/days` | `1d` |

### Candle

```rust
#[derive(Debug, Clone)]
pub struct Candle {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub timeframe: TimeFrame,
    pub open_time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}
```

### Ticker

```rust
#[derive(Debug, Clone)]
pub struct Ticker {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub price: f64,
    pub volume: f64,
    pub timestamp: DateTime<Utc>,
}
```

### Trade

```rust
#[derive(Debug, Clone)]
pub struct Trade {
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub price: f64,
    pub volume: f64,
    pub side: TradeSide,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}
```

## 심볼 형식

심볼은 거래소별 고유 형식 그대로 저장:

| 거래소 | 형식 | 예시 |
|--------|------|------|
| Upbit | `{호가통화}-{기준통화}` (대문자, 하이픈) | `KRW-BTC` |
| Binance | `{기준통화}{호가통화}` (대소문자 API마다 상이) | `BTCUSDT` (REST), `btcusdt` (WS 스트림명) |

## 테스트

- 단위 테스트: TimeFrame 문자열 파싱 왕복(round-trip)
- 단위 테스트: ExchangeKind display
- 단위 테스트: 모든 타입의 serde 직렬화/역직렬화
