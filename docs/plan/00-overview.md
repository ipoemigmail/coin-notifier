# 00. 프로젝트 개요

## 목표

Rust(edition 2024) + Tokio 기반으로 다음을 수행하는 CLI 애플리케이션을 구축한다:

1. Upbit/Binance 거래소에서 WebSocket을 통해 실시간 코인 시세 데이터를 수집
2. REST API를 통해 과거 캔들(OHLCV) 데이터를 수집
3. 기술적 분석 지표를 계산 (MVP: RSI)
4. 사용자가 정의한 알림 조건을 평가
5. 매매 타이밍 알림을 터미널에 출력

## 기술 스택

| 구성 요소 | 선택 | 이유 |
|-----------|------|------|
| 언어 | Rust (edition 2024) | 성능, 안전성, 네이티브 async fn in trait |
| 비동기 런타임 | Tokio | 사실상 표준, 완전한 기능 |
| HTTP 클라이언트 | reqwest 0.13 | Tokio 네이티브, JSON 지원, 가장 널리 사용 |
| WebSocket | tokio-tungstenite 0.28 | Tokio 네이티브, WebSocket crate 1위 |
| 데이터베이스 | SQLite (sqlx 0.8) | 비동기 네이티브, 컴파일 타임 쿼리 검증, 내장 마이그레이션 |
| 직렬화 | serde + serde_json | 업계 표준 |
| 설정 | toml 0.8 | 단순, serde 기반 |
| 에러 처리 | error-stack 0.6 + derive_more 2 | 컨텍스트 기반 에러 전파, anyhow/thiserror 미사용 |
| CLI | clap 4 | derive 기반 인자 파싱 |
| 로깅 | tracing + tracing-subscriber | 구조적 로깅, text/json 선택 가능 |
| 날짜/시간 | chrono 0.4 | serde 통합, UTC 지원 |
| Future 유틸리티 | futures 0.3 | BoxFuture 타입 별칭, StreamExt/SinkExt 유틸리티 |
| Rate Limiting | governor 0.8 | Token bucket 알고리즘 |
| UUID | uuid 1 | Upbit WebSocket ticket ID 생성 |

## 핵심 설계 결정

| 결정 | 선택 | 검토한 대안 |
|------|------|------------|
| async trait | 네이티브 async fn (async_trait crate 미사용) | async_trait crate |
| Trait 다형성 | `Pin<Box<dyn Future>>` (futures::future::BoxFuture) + `dyn Exchange` | enum 디스패치, 제네릭, trait-variant crate |
| 에러 처리 | error-stack + derive_more | anyhow, thiserror |
| 모듈 레이아웃 | `a.rs` + `a/` 디렉토리 방식 | `a/mod.rs` 방식 |

## 구현 단계

| 단계 | 스텝 | 설명 |
|------|------|------|
| 1 | Step 1-4 | 프로젝트 기반: 의존성, 에러, 설정, 데이터 모델 |
| 2 | Step 5-7 | 데이터 수집: Exchange trait, Upbit, Binance, 과거 데이터 수집기 |
| 3 | Step 8 | 저장소: SQLite 스키마, CRUD, 마이그레이션 |
| 4 | Step 9-10 | 분석: 지표(RSI 우선), 조건 평가 엔진 |
| 5 | Step 11-12 | 통합: 터미널 알림, 메인 파이프라인 조립 |
| 6 | Step 13 | 안정화: 재연결, 재시도, 에러 컨텍스트 보강 |
