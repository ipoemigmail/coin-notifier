use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::config::{AppConfig, BacktestConfig};
use crate::model::{BacktestRun, BacktestTrade, Candle, ExchangeKind, TimeFrame};
use crate::signal_input::{SignalInput, build_default_inputs, build_inputs};
use crate::signal_model::{
    ModelContext, SignalAction, TradingModel, build_default_model, build_model,
};
use crate::storage::Storage;

pub struct BacktestOutput {
    pub run: BacktestRun,
    pub trades: Vec<BacktestTrade>,
}

pub async fn run(config: &AppConfig, storage: &dyn Storage) -> Result<BacktestOutput, String> {
    let settings = config
        .backtest
        .as_ref()
        .ok_or_else(|| "[backtest] section is required".to_string())?;

    let exchange = parse_exchange(&settings.exchange)?;
    let timeframe = TimeFrame::from_str(&settings.timeframe)
        .ok_or_else(|| format!("unknown timeframe: {}", settings.timeframe))?;

    let candles = storage
        .get_candles_in_range(
            exchange,
            &settings.symbol,
            timeframe,
            settings.start_time,
            settings.end_time,
        )
        .await
        .map_err(|e| format!("failed to load candles: {e:?}"))?;

    if candles.len() < 2 {
        return Err("backtest requires at least 2 candles".to_string());
    }

    let inputs = if config.inputs.is_empty() {
        build_default_inputs()?
    } else {
        build_inputs(&config.inputs)?
    };

    let max_required = inputs
        .iter()
        .map(|input| input.required_candles())
        .max()
        .unwrap_or(1);
    if candles.len() < max_required {
        return Err(format!(
            "insufficient candles: need at least {max_required}, got {}",
            candles.len()
        ));
    }

    let model = find_model(config, settings)?;
    let input_series = build_input_series(&inputs, &candles)?;

    let mut engine = BacktestEngine::new(settings, exchange, timeframe, model.name().to_string());
    engine.execute(&candles, &input_series, model.as_ref())?;
    let run = engine.build_run(&settings.symbol);
    let trades = engine.trades.clone();

    storage
        .save_backtest_results(run.clone(), trades.clone())
        .await
        .map_err(|e| format!("failed to save backtest results: {e:?}"))?;

    Ok(BacktestOutput { run, trades })
}

fn find_model(
    config: &AppConfig,
    settings: &BacktestConfig,
) -> Result<Box<dyn TradingModel>, String> {
    if config.models.is_empty() {
        return Ok(build_default_model());
    }

    let model_config = config
        .models
        .iter()
        .find(|m| m.name == settings.model)
        .ok_or_else(|| format!("model not found: {}", settings.model))?;
    build_model(model_config)
}

fn build_input_series(
    inputs: &[Box<dyn SignalInput>],
    candles: &[Candle],
) -> Result<HashMap<String, Vec<Option<f64>>>, String> {
    let mut map = HashMap::new();
    for input in inputs {
        let values = input.series(candles)?;
        map.insert(input.name().to_string(), values);
    }
    Ok(map)
}

fn parse_exchange(name: &str) -> Result<ExchangeKind, String> {
    match name {
        "upbit" => Ok(ExchangeKind::Upbit),
        "binance" => Ok(ExchangeKind::Binance),
        _ => Err(format!("unknown exchange: {name}")),
    }
}

struct BacktestEngine<'a> {
    settings: &'a BacktestConfig,
    exchange: ExchangeKind,
    timeframe: TimeFrame,
    model_name: String,
    cash: f64,
    last_entry_fill_index: Option<usize>,
    open_lots: Vec<OpenLot>,
    trades: Vec<BacktestTrade>,
    equity_curve: Vec<f64>,
}

#[derive(Clone)]
struct OpenLot {
    entry_time: DateTime<Utc>,
    entry_price: f64,
    quantity: f64,
    fee_paid: f64,
}

impl<'a> BacktestEngine<'a> {
    fn new(
        settings: &'a BacktestConfig,
        exchange: ExchangeKind,
        timeframe: TimeFrame,
        model_name: String,
    ) -> Self {
        Self {
            settings,
            exchange,
            timeframe,
            model_name,
            cash: settings.initial_capital,
            last_entry_fill_index: None,
            open_lots: Vec::new(),
            trades: Vec::new(),
            equity_curve: Vec::new(),
        }
    }

