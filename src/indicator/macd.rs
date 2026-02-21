use error_stack::{Report, bail};

use crate::error::IndicatorError;
use crate::indicator::ma::Ema;
use crate::indicator::{Indicator, close_prices};
use crate::model::Candle;

pub struct Macd {
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
}

impl Macd {
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
    ) -> Result<Self, Report<IndicatorError>> {
        if fast_period == 0 || slow_period == 0 || signal_period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "all periods must be > 0".into(),
            });
        }
        if fast_period >= slow_period {
            bail!(IndicatorError::InvalidParameter {
                name: "fast_period must be < slow_period".into(),
            });
        }
        Ok(Self {
            fast_period,
            slow_period,
            signal_period,
        })
    }

    /// Calculate (macd_line, signal_line, histogram) tuples.
    pub fn calculate_full(
        &self,
        candles: &[Candle],
    ) -> Result<Vec<(f64, f64, f64)>, Report<IndicatorError>> {
        let prices = close_prices(candles);
        if prices.len() < self.required_candles() {
            bail!(IndicatorError::InsufficientData {
                required: self.required_candles(),
                available: prices.len(),
            });
        }

        let fast_ema = Ema::new(self.fast_period)?.calculate_prices(&prices)?;
        let slow_ema = Ema::new(self.slow_period)?.calculate_prices(&prices)?;

        // Align: slow_ema is shorter by (slow_period - fast_period) elements
        let offset = self.slow_period - self.fast_period;
        let macd_line: Vec<f64> = fast_ema[offset..]
            .iter()
            .zip(slow_ema.iter())
            .map(|(f, s)| f - s)
            .collect();

        let signal_line = Ema::new(self.signal_period)?.calculate_prices(&macd_line)?;
        // Signal is shorter by (signal_period - 1)
        let signal_offset = self.signal_period - 1;
        let histogram: Vec<f64> = macd_line[signal_offset..]
            .iter()
            .zip(signal_line.iter())
            .map(|(m, s)| m - s)
            .collect();

        let result: Vec<(f64, f64, f64)> = macd_line[signal_offset..]
            .iter()
            .zip(signal_line.iter())
            .zip(histogram.iter())
            .map(|((m, s), h)| (*m, *s, *h))
            .collect();

        Ok(result)
    }
}

impl Indicator for Macd {
    fn name(&self) -> &str {
        "macd"
    }

    fn required_candles(&self) -> usize {
        self.slow_period + self.signal_period
    }

    /// Returns MACD line values only.
    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        Ok(self
            .calculate_full(candles)?
            .into_iter()
            .map(|(m, _, _)| m)
            .collect())
    }
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
    fn macd_invalid_fast_ge_slow() {
        assert!(Macd::new(26, 12, 9).is_err());
    }

    #[test]
    fn macd_period_zero_invalid() {
        assert!(Macd::new(0, 26, 9).is_err());
    }

    #[test]
    fn macd_insufficient_data() {
        let macd = Macd::new(12, 26, 9).unwrap();
        assert!(macd.calculate(&candles_from_closes(&[1.0; 30])).is_err());
    }

    #[test]
    fn macd_flat_prices_returns_zero() {
        let macd = Macd::new(3, 5, 3).unwrap();
        // Need 5 + 3 = 8 candles minimum
        let candles = candles_from_closes(&[10.0_f64; 10]);
        let values = macd.calculate(&candles).unwrap();
        for v in &values {
            assert!(v.abs() < 1e-9, "expected 0 for flat prices, got {v}");
        }
    }

    #[test]
    fn macd_output_non_empty() {
        let macd = Macd::new(3, 5, 3).unwrap();
        let closes: Vec<f64> = (1..=12).map(|i| i as f64).collect();
        let candles = candles_from_closes(&closes);
        let values = macd.calculate(&candles).unwrap();
        assert!(!values.is_empty());
    }
}
