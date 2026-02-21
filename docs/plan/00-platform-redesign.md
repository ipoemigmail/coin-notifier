# Coin Notifier Platform Redesign Plan

## Objective

Build a model-driven trading platform that is easy to extend in three areas:

1. Add new inputs (features) with minimal changes.
2. Add new models by combining existing inputs.
3. Run repeatable historical backtests with realistic execution assumptions.

## Fixed Decisions

- Execution price rule: `next open`
- Exit rule: full liquidation on sell signal
- End-of-run rule: force-close remaining position at last candle close
- Position sizing: `fixed_percent_equity = 10%`
- Direction: long-only
- Pyramiding policy:
  - backtest: max 3 entries
  - live-only mode: unlimited for now
  - auto-trade phase: max 3 entries
- Pyramiding spacing: `cooldown_bars` configurable, default `3`
- Slippage: fixed bps, configurable, default `10 bps`

## Scope

### In Scope

- Replace legacy alert-centric planning documents with a model-platform plan.
- Introduce input registry (`src/signal_input.rs`) and model registry (`src/signal_model.rs`).
- Introduce backtest engine (`src/backtest.rs`) with deterministic rules.
- Extend config schema for inputs/models/backtest/live risk policy.
- Persist backtest results to SQLite.

### Out of Scope

- Real order execution and account synchronization.
- Multi-symbol portfolio engine in MVP.
- ML training pipeline and web dashboard.

## Architecture

### Core Modules

- `src/signal_input.rs`
  - `SignalInput` trait
  - Config-driven input builders (`rsi`, `sma`, `ema`, `macd`, `bollinger`, `volume_ma`, `close`)
- `src/signal_model.rs`
  - `TradingModel` trait
  - Model builders (`rsi_reversion`, `sma_cross`)
- `src/backtest.rs`
  - Loads candles from storage
  - Computes input series
  - Evaluates model signals
  - Applies fills, fees, slippage, pyramiding, cooldown
  - Computes metrics and saves run/trades

### Data Flow

1. Load config
2. Build storage
3. Load candles in backtest range
4. Build input series
5. Evaluate model per bar (signal at `t`)
6. Fill at `t+1 open`
7. Save run summary + trade logs

## Config Contract

New sections are now available:

- `[[inputs]]`: reusable feature definitions
- `[[models]]`: reusable model definitions
- `[backtest]`: run target, period, sizing
- `[backtest.costs]`: slippage/fee override
- `[backtest.risk]`: max entries and cooldown bars
- `[live.risk]`: unlimited by omitting `max_entries_per_position`

## Storage Contract

### New Tables

- `backtest_runs`
- `backtest_trades`

### New Migration

- `migrations/002_backtest_results.sql`

### Storage Trait Additions

- `get_candles_in_range(...)`
- `save_backtest_results(...)`

## Verification Strategy

- Unit tests for config defaults/validation and backtest utility math.
- Existing indicator/storage tests remain active.
- Recommended command set:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build --release
```

## Rollout Notes

- CLI now supports two modes via subcommand:
  - `live` (existing stream processing)
  - `backtest run` (new model platform flow)
  - `backtest report` (query stored run summaries/trades)
- Backtest output is printed to terminal and persisted in SQLite.
