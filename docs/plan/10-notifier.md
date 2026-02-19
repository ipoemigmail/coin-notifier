# 10. 알림기 — 알림 출력

## 개요

위치: `src/notifier.rs` (trait) + `src/notifier/terminal.rs` (구현)

## Notifier Trait

```rust
// src/notifier.rs
pub mod terminal;

use crate::strategy::EvaluationResult;
use crate::model::ExchangeKind;

pub trait Notifier: Send + Sync {
    fn notify(
        &self,
        exchange: ExchangeKind,
        symbol: &str,
        price: f64,
        result: &EvaluationResult,
    );
}
```

## 터미널 알림기

`src/notifier/terminal.rs`에 위치.

일반 로그와 시각적으로 구분하기 위해 `tracing::warn!` 레벨 사용.

### 출력 형식

```
[2026-02-20 15:30:00 UTC] ALERT [Upbit] KRW-BTC
  BTC RSI oversold | RSI(14) = 28.5 | Price: 120,500,000
```

### 구현

```rust
pub struct TerminalNotifier;

impl Notifier for TerminalNotifier {
    fn notify(
        &self,
        exchange: ExchangeKind,
        symbol: &str,
        price: f64,
        result: &EvaluationResult,
    ) {
        tracing::warn!(
            exchange = %exchange,
            symbol = symbol,
            alert = result.alert_name,
            indicator_value = result.indicator_value,
            price = price,
            "ALERT: {}",
            result.message,
        );
    }
}
```

## 로깅 설정

`general.log_format`으로 구성:

| 값 | 동작 |
|----|------|
| `"text"` | `tracing_subscriber::fmt()` 기본 텍스트 형식 |
| `"json"` | `tracing_subscriber::fmt().json()` 구조적 JSON 출력 |

로그 레벨은 `general.log_level` → `EnvFilter`로 설정.

### 초기화 (main.rs에서)

```rust
match config.general.log_format.as_str() {
    "json" => {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::new(&config.general.log_level))
            .init();
    }
    _ => {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(&config.general.log_level))
            .init();
    }
}
```

## 알림 영속화

알림 후, `Storage::log_alert()`를 통해 `alerts_log` 테이블에 기록.
두 가지 목적:

1. 쿨다운 적용 (09-strategy.md 참고)
2. 트리거된 모든 알림의 이력 기록

## 향후 확장 포인트

`Notifier` trait은 확장성을 고려하여 설계됨:

- `TelegramNotifier` — Telegram Bot API로 전송
- `SlackNotifier` — Slack 웹훅으로 전송
- `DiscordNotifier` — Discord 웹훅으로 전송

MVP 범위 외이나 `Notifier` trait을 구현하여 추가 가능.
