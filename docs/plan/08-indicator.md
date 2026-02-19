# 08. 기술적 분석 지표

## 개요

위치: `src/indicator.rs` (trait) + `src/indicator/*.rs` (구현)

## Indicator Trait

```rust
// src/indicator.rs
pub mod rsi;
pub mod ma;
pub mod macd;
pub mod bollinger;
pub mod volume;

use error_stack::Report;
use crate::error::IndicatorError;
use crate::model::Candle;

pub trait Indicator {
    /// 지표의 고유 이름 (예: "rsi", "sma", "ema")
    fn name(&self) -> &str;

    /// 최소 1개 출력값을 생성하기 위해 필요한 최소 캔들 수
    fn required_candles(&self) -> usize;

    /// 캔들로부터 지표 값을 계산.
    /// 캔들은 시간순 오름차순 (가장 오래된 것부터).
    /// 출력 포인트당 하나의 계산된 값을 Vec으로 반환.
    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>>;
}
```

## 구현 우선순위

| 우선순위 | 지표 | 파일 |
|----------|------|------|
| 1 (MVP) | RSI | `indicator/rsi.rs` |
| 2 | SMA / EMA | `indicator/ma.rs` |
| 3 | MACD | `indicator/macd.rs` |
| 4 | 볼린저 밴드 | `indicator/bollinger.rs` |
| 5 | 거래량 분석 | `indicator/volume.rs` |

---

## RSI (상대강도지수) — MVP

파일: `src/indicator/rsi.rs`

### 파라미터

| 파라미터 | 기본값 | 설명 |
|----------|--------|------|
| `period` | 14 | 룩백 기간 |

### 공식

1. 가격 변동 계산: `delta[i] = close[i] - close[i-1]`
2. 상승/하락 분리:
   - `gain[i] = max(delta[i], 0)`
   - `loss[i] = max(-delta[i], 0)`
3. 첫 평균 (단순): `avg_gain = mean(gain[1..period+1])`, `avg_loss = mean(loss[1..period+1])`
4. 이후 평균 (Wilder 스무딩):
   - `avg_gain = (prev_avg_gain * (period - 1) + gain[i]) / period`
   - `avg_loss = (prev_avg_loss * (period - 1) + loss[i]) / period`
5. RS = `avg_gain / avg_loss`
6. RSI = `100 - 100 / (1 + RS)`
7. `avg_loss == 0`이면: RSI = 100

### 필요 캔들 수

최소 `period + 1` (예: period=14이면 15개)

### 출력

인덱스 `period`부터 캔들당 하나의 RSI 값.

---

## SMA (단순이동평균)

파일: `src/indicator/ma.rs`

### 공식

`SMA[i] = sum(close[i-period+1..=i]) / period`

### 필요 캔들 수

`period`

---

## EMA (지수이동평균)

파일: `src/indicator/ma.rs` (SMA와 같은 파일)

### 공식

1. `k = 2.0 / (period + 1) as f64`
2. 첫 EMA = 처음 `period`개 캔들의 SMA
3. `EMA[i] = close[i] * k + EMA[i-1] * (1 - k)`

### 필요 캔들 수

`period`

---

## MACD (이동평균수렴확산)

파일: `src/indicator/macd.rs`

### 파라미터

| 파라미터 | 기본값 | 설명 |
|----------|--------|------|
| `fast_period` | 12 | 빠른 EMA 기간 |
| `slow_period` | 26 | 느린 EMA 기간 |
| `signal_period` | 9 | 시그널선 EMA 기간 |

### 공식

1. `macd_line = EMA(fast_period) - EMA(slow_period)`
2. `signal_line = EMA(macd_line, signal_period)`
3. `histogram = macd_line - signal_line`

### 필요 캔들 수

`slow_period + signal_period` (기본값: 35)

### 출력

MACD 라인 값 반환. 시그널과 히스토그램은 별도 메서드 또는 튜플로 반환 가능.

---

## 볼린저 밴드

파일: `src/indicator/bollinger.rs`

### 파라미터

| 파라미터 | 기본값 | 설명 |
|----------|--------|------|
| `period` | 20 | SMA 기간 |
| `std_dev_multiplier` | 2.0 | 표준편차 배수 |

### 공식

1. `middle = SMA(period)`
2. `std_dev = sqrt(sum((close[i] - middle)^2) / period)`
3. `upper = middle + std_dev_multiplier * std_dev`
4. `lower = middle - std_dev_multiplier * std_dev`

### 필요 캔들 수

`period`

### 출력

중간 밴드 값 반환. 상단/하단은 함께 계산 가능.

---

## 거래량 분석

파일: `src/indicator/volume.rs`

### 유형

1. **거래량 MA**: 거래량의 단순이동평균
2. **거래량 급등**: 현재 거래량이 평균의 N배를 초과하는지 감지

### 파라미터

| 파라미터 | 기본값 | 설명 |
|----------|--------|------|
| `period` | 20 | 거래량 MA 기간 |
| `surge_multiplier` | 2.0 | 급등 감지 배수 |

### 공식

1. `vol_ma = sum(volume[i-period+1..=i]) / period`
2. `is_surge = volume[i] > vol_ma * surge_multiplier`

### 출력

거래량 MA 값 반환. 급등 감지는 비교를 통한 불리언으로 도출.

---

## 테스트

각 지표별:

- 단위 테스트: 알려진 데이터셋으로 정확도 검증 (예: Investopedia 예시 데이터)
- 단위 테스트: 데이터 부족 시 `IndicatorError::InsufficientData` 반환
- 단위 테스트: 잘못된 파라미터 (period=0) 시 `IndicatorError::InvalidParameter` 반환
- 단위 테스트: 모든 가격이 동일한 엣지 케이스
