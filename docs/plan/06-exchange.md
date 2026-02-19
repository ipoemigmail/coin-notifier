# 06. Exchange Trait 및 거래소 구현

## Exchange Trait

`src/exchange.rs`에 위치. dyn 호환을 위해 `futures::future::BoxFuture` 사용
(`async_trait` crate 미사용).

```rust
// src/exchange.rs
pub mod upbit;
pub mod binance;

use futures::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use error_stack::Report;

use crate::error::ExchangeError;
use crate::model::{Candle, ExchangeKind, Ticker, TimeFrame, Trade};

pub trait Exchange: Send + Sync {
    fn kind(&self) -> ExchangeKind;

    fn fetch_candles(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<ExchangeError>>>;

    fn subscribe_ticker(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Ticker>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>>;

    fn subscribe_trades(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<Trade>,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), Report<ExchangeError>>>;
}
```

### 구현 패턴

각 impl은 비동기 로직을 `Box::pin(async move { ... })`으로 감싼다:

```rust
impl Exchange for UpbitExchange {
    fn kind(&self) -> ExchangeKind {
        ExchangeKind::Upbit
    }

    fn fetch_candles(
        &self,
        symbol: &str,
        timeframe: TimeFrame,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<Candle>, Report<ExchangeError>>> {
        Box::pin(async move {
            // 실제 비동기 구현
        })
    }
    // ...
}
```

### 사용법 (동적 디스패치)

```rust
let exchanges: Vec<Arc<dyn Exchange>> = vec![
    Arc::new(UpbitExchange::new()),
    Arc::new(BinanceExchange::new()),
];

for exchange in &exchanges {
    let candles = exchange.fetch_candles("BTC", TimeFrame::Min1, 200).await?;
}
```

---

## Upbit 구현

`src/exchange/upbit.rs`에 위치.

### Rate Limiter

`governor` crate의 `DefaultDirectRateLimiter`를 사용하여 REST API rate limit을 관리한다.

```rust
pub struct UpbitExchange {
    client: reqwest::Client,
    rate_limiter: Arc<DefaultDirectRateLimiter>,  // 초당 8회 (안전 마진)
}
```

- `fetch_candles_page()` 호출 전 `self.rate_limiter.until_ready().await`로 대기
- Upbit 실제 한도는 IP당 초당 10회이나, 안전 마진을 위해 8회로 설정
- **burst=1로 설정**하여 요청이 ~125ms 간격으로 균일하게 분산됨
  - burst 미제한 시 다수 job이 동시 시작하면 초기 burst로 rate limit 초과 발생
- `Arc`로 래핑하여 병렬 태스크에서 공유 시에도 전체 한도를 준수
- 기존 `sleep(150ms)` / `sleep(500ms)` 하드코딩 방식을 대체

### REST API — 캔들 데이터

| 항목 | 상세 |
|------|------|
| 기본 URL | `https://api.upbit.com` |
| 분 캔들 | `GET /v1/candles/minutes/{unit}` (unit: 1,3,5,10,15,30,60,240) |
| 일 캔들 | `GET /v1/candles/days` |
| 요청당 최대 | 200개 |
| 페이지네이션 | `to` 파라미터 (ISO 8601), 역시간순 |
| Rate limit | IP당 초당 10회 (candle 그룹) |
| 인증 | 시세 데이터는 불필요 |

쿼리 파라미터:
- `market` (필수): 예) `KRW-BTC`
- `count` (선택): 최대 200
- `to` (선택): 마지막 캔들 타임스탬프

Rate limit 응답 헤더: `Remaining-Req: group=candle; min=1800; sec=9`

### WebSocket — 실시간 데이터

| 항목 | 상세 |
|------|------|
| 엔드포인트 | `wss://api.upbit.com/websocket/v1` |
| TLS | 1.2+ (1.3 권장) |
| 압축 | RFC 7692 per-message deflate |
| 유휴 타임아웃 | 120초 (PING 전송으로 유지) |
| 연결 Rate limit | 초당 5회 |
| 메시지 Rate limit | 초당 5회, 분당 100회 |

구독 메시지 형식:
```json
[
  { "ticket": "<uuid>" },
  {
    "type": "ticker",
    "codes": ["KRW-BTC", "KRW-ETH"],
    "is_only_realtime": true
  },
  { "format": "DEFAULT" }
]
```

사용 가능한 type: `ticker`, `trade`, `orderbook`

**중요**: `Origin` 헤더를 포함하지 않아야 함 (포함 시 10초당 1회 제한 발동).

### 재연결 전략

- 연결 끊김 시: 지수 백오프 (1s → 2s → 4s → ... → 60s 최대)
- 유휴 타임아웃: 120초 전에 PING 전송