    fn execute(
        &mut self,
        candles: &[Candle],
        input_series: &HashMap<String, Vec<Option<f64>>>,
        model: &dyn TradingModel,
    ) -> Result<(), String> {
        for index in 0..candles.len().saturating_sub(1) {
            let Some(feature_values) = collect_feature_values(model, input_series, index) else {
                self.record_equity(candles[index].close);
                continue;
            };

            let action = model.evaluate(&ModelContext {
                feature_values: &feature_values,
            })?;

            match action {
                SignalAction::Buy => self.try_buy(index, candles)?,
                SignalAction::Sell => self.try_sell(index, candles, "model_sell"),
                SignalAction::Hold => {}
            }

            self.record_equity(candles[index].close);
        }

        self.force_close(candles)?;
        Ok(())
    }

    fn try_buy(&mut self, index: usize, candles: &[Candle]) -> Result<(), String> {
        if index + 1 >= candles.len() {
            return Ok(());
        }

        if self.hit_max_entries() {
            return Ok(());
        }

        if !self.cooldown_elapsed(index + 1) {
            return Ok(());
        }

        let next_open = candles[index + 1].open;
        let fill_price = apply_slippage(next_open, self.settings.costs.slippage_bps, true);
        let fee_rate = resolve_fee_bps(self.exchange, &self.settings.costs.fee_bps_overrides);
        let fee_multiplier = 1.0 + fee_rate / 10_000.0;

        let equity = self.current_equity(candles[index].close);
        let target_notional = equity * self.settings.entry_size_percent / 100.0;
        let affordable_notional = self.cash / fee_multiplier;
        let notional = target_notional.min(affordable_notional);

        if notional <= 0.0 || fill_price <= 0.0 {
            return Ok(());
        }

        let fee_paid = notional * fee_rate / 10_000.0;
        let quantity = notional / fill_price;
        self.cash -= notional + fee_paid;

        self.open_lots.push(OpenLot {
            entry_time: candles[index + 1].open_time,
            entry_price: fill_price,
            quantity,
            fee_paid,
        });
        self.last_entry_fill_index = Some(index + 1);

        Ok(())
    }

    fn try_sell(&mut self, index: usize, candles: &[Candle], reason: &str) {
        if self.open_lots.is_empty() || index + 1 >= candles.len() {
            return;
        }

        let next_open = candles[index + 1].open;
        let fill_price = apply_slippage(next_open, self.settings.costs.slippage_bps, false);
        self.close_all_lots(fill_price, candles[index + 1].open_time, reason);
    }

    fn force_close(&mut self, candles: &[Candle]) -> Result<(), String> {
        if self.open_lots.is_empty() {
            self.equity_curve.push(self.cash);
            return Ok(());
        }

        let last = candles
            .last()
            .ok_or_else(|| "candles cannot be empty".to_string())?;
        let fill_price = apply_slippage(last.close, self.settings.costs.slippage_bps, false);
        self.close_all_lots(fill_price, last.open_time, "forced_exit");
        self.equity_curve.push(self.cash);

        Ok(())
    }

    fn close_all_lots(&mut self, fill_price: f64, exit_time: DateTime<Utc>, reason: &str) {
        let fee_rate = resolve_fee_bps(self.exchange, &self.settings.costs.fee_bps_overrides);
        for lot in self.open_lots.drain(..) {
            let exit_notional = fill_price * lot.quantity;
            let exit_fee = exit_notional * fee_rate / 10_000.0;
            let gross_pnl = (fill_price - lot.entry_price) * lot.quantity;
            let net_pnl = gross_pnl - lot.fee_paid - exit_fee;

            self.cash += exit_notional - exit_fee;
            self.trades.push(BacktestTrade {
                run_id: String::new(),
                exchange: self.exchange,
                symbol: self.settings.symbol.clone(),
                entry_time: lot.entry_time,
                exit_time,
                entry_price: lot.entry_price,
                exit_price: fill_price,
                quantity: lot.quantity,
                gross_pnl,
                net_pnl,
                fee_paid: lot.fee_paid + exit_fee,
                reason: reason.to_string(),
            });
        }
    }

