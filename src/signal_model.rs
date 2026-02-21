use std::collections::HashMap;

use crate::config::TradingModelConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Buy,
    Sell,
    Hold,
}

pub struct ModelContext<'a> {
    pub feature_values: &'a HashMap<String, f64>,
}

pub trait TradingModel: Send + Sync {
    fn name(&self) -> &str;
    fn required_inputs(&self) -> &[String];
    fn evaluate(&self, ctx: &ModelContext<'_>) -> Result<SignalAction, String>;
}

pub fn build_model(config: &TradingModelConfig) -> Result<Box<dyn TradingModel>, String> {
    match config.kind.as_str() {
        "rsi_reversion" => {
            let input_name = get_string(config, "input", "rsi_14");
            let oversold = get_f64(config, "oversold", 30.0);
            let overbought = get_f64(config, "overbought", 70.0);
            Ok(Box::new(RsiReversionModel::new(
                config.name.clone(),
                input_name,
                oversold,
                overbought,
            )))
        }
        "sma_cross" => {
            let short_input = get_string(config, "short_input", "sma_short");
            let long_input = get_string(config, "long_input", "sma_long");
            Ok(Box::new(SmaCrossModel::new(
                config.name.clone(),
                short_input,
                long_input,
            )))
        }
        other => Err(format!("unknown model kind: {other}")),
    }
}

pub fn build_default_model() -> Box<dyn TradingModel> {
    Box::new(RsiReversionModel::new(
        "rsi_reversion_default".to_string(),
        "rsi_14".to_string(),
        30.0,
        70.0,
    ))
}

fn get_f64(config: &TradingModelConfig, key: &str, default: f64) -> f64 {
    config
        .params
        .get(key)
        .and_then(|v| v.as_float())
        .unwrap_or(default)
}

fn get_string(config: &TradingModelConfig, key: &str, default: &str) -> String {
    config
        .params
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

struct RsiReversionModel {
    name: String,
    input_name: String,
    oversold: f64,
    overbought: f64,
    required_inputs: Vec<String>,
}

impl RsiReversionModel {
    fn new(name: String, input_name: String, oversold: f64, overbought: f64) -> Self {
        Self {
            name,
            required_inputs: vec![input_name.clone()],
            input_name,
            oversold,
            overbought,
        }
    }
}

impl TradingModel for RsiReversionModel {
    fn name(&self) -> &str {
        &self.name
    }

    fn required_inputs(&self) -> &[String] {
        &self.required_inputs
    }

    fn evaluate(&self, ctx: &ModelContext<'_>) -> Result<SignalAction, String> {
        let Some(rsi) = ctx.feature_values.get(&self.input_name).copied() else {
            return Ok(SignalAction::Hold);
        };

        if rsi < self.oversold {
            return Ok(SignalAction::Buy);
        }
        if rsi > self.overbought {
            return Ok(SignalAction::Sell);
        }
        Ok(SignalAction::Hold)
    }
}

struct SmaCrossModel {
    name: String,
    short_input: String,
    long_input: String,
    required_inputs: Vec<String>,
}

impl SmaCrossModel {
    fn new(name: String, short_input: String, long_input: String) -> Self {
        Self {
            name,
            required_inputs: vec![short_input.clone(), long_input.clone()],
            short_input,
            long_input,
        }
    }
}

impl TradingModel for SmaCrossModel {
    fn name(&self) -> &str {
        &self.name
    }

    fn required_inputs(&self) -> &[String] {
        &self.required_inputs
    }

    fn evaluate(&self, ctx: &ModelContext<'_>) -> Result<SignalAction, String> {
        let Some(short) = ctx.feature_values.get(&self.short_input).copied() else {
            return Ok(SignalAction::Hold);
        };
        let Some(long) = ctx.feature_values.get(&self.long_input).copied() else {
            return Ok(SignalAction::Hold);
        };

        if short > long {
            return Ok(SignalAction::Buy);
        }
        if short < long {
            return Ok(SignalAction::Sell);
        }
        Ok(SignalAction::Hold)
    }
}