---

## Binance 구현

`src/exchange/binance.rs`에 위치.

### Rate Limiter

Upbit와 동일하게 `governor`의 `DefaultDirectRateLimiter`를 사용한다.

```rust
pub struct BinanceExchange {
    client: reqwest::Client,
    rate_limiter: Arc<DefaultDirectRateLimiter>,  // 초당 20회 (안전 마진)
}
```

- `fetch_candles()` 호출 전 `self.rate_limiter.until_ready().await`로 대기
- Binance kline 요청의 weight는 2이고, IP당 분당 ~6000 weight 한도
- 초당 20회 = 분당 1200회 × weight 2 = 분당 2400 weight (한도의 40%)로 안전 마진 확보

### REST API — Kline 데이터

| 항목 | 상세 |
|------|------|
| 기본 URL | `https://api.binance.com` (또는 API Key 불필요: `https://data-api.binance.vision`) |
| 엔드포인트 | `GET /api/v3/klines` |
| 요청당 최대 | 1000개 |
| Weight | 요청당 2 |
| 페이지네이션 | `startTime` (밀리초 epoch), 시간순 |
| Rate limit | Weight 기반, IP당 분당 ~6000 weight |
| 인증 | 시세 데이터는 불필요 |

쿼리 파라미터:
- `symbol` (필수): 예) `BTCUSDT`
- `interval` (필수): `1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `2h`, `4h`, `6h`, `8h`, `12h`, `1d`
- `startTime` (선택): 밀리초 epoch
- `endTime` (선택): 밀리초 epoch
- `limit` (선택): 기본 500, 최대 1000

응답: 배열의 배열 (캔들당 12개 요소).

Rate limit 응답 헤더: `X-MBX-USED-WEIGHT-1M`

### WebSocket — 실시간 데이터

| 항목 | 상세 |
|------|------|
| 단일 스트림 | `wss://stream.binance.com:9443/ws/<스트림명>` |
| 복합 스트림 | `wss://stream.binance.com:9443/stream?streams=<s1>/<s2>` |
| 연결 수명 | 24시간 (자동 끊김) |
| 서버 ping | 20초마다; 60초 내 pong 응답 필요 |
| 연결당 최대 스트림 | 1024개 |
| 연결 Rate limit | IP당 5분당 300 연결 |
| 메시지 Rate limit | 초당 5개 |

구독 메시지 형식:
```json
{
  "method": "SUBSCRIBE",
  "params": ["btcusdt@ticker", "ethusdt@trade"],
  "id": 1
}
```

스트림명 (모두 소문자 심볼):
- Ticker: `<symbol>@ticker`
- Trade: `<symbol>@trade`
- Kline: `<symbol>@kline_<interval>`

### 재연결 전략

- 23시간 경과 시 사전 재연결 (24시간 자동 종료 전)
- 연결 끊김 시: 지수 백오프 (1s → 2s → 4s → ... → 60s 최대)
- 서버 ping에 pong으로 응답

---

## 과거 데이터 수집기

앱 시작 시 실행, 코인별/타임프레임별로 `general.historical_candles` (기본 500) 캔들 수집.

### 공통 전략
- **모든 거래소의 과거 수집 태스크를 병렬로 spawn** (코인+타임프레임 조합별 1개 태스크)
- Rate limiting은 각 Exchange 구조체 내부의 `governor` rate limiter가 자동으로 관리
- 기존의 Upbit 전용 순차 처리 / `sleep` 하드코딩 방식을 제거하고 통합

### Upbit 전략
- 요청당 200개씩 `to` 파라미터로 역순 페이지네이션 (최신→과거)
- 500 캔들 = 3회 요청 (200 + 200 + 100)
- rate limiter가 초당 8회로 자동 조절 (병렬 태스크 간 공유)

### Binance 전략
- 요청당 500개(최대 1000) 씩 `startTime`으로 순방향 페이지네이션 (과거→최신)
- 500 캔들 = 1회 요청
- rate limiter가 초당 20회로 자동 조절

### 진행률 로깅
- `tracing::info!`로 진행률 표시 (예: "KRW-BTC 1m 캔들 수집 중: 200/500")

## 테스트

- 통합 테스트 (`#[ignore]`): Upbit REST 캔들 수집
- 통합 테스트 (`#[ignore]`): Binance REST kline 수집
- 통합 테스트 (`#[ignore]`): Upbit WebSocket 연결 + 수신
- 통합 테스트 (`#[ignore]`): Binance WebSocket 연결 + 수신
- 통합 테스트 (`#[ignore]`): 과거 데이터 수집기 전체 흐름
