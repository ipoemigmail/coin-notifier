# 01. 범위 및 가정

## 범위 내 (In Scope)

- **데이터 수집**: Upbit/Binance WebSocket(ticker/trade) 실시간 스트림 + REST API 과거 캔들 초기 수집 + trade 기반 1m 캔들 실시간 동기화
- **거래소 추상화**: `BoxFuture`를 사용한 `dyn Exchange` trait 기반 동적 디스패치
- **데이터 저장**: SQLite(sqlx)에 캔들, 체결, 알림 이력 저장
- **기술적 분석**: `Indicator` trait 기반 지표 (MVP: RSI, 이후 SMA/EMA, MACD, 볼린저 밴드, 거래량)
- **알림 시스템**: TOML 설정 기반 알림 조건 + 터미널 출력
- **캔들 타임프레임**: 1m, 3m, 5m, 15m, 30m, 1h, 4h, 1d
- **로그 형식**: 설정으로 text/json 선택 가능
- **Rate Limiting**: 거래소별 API 호출 제한 (governor)
- **Graceful Shutdown**: Ctrl+C를 CancellationToken으로 처리

## 범위 외 (Out of Scope)

- 자동 매매 (주문 실행)
- 웹 UI / GUI
- 외부 메신저 알림 (Telegram, Slack, Discord) — 추후 확장
- ML/DL 기반 예측
- 백테스팅 엔진
- 멀티 유저 지원
- Docker 배포 구성

## 가정

- Rust edition 2024, stable 툴체인
- Upbit/Binance 시세(공개 시장 데이터) API는 인증 불필요
- 단일 프로세스, 단일 사용자 환경
- 설정 파일은 앱 시작 시 로드; 런타임 변경 시 재시작 필요
- `Box::pin()` 힙 할당 오버헤드는 네트워크 IO 대비 무시 가능
- 앱 시작 시 과거 500 캔들 수집
- 알림 쿨다운 기본값 5분 (설정 미지정 시)
