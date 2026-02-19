# 02. 아키텍처

## 모듈 구조

`a.rs` + `a/` 디렉토리 방식을 사용한다 (`a/mod.rs` 방식 아님).

```
src/
├── main.rs                  # 진입점: CLI 파싱, tokio 런타임, 파이프라인 조립
├── config.rs                # TOML 설정 파싱 및 검증
├── error.rs                 # 앱 에러 타입 (derive_more + error-stack)
├── model.rs                 # 공통 데이터 모델: Candle, Trade, Ticker, TimeFrame 등
├── exchange.rs              # Exchange trait 정의, BoxFuture re-export
├── exchange/
│   ├── upbit.rs             # Upbit REST + WebSocket 구현
│   └── binance.rs           # Binance REST + WebSocket 구현
├── storage.rs               # Storage trait 정의
├── storage/
│   └── sqlite.rs            # SQLite 구현 (sqlx)
├── indicator.rs             # Indicator trait 정의
├── indicator/
│   ├── rsi.rs               # RSI (MVP 최우선)
│   ├── ma.rs                # SMA / EMA
│   ├── macd.rs              # MACD
│   ├── bollinger.rs         # 볼린저 밴드
│   └── volume.rs            # 거래량 분석
├── strategy.rs              # 조건 타입, 평가 로직
├── strategy/
│   └── condition.rs         # 설정 기반 조건 평가
├── notifier.rs              # Notifier trait 정의
└── notifier/
    └── terminal.rs          # 터미널 출력 구현
```

## Cargo.toml 의존성

```toml
[package]
name = "coin-notifier"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.13", features = ["json", "query"] }
tokio-tungstenite = { version = "0.28", features = ["rustls-tls-native-roots"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "migrate", "chrono"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
error-stack = "0.6"
derive_more = { version = "2", features = ["display", "error"] }
uuid = { version = "1", features = ["v4"] }
futures = "0.3"
governor = { version = "0.10", features = ["std"] }
nonzero_ext = "0.3"
tokio-util = { version = "0.7", features = ["rt"] }
```

## 데이터 흐름

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                              │
│  CLI 파싱 → 설정 로드 → DB 초기화 → CancellationToken        │
└──────┬──────────────────────────────────────┬───────────────┘
       │                                      │
       ▼                                      ▼
┌──────────────┐                    ┌──────────────────┐
│ 과거 데이터   │                    │ WebSocket         │
│ 수집기        │                    │ 구독자             │
│ (거래소별)    │                    │ (거래소별)          │
└──────┬───────┘                    └────────┬─────────┘
       │                                     │
       │      mpsc::Sender<Candle/Ticker>    │
       └──────────────┬──────────────────────┘
                      ▼
              ┌───────────────┐
              │  분석 루프     │
              │               │
              │ 1. DB 저장    │
              │ 2. 지표 계산  │
              │ 3. 조건 평가  │
              │ 4. 알림 출력  │
              └───────┬───────┘
                      │
              ┌───────▼───────┐
              │   터미널      │
              │   알림기      │
              └───────────────┘
```

## 동시성 모델

- 각 거래소는 독립적인 `tokio::spawn` 태스크로 실행
- 태스크 간 통신은 `tokio::sync::mpsc` 채널 사용
- `tokio_util::sync::CancellationToken`으로 graceful shutdown 조율
- `tokio::signal::ctrl_c()`로 취소 트리거
- 거래소별 REST API rate limiting은 `governor` crate의 `DefaultDirectRateLimiter`로 관리
  - Upbit: 초당 8회 (실제 한도 10회, 안전 마진 적용)
  - Binance: 초당 20회 (kline weight=2, 분당 ~6000 weight 한도 대비 안전 마진)
  - rate limiter는 `Arc`로 공유되어 병렬 태스크에서도 전체 한도를 준수
