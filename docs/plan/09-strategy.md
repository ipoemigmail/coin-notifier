# 09. 전략 — 조건 평가 엔진

## 개요

위치: `src/strategy.rs` (타입) + `src/strategy/condition.rs` (평가 로직)

TOML 설정에 정의된 알림 조건을 계산된 지표 값과 비교하여 평가.

## 조건 타입

| 조건 | 설명 | 필요 설정 필드 |
|------|------|---------------|
| `above` | 지표 값 > 임계값 | `threshold` |
| `below` | 지표 값 < 임계값 | `threshold` |
| `cross_above` | 이전 값 <= 임계값 AND 현재 값 > 임계값 | `threshold` |
| `cross_below` | 이전 값 >= 임계값 AND 현재 값 < 임계값 | `threshold` |
| `between` | low < 지표 값 < high | `threshold` (하한), `threshold_high` (상한) |

### Rust 표현

```rust
// src/strategy.rs
pub mod condition;

#[derive(Debug, Clone)]
pub enum ConditionType {
    Above(f64),
    Below(f64),
    CrossAbove(f64),
    CrossBelow(f64),
    Between { low: f64, high: f64 },
}

#[derive(Debug, Clone)]
pub struct AlertRule {
    pub name: String,
    pub exchange: ExchangeKind,
    pub symbol: String,
    pub indicator_name: String,
    pub indicator_params: IndicatorParams,
    pub condition: ConditionType,
    pub cooldown_minutes: u64,
}

#[derive(Debug, Clone)]
pub struct IndicatorParams {
    pub period: Option<usize>,
    pub fast_period: Option<usize>,
    pub slow_period: Option<usize>,
    pub signal_period: Option<usize>,
    pub std_dev_multiplier: Option<f64>,
    pub surge_multiplier: Option<f64>,
}
```

## 평가 로직

`src/strategy/condition.rs`에 위치.

```rust
pub struct EvaluationResult {
    pub triggered: bool,
    pub alert_name: String,
    pub indicator_value: f64,
    pub message: String,
}

pub fn evaluate(
    rule: &AlertRule,
    current_value: f64,
    previous_value: Option<f64>,
) -> EvaluationResult;
```

### 평가 규칙

```
above(threshold):
    triggered = current_value > threshold

below(threshold):
    triggered = current_value < threshold

cross_above(threshold):
    triggered = previous_value.is_some()
             && previous_value.unwrap() <= threshold
             && current_value > threshold

cross_below(threshold):
    triggered = previous_value.is_some()
             && previous_value.unwrap() >= threshold
             && current_value < threshold

between(low, high):
    triggered = current_value > low && current_value < high
```

## 쿨다운 로직

알림 트리거 전:

1. `storage.last_alert_time(alert_name)` 조회
2. `last_triggered + cooldown_minutes > now` 이면 → 건너뜀
3. 아니면 → 트리거하고 `alerts_log`에 기록

```rust
pub async fn should_alert(
    storage: &dyn Storage,
    rule: &AlertRule,
) -> Result<bool, Report<StorageError>> {
    let last_time = storage.last_alert_time(&rule.name).await?;
    match last_time {
        Some(t) if Utc::now() - t < Duration::minutes(rule.cooldown_minutes as i64) => Ok(false),
        _ => Ok(true),
    }
}
```

## 알림 파이프라인 (실시간 이벤트 기준)

```
1. 새 trade 도착 (exchange, symbol, price, volume, timestamp)
2. trade 동기화 루프가 1m 캔들 갱신 후 DB upsert
3. 새 ticker 도착 (exchange, symbol)
4. DB에서 최근 1m 캔들 조회 (지표 계산에 충분한 수)
5. 해당 (exchange, symbol)과 일치하는 각 AlertRule에 대해:
   a. rule.indicator_name + rule.indicator_params로 지표 인스턴스 생성
   b. 캔들로부터 지표 값 계산
   c. current_value (마지막)와 previous_value (끝에서 두 번째) 추출
   d. 조건 평가
   e. 트리거되고 쿨다운 통과 시 → 알림 발생
```

현재 구현은 분석 시 타임프레임을 `TimeFrame::Min1`로 고정하여 사용한다.

## 설정에서 AlertRule 변환

TOML의 `AlertConfig`를 `AlertRule`로 파싱:

- `config.condition` 문자열 → `ConditionType` 열거형 매핑
- `config.indicator` 문자열 → 지표 이름 매핑
- `config.params` TOML 테이블 → `IndicatorParams` 매핑
- `config.cooldown_minutes.unwrap_or(general.default_cooldown_minutes)` 사용

## 테스트

- 단위 테스트: `above` 조건 — 값이 임계값 초과 시 트리거
- 단위 테스트: `below` 조건 — 값이 임계값 미만 시 트리거
- 단위 테스트: `cross_above` — 이전 <= 임계값이고 현재 > 임계값일 때 트리거
- 단위 테스트: `cross_above` — 두 값 모두 임계값 이상이면 트리거 안 됨
- 단위 테스트: `between` — 범위 내에서만 트리거
- 단위 테스트: 쿨다운이 시간 내 재트리거 방지
- 단위 테스트: 쿨다운 경과 후 재트리거 허용
