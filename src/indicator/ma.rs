use error_stack::{Report, bail};

use crate::error::IndicatorError;
use crate::indicator::{Indicator, close_prices};
use crate::model::Candle;

/// Simple Moving Average.
pub struct Sma {
    period: usize,
}

impl Sma {
    pub fn new(period: usize) -> Result<Self, Report<IndicatorError>> {
        if period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "period must be > 0".into(),
            });
        }
        Ok(Self { period })
    }

    /// Calculate SMA values from a price slice (internal helper).
    pub fn calculate_prices(&self, prices: &[f64]) -> Result<Vec<f64>, Report<IndicatorError>> {
        if prices.len() < self.period {
            bail!(IndicatorError::InsufficientData {
                required: self.period,
                available: prices.len(),
            });
        }
        Ok(prices
            .windows(self.period)
            .map(|w| w.iter().sum::<f64>() / self.period as f64)
            .collect())
    }
}

impl Indicator for Sma {
    fn name(&self) -> &str {
        "sma"
    }

    fn required_candles(&self) -> usize {
        self.period
    }

    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        self.calculate_prices(&close_prices(candles))
    }
}

/// Exponential Moving Average.
pub struct Ema {
    period: usize,
}

impl Ema {
    pub fn new(period: usize) -> Result<Self, Report<IndicatorError>> {
        if period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "period must be > 0".into(),
            });
        }
        Ok(Self { period })
    }

    /// Calculate EMA values from a price slice (internal helper).
    pub fn calculate_prices(&self, prices: &[f64]) -> Result<Vec<f64>, Report<IndicatorError>> {
        if prices.len() < self.period {
            bail!(IndicatorError::InsufficientData {
                required: self.period,
                available: prices.len(),
            });
        }

        let k = 2.0 / (self.period as f64 + 1.0);
        // Seed with SMA of first `period` values
        let seed: f64 = prices[..self.period].iter().sum::<f64>() / self.period as f64;
        let mut ema = seed;
        let mut results = vec![ema];

        for &price in &prices[self.period..] {
            ema = price * k + ema * (1.0 - k);
            results.push(ema);
        }

        Ok(results)
    }
}

impl Indicator for Ema {
    fn name(&self) -> &str {
        "ema"
    }

    fn required_candles(&self) -> usize {
        self.period
    }

    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        self.calculate_prices(&close_prices(candles))
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
    fn sma_period_zero_invalid() {
        assert!(Sma::new(0).is_err());
    }

    #[test]
    fn sma_insufficient_data() {
        let sma = Sma::new(5).unwrap();
        assert!(sma.calculate(&candles_from_closes(&[1.0; 4])).is_err());
    }

    #[test]
    fn sma_flat_prices() {
        let sma = Sma::new(3).unwrap();
        let candles = candles_from_closes(&[10.0; 5]);
        let values = sma.calculate(&candles).unwrap();
        assert_eq!(values.len(), 3);
        for v in &values {
            assert!((v - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn sma_known_value() {
        let sma = Sma::new(3).unwrap();
        let candles = candles_from_closes(&[1.0, 2.0, 3.0, 4.0]);
        let values = sma.calculate(&candles).unwrap();
        // (1+2+3)/3 = 2.0, (2+3+4)/3 = 3.0
        assert!((values[0] - 2.0).abs() < 1e-9);
        assert!((values[1] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn ema_period_zero_invalid() {
        assert!(Ema::new(0).is_err());
    }

    #[test]
    fn ema_insufficient_data() {
        let ema = Ema::new(5).unwrap();
        assert!(ema.calculate(&candles_from_closes(&[1.0; 4])).is_err());
    }

    #[test]
    fn ema_flat_prices() {
        let ema = Ema::new(3).unwrap();
        let candles = candles_from_closes(&[10.0; 6]);
        let values = ema.calculate(&candles).unwrap();
        for v in &values {
            assert!((v - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn ema_seed_equals_sma() {
        // Seed (first EMA value) should equal SMA of first `period` prices
        let ema = Ema::new(3).unwrap();
        let candles = candles_from_closes(&[1.0, 2.0, 3.0, 4.0]);
        let values = ema.calculate(&candles).unwrap();
        // seed = (1+2+3)/3 = 2.0
        assert!((values[0] - 2.0).abs() < 1e-9);
    }
}