    fn hit_max_entries(&self) -> bool {
        let max_entries = self.settings.risk.max_entries_per_position;
        max_entries > 0 && self.open_lots.len() >= max_entries
    }

    fn cooldown_elapsed(&self, fill_index: usize) -> bool {
        let Some(last_fill_index) = self.last_entry_fill_index else {
            return true;
        };
        let cooldown = self.settings.risk.cooldown_bars;
        fill_index > last_fill_index + cooldown
    }

    fn record_equity(&mut self, mark_price: f64) {
        self.equity_curve.push(self.current_equity(mark_price));
    }

    fn current_equity(&self, mark_price: f64) -> f64 {
        let position_value: f64 = self
            .open_lots
            .iter()
            .map(|lot| lot.quantity * mark_price)
            .sum();
        self.cash + position_value
    }

    fn build_run(&mut self, symbol: &str) -> BacktestRun {
        let run_id = Uuid::new_v4().to_string();
        let wins = self.trades.iter().filter(|t| t.net_pnl > 0.0).count();
        let win_rate_pct = if self.trades.is_empty() {
            0.0
        } else {
            wins as f64 / self.trades.len() as f64 * 100.0
        };

        for trade in &mut self.trades {
            trade.run_id = run_id.clone();
        }

        let final_equity = self.cash;
        let total_return_pct = (final_equity / self.settings.initial_capital - 1.0) * 100.0;
        let max_drawdown_pct = calculate_max_drawdown_pct(&self.equity_curve);

        BacktestRun {
            run_id,
            model_name: self.model_name.clone(),
            exchange: self.exchange,
            symbol: symbol.to_string(),
            timeframe: self.timeframe,
            start_time: self.settings.start_time,
            end_time: self.settings.end_time,
            initial_capital: self.settings.initial_capital,
            final_equity,
            total_return_pct,
            max_drawdown_pct,
            win_rate_pct,
            trade_count: self.trades.len(),
            created_at: Utc::now(),
        }
    }
}

fn collect_feature_values(
    model: &dyn TradingModel,
    input_series: &HashMap<String, Vec<Option<f64>>>,
    index: usize,
) -> Option<HashMap<String, f64>> {
    let mut values = HashMap::new();
    for input_name in model.required_inputs() {
        let value = input_series
            .get(input_name)
            .and_then(|series| series.get(index))
            .and_then(|value| *value)?;
        values.insert(input_name.clone(), value);
    }
    Some(values)
}

fn apply_slippage(price: f64, slippage_bps: f64, is_buy: bool) -> f64 {
    let ratio = slippage_bps / 10_000.0;
    if is_buy {
        return price * (1.0 + ratio);
    }
    price * (1.0 - ratio)
}

fn resolve_fee_bps(exchange: ExchangeKind, overrides: &HashMap<String, f64>) -> f64 {
    let key = exchange.to_string();
    if let Some(value) = overrides.get(&key) {
        return *value;
    }
    default_fee_bps(exchange)
}

fn default_fee_bps(exchange: ExchangeKind) -> f64 {
    match exchange {
        ExchangeKind::Upbit => 5.0,
        ExchangeKind::Binance => 10.0,
    }
}

fn calculate_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    if equity_curve.is_empty() {
        return 0.0;
    }

    let mut peak = equity_curve[0];
    let mut max_drawdown = 0.0;
    for equity in equity_curve {
        if *equity > peak {
            peak = *equity;
        }
        if peak <= 0.0 {
            continue;
        }
        let drawdown = (peak - *equity) / peak;
        if drawdown > max_drawdown {
            max_drawdown = drawdown;
        }
    }
    max_drawdown * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slippage_applies_in_expected_direction() {
        assert_eq!(apply_slippage(100.0, 10.0, true), 100.1);
        assert_eq!(apply_slippage(100.0, 10.0, false), 99.9);
    }

    #[test]
    fn max_drawdown_detects_peak_to_trough() {
        let curve = vec![100.0, 120.0, 90.0, 110.0];
        let drawdown = calculate_max_drawdown_pct(&curve);
        assert!((drawdown - 25.0).abs() < 1e-9);
    }
}
