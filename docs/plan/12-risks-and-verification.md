# 12. 리스크, 완화 전략, 검증 계획

## 리스크 및 완화 전략

| # | 리스크 | 영향 | 완화 전략 |
|---|--------|------|-----------|
| 1 | 거래소 API 스펙 또는 Rate limit 정책 변경 | 데이터 수집 중단 | `dyn Exchange` 추상화로 변경 격리; URL과 rate limit을 TOML에서 설정 가능 |
| 2 | WebSocket 연결 불안정 (네트워크 장애, 서버 점검) | 실시간 데이터 누락 | 지수 백오프 재연결 (1s → 60s 최대); REST API 폴백으로 데이터 갭 보충 |
| 3 | 기술적 분석 지표 계산 오류로 잘못된 알림 | 잘못된 매매 시그널 | 알려진 데이터셋 기반 단위 테스트; 쿨다운으로 중복 알림 방지 |
| 4 | 다수 코인 동시 수집 시 SQLite 동시 쓰기 성능 문제 | 데이터 삽입 지연 | WAL 모드 활성화; 배치 삽입; 필요 시 전용 writer 태스크 분리 |
| 5 | Upbit WebSocket Origin 헤더 포함 시 10초당 1회 제한 | 심각한 데이터 수신 제한 | WebSocket 연결에 Origin 헤더 미포함 |
| 6 | Binance WebSocket 24시간 자동 종료 | 예기치 않은 데이터 갭 | 23시간 경과 시 타이머로 사전 재연결 |
| 7 | governor crate가 edition 2024와 비호환 | 빌드 실패 | **해결됨**: governor 0.10.4가 edition 2024와 정상 호환 확인. Upbit(초당 8회) / Binance(초당 20회) rate limiter 적용 완료 |
| 8 | derive_more Error derive의 복잡한 에러 타입 처리 한계 | 컴파일 오류 또는 trait impl 누락 | 필요 시 수동 `impl std::error::Error`로 대체 |
| 9 | error-stack 0.6 deprecated API (Context, Result) 실수로 사용 | 컴파일러 경고, 향후 호환성 깨짐 | derive_more를 통해 `std::error::Error`만 사용; deprecated 항목 import 금지 |

## 검증 계획

### 단위 테스트

| 컴포넌트 | 테스트 케이스 |
|----------|-------------|
| 설정 파싱 (`config.rs`) | 유효한 설정 정상 파싱; 선택 필드 누락 시 기본값 적용; 잘못된 거래소 참조 거부; 잘못된 타임프레임 거부 |
| 에러 타입 (`error.rs`) | 에러 생성; `change_context` 전파; `attach` / `attach_with` 컨텍스트 추가 |
| 데이터 모델 (`model.rs`) | serde 직렬화 왕복; TimeFrame 문자열 파싱; ExchangeKind display |
| RSI (`indicator/rsi.rs`) | 알려진 데이터셋 정확도; 데이터 부족 에러; period=0 에러; 모든 가격 동일 엣지 케이스 |
| SMA/EMA (`indicator/ma.rs`) | 알려진 데이터셋 정확도; period 경계값; 데이터 부족 |
| MACD (`indicator/macd.rs`) | 알려진 데이터셋 정확도; 기본 파라미터 |
| 볼린저 밴드 (`indicator/bollinger.rs`) | 알려진 데이터셋 정확도; 밴드 폭 |
| 거래량 (`indicator/volume.rs`) | 거래량 MA 계산; 급등 감지 |
| 조건 평가 (`strategy/condition.rs`) | 각 조건 타입 정상 트리거; cross 조건은 이전 값 필요; between 범위 검증 |
| 쿨다운 로직 | 쿨다운 시간 내 재트리거 차단; 경과 후 허용 |
| SQLite CRUD (`storage/sqlite.rs`) | 캔들 upsert + 조회; 체결 삽입; 알림 로그 + last_alert_time; upsert 중복 제거 (인메모리 `:memory:` DB) |

### 통합 테스트 (`#[ignore]`)

| 테스트 | 설명 |
|--------|------|
| Upbit REST 캔들 | KRW-BTC 200 캔들 수집, 구조 검증 |
| Binance REST kline | BTCUSDT 500 kline 수집, 구조 검증 |
| Upbit WebSocket | 연결, ticker 구독, 최소 1개 메시지 수신, 종료 |
| Binance WebSocket | 연결, ticker 구독, 최소 1개 메시지 수신, 종료 |
| 과거 수집 파이프라인 | 과거 데이터 수집 → SQLite 저장 → 조회하여 건수 검증 |

### E2E / 수동 테스트

| 테스트 | 절차 | 기대 결과 |
|--------|------|-----------|
| 전체 파이프라인 | 유효한 설정으로 앱 실행, 알림 대기 | 데이터 수집, 지표 계산, 터미널에 알림 출력 |
| 네트워크 끊김 | 앱 시작, 네트워크 끊김, 재연결 | 끊김 로그, 자동 재연결, 데이터 수집 재개 |
| Graceful shutdown | 앱 시작, Ctrl+C | 모든 태스크 정상 종료, 패닉 없음, DB 정상 닫힘 |
| 잘못된 설정 | 잘못된 TOML로 앱 실행 | 설정 파싱 실패를 보여주는 error-stack 트리 포함 명확한 에러 메시지 |
| 설정 파일 없음 | 존재하지 않는 경로로 앱 실행 | 파일 없음을 나타내는 명확한 에러 메시지 |

### 린트 및 빌드

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo build --release
cargo test
cargo test -- --ignored    # 통합 테스트 (네트워크 필요)
```

## 테스트 실행 순서

1. `cargo fmt --check` — 포맷팅
2. `cargo clippy -- -D warnings` — 린트
3. `cargo test` — 단위 테스트
4. `cargo build --release` — 릴리스 빌드
5. `cargo test -- --ignored` — 통합 테스트 (수동, 네트워크 필요)
6. 수동 E2E 테스트 — 전체 파이프라인 검증
