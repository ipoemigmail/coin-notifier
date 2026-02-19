# 11. 메인 파이프라인 통합

## 개요

`src/main.rs`에 위치. 모든 컴포넌트를 런타임 파이프라인으로 조립.

## CLI 인자

`clap` derive 사용:

```rust
#[derive(clap::Parser)]
#[command(name = "coin-notifier", about = "Coin trading signal notifier")]
struct Cli {
    /// 설정 파일 경로
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}
```

## 시작 순서

```
1. CLI 인자 파싱 (clap)
2. 설정 로드 및 검증 (config.rs)
3. tracing-subscriber 초기화 (설정에 따라 text 또는 json)
4. SQLite 데이터베이스 초기화
   - data_dir 없으면 생성
   - create_if_missing(true)로 연결
   - sqlx 마이그레이션 실행
5. CancellationToken 생성
6. exchanges: Vec<Arc<dyn Exchange>> 구성
   - 설정의 활성화된 각 거래소에 대해 UpbitExchange 또는 BinanceExchange 인스턴스 생성
   - 각 거래소는 내부에 governor rate limiter를 보유
7. 데이터 흐름용 mpsc 채널 생성
8. 과거 데이터 수집 태스크 spawn (모든 거래소 병렬)
   - 거래소별, 코인별, 타임프레임별 조합마다 tokio::spawn
   - rate limiting은 각 Exchange 내부의 governor가 자동 관리
9. 모든 과거 수집 태스크 완료 대기
10. WebSocket 구독 태스크 spawn (거래소별)
11. 분석 루프 태스크 spawn
12. ctrl_c 대기 → cancel token → 모든 태스크 join
```

## 태스크 구조

```
main()
  │
  ├─ [태스크들] 과거 수집 (모든 거래소 병렬)
  │    └─ 거래소별 × 코인별 × 타임프레임별 조합마다 독립 spawn
  │       └─ 500 캔들 수집 → DB 저장
  │       └─ rate limiting은 Exchange 내부의 governor가 자동 관리
  │
  │  (WS 시작 전 모든 과거 수집 태스크 완료 대기)
  │
  ├─ [태스크] Upbit WebSocket
  │    └─ subscribe_ticker → mpsc::Sender<Ticker>
  │
  ├─ [태스크] Binance WebSocket
  │    └─ subscribe_ticker → mpsc::Sender<Ticker>
  │
  ├─ [태스크] 분석 루프
  │    └─ mpsc::Receiver<Ticker>
  │       각 ticker에 대해:
  │         1. 일치하는 각 AlertRule에 대해:
  │            a. DB에서 최근 캔들 조회
  │            b. 지표 계산
  │            c. 조건 평가
  │            d. 쿨다운 확인
  │            e. 트리거 시 → 알림 + 알림 로그 기록
  │
  └─ [종료] ctrl_c
       └─ CancellationToken::cancel()
       └─ spawn된 모든 태스크 join
```

## 채널 설계

```rust
let (ticker_tx, ticker_rx) = tokio::sync::mpsc::channel::<Ticker>(1024);

// 각 거래소는 ticker_tx의 clone을 받음
// 분석 루프는 ticker_rx에서 읽음
```

## Graceful Shutdown

```rust
use tokio_util::sync::CancellationToken;

let cancel = CancellationToken::new();

// 각 장기 실행 태스크에서:
tokio::select! {
    _ = cancel.cancelled() => {
        tracing::info!("종료 중...");
        break;
    }
    msg = ws_stream.next() => {
        // 메시지 처리
    }
}

// main에서:
tokio::signal::ctrl_c().await?;
tracing::info!("ctrl+c 수신, 종료 시작");
cancel.cancel();

// 타임아웃과 함께 모든 태스크 join
for handle in task_handles {
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
}
```

## main의 에러 처리

```rust
#[tokio::main]
async fn main() {
    if let Err(report) = run().await {
        eprintln!("{report:?}");  // error-stack 형식 출력
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Report<AppError>> {
    // 실제 로직
}
```

`AppError`는 최상위 에러 타입:

```rust
#[derive(Debug, Display, Error)]
pub enum AppError {
    #[display("configuration error")]
    Config,
    #[display("storage error")]
    Storage,
    #[display("exchange error")]
    Exchange,
    #[display("runtime error")]
    Runtime,
}
```

하위 에러는 `change_context(AppError::Config)` 등으로 변환.
