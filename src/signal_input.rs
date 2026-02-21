use crate::config::InputConfig;
use crate::indicator::Indicator;
use crate::indicator::bollinger::BollingerBands;
use crate::indicator::ma::{Ema, Sma};
use crate::indicator::macd::Macd;
use crate::indicator::rsi::Rsi;
use crate::indicator::volume::VolumeMA;
use crate::model::Candle;

pub trait SignalInput: Send {
    fn name(&self) -> &str;
    fn required_candles(&self) -> usize;
    fn series(&self, candles: &[Candle]) -> Result<Vec<Option<f64>>, String>;
}

pub fn build_inputs(configs: &[InputConfig]) -> Result<Vec<Box<dyn SignalInput>>, String> {
    let mut inputs = Vec::new();
    for config in configs {
        let input = build_input(config)?;
        inputs.push(input);
    }
    Ok(inputs)
}

pub fn build_default_inputs() -> Result<Vec<Box<dyn SignalInput>>, String> {
    Ok(vec![Box::new(IndicatorInput::new(
        "rsi_14".to_string(),
        Box::new(Rsi::new(14).map_err(|e| format!("default RSI build failed: {e:?}"))?),
    ))])
}

fn build_input(config: &InputConfig) -> Result<Box<dyn SignalInput>, String> {
    match config.kind.as_str() {
        "close" => Ok(Box::new(CloseInput {
            name: config.name.clone(),
        })),
        "rsi" => {
            let period = get_usize(config, "period", 14);
            let indicator = Rsi::new(period).map_err(|e| format!("invalid RSI input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        "sma" => {
            let period = get_usize(config, "period", 20);
            let indicator = Sma::new(period).map_err(|e| format!("invalid SMA input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        "ema" => {
            let period = get_usize(config, "period", 20);
            let indicator = Ema::new(period).map_err(|e| format!("invalid EMA input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        "macd" => {
            let fast = get_usize(config, "fast_period", 12);
            let slow = get_usize(config, "slow_period", 26);
            let signal = get_usize(config, "signal_period", 9);
            let indicator =
                Macd::new(fast, slow, signal).map_err(|e| format!("invalid MACD input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        "bollinger" => {
            let period = get_usize(config, "period", 20);
            let multiplier = get_f64(config, "std_dev_multiplier", 2.0);
            let indicator = BollingerBands::new(period, multiplier)
                .map_err(|e| format!("invalid bollinger input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        "volume_ma" => {
            let period = get_usize(config, "period", 20);
            let indicator =
                VolumeMA::new(period).map_err(|e| format!("invalid volume input: {e:?}"))?;
            Ok(Box::new(IndicatorInput::new(
                config.name.clone(),
                Box::new(indicator),
            )))
        }
        other => Err(format!("unknown input kind: {other}")),
    }
}

fn get_usize(config: &InputConfig, key: &str, default: usize) -> usize {
    config
        .params
        .get(key)
        .and_then(|v| v.as_integer())
        .map(|v| v as usize)
        .unwrap_or(default)
}

fn get_f64(config: &InputConfig, key: &str, default: f64) -> f64 {
    config
        .params
        .get(key)
        .and_then(|v| v.as_float())
        .unwrap_or(default)
}

struct IndicatorInput {
    name: String,
    indicator: Box<dyn Indicator>,
}

impl IndicatorInput {
    fn new(name: String, indicator: Box<dyn Indicator>) -> Self {
        Self { name, indicator }
    }
}

impl SignalInput for IndicatorInput {
    fn name(&self) -> &str {
        &self.name
    }

    fn required_candles(&self) -> usize {
        self.indicator.required_candles()
    }

    fn series(&self, candles: &[Candle]) -> Result<Vec<Option<f64>>, String> {
        let values = self
            .indicator
            .calculate(candles)
            .map_err(|e| format!("input {} failed: {e:?}", self.name))?;
        Ok(align_series(candles.len(), values))
    }
}

struct CloseInput {
    name: String,
}

impl SignalInput for CloseInput {
    fn name(&self) -> &str {
        &self.name
    }

    fn required_candles(&self) -> usize {
        1
    }

    fn series(&self, candles: &[Candle]) -> Result<Vec<Option<f64>>, String> {
        Ok(candles.iter().map(|c| Some(c.close)).collect())
    }
}

fn align_series(total_len: usize, values: Vec<f64>) -> Vec<Option<f64>> {
    let offset = total_len.saturating_sub(values.len());
    let mut output = vec![None; total_len];
    for (index, value) in values.into_iter().enumerate() {
        output[offset + index] = Some(value);
    }
    output
}
