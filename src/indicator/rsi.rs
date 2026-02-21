use error_stack::{Report, bail};

use crate::error::IndicatorError;
use crate::indicator::{Indicator, close_prices};
use crate::model::Candle;

/// RSI (Relative Strength Index) using Wilder's smoothing method.
pub struct Rsi {
    period: usize,
}

impl Rsi {
    pub fn new(period: usize) -> Result<Self, Report<IndicatorError>> {
        if period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "period must be > 0".into(),
            });
        }
        Ok(Self { period })
    }
}

impl Indicator for Rsi {
    fn name(&self) -> &str {
        "rsi"
    }

    fn required_candles(&self) -> usize {
        self.period + 1
    }

    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        let prices = close_prices(candles);
        if prices.len() < self.required_candles() {
            bail!(IndicatorError::InsufficientData {
                required: self.required_candles(),
                available: prices.len(),
            });
        }

        let deltas: Vec<f64> = prices.windows(2).map(|w| w[1] - w[0]).collect();

        // Seed using simple average of first `period` gains/losses
        let mut avg_gain: f64 = deltas[..self.period]
            .iter()
            .map(|&d| d.max(0.0))
            .sum::<f64>()
            / self.period as f64;
        let mut avg_loss: f64 = deltas[..self.period]
            .iter()
            .map(|&d| (-d).max(0.0))
            .sum::<f64>()
            / self.period as f64;

        let first_rsi = rsi_value(avg_gain, avg_loss);
        let mut results = vec![first_rsi];

        // Wilder smoothing for subsequent values
        for &delta in &deltas[self.period..] {
            let gain = delta.max(0.0);
            let loss = (-delta).max(0.0);
            avg_gain = (avg_gain * (self.period - 1) as f64 + gain) / self.period as f64;
            avg_loss = (avg_loss * (self.period - 1) as f64 + loss) / self.period as f64;
            results.push(rsi_value(avg_gain, avg_loss));
        }

        Ok(results)
    }
}

fn rsi_value(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return 100.0;
    }
    let rs = avg_gain / avg_loss;
    100.0 - 100.0 / (1.0 + rs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExchangeKind, TimeFrame};
    use chrono::Utc;

    fn candles_from_closes(closes: &[f64]) -> Vec<Candle> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Candle {
                exchange: ExchangeKind::Upbit,
                symbol: "TEST".into(),
                timeframe: TimeFrame::Min1,
                open_time: Utc::now() + chrono::Duration::minutes(i as i64),
                open: c,
                high: c,
                low: c,
                close: c,
                volume: 1.0,
            })
            .collect()
    }

    #[test]
    fn rsi_insufficient_data() {
        let rsi = Rsi::new(14).unwrap();
        let candles = candles_from_closes(&[1.0; 10]);
        assert!(rsi.calculate(&candles).is_err());
    }

    #[test]
    fn rsi_period_zero_invalid() {
        assert!(Rsi::new(0).is_err());
    }

    #[test]
    fn rsi_all_gains_returns_100() {
        let rsi = Rsi::new(3).unwrap();
        // 4 candles needed (period + 1), all rising
        let candles = candles_from_closes(&[1.0, 2.0, 3.0, 4.0]);
        let values = rsi.calculate(&candles).unwrap();
        assert!(!values.is_empty());
        assert_eq!(values[0], 100.0);
    }

    #[test]
    fn rsi_all_losses_returns_0() {
        let rsi = Rsi::new(3).unwrap();
        let candles = candles_from_closes(&[4.0, 3.0, 2.0, 1.0]);
        let values = rsi.calculate(&candles).unwrap();
        assert!(!values.is_empty());
        // avg_gain = 0, so RSI should be 0
        assert!((values[0] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn rsi_known_value() {
        // Simple sanity: RSI with 3-period for a flat-then-spike sequence
        let rsi = Rsi::new(3).unwrap();
        // prices: 10, 10, 10, 11 -> only last delta is a gain
        let candles = candles_from_closes(&[10.0, 10.0, 10.0, 11.0]);
        let values = rsi.calculate(&candles).unwrap();
        assert!(!values.is_empty());
        // All deltas 0,0,+1 -> avg_gain=1/3, avg_loss=0 -> RSI=100
        assert_eq!(values[0], 100.0);
    }

    #[test]
    fn rsi_output_length() {
        let rsi = Rsi::new(14).unwrap();
        let candles = candles_from_closes(&[100.0_f64; 20]);
        let values = rsi.calculate(&candles).unwrap();
        // 20 prices -> 19 deltas -> 1 seed + 5 subsequent = 6 values
        assert_eq!(values.len(), 20 - 14);
    }
}
