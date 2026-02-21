use error_stack::{Report, bail};

use crate::error::IndicatorError;
use crate::indicator::ma::Sma;
use crate::indicator::{Indicator, close_prices};
use crate::model::Candle;

pub struct BollingerBands {
    period: usize,
    std_dev_multiplier: f64,
}

impl BollingerBands {
    pub fn new(period: usize, std_dev_multiplier: f64) -> Result<Self, Report<IndicatorError>> {
        if period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "period must be > 0".into(),
            });
        }
        if std_dev_multiplier <= 0.0 {
            bail!(IndicatorError::InvalidParameter {
                name: "std_dev_multiplier must be > 0".into(),
            });
        }
        Ok(Self {
            period,
            std_dev_multiplier,
        })
    }

    /// Returns (upper, middle, lower) band values.
    pub fn calculate_bands(
        &self,
        candles: &[Candle],
    ) -> Result<Vec<(f64, f64, f64)>, Report<IndicatorError>> {
        let prices = close_prices(candles);
        if prices.len() < self.period {
            bail!(IndicatorError::InsufficientData {
                required: self.period,
                available: prices.len(),
            });
        }

        let sma = Sma::new(self.period)?.calculate_prices(&prices)?;

        let bands = prices
            .windows(self.period)
            .zip(sma.iter())
            .map(|(window, &middle)| {
                let variance =
                    window.iter().map(|&p| (p - middle).powi(2)).sum::<f64>() / self.period as f64;
                let std_dev = variance.sqrt();
                let upper = middle + self.std_dev_multiplier * std_dev;
                let lower = middle - self.std_dev_multiplier * std_dev;
                (upper, middle, lower)
            })
            .collect();

        Ok(bands)
    }
}

impl Indicator for BollingerBands {
    fn name(&self) -> &str {
        "bollinger"
    }

    fn required_candles(&self) -> usize {
        self.period
    }

    /// Returns middle band (SMA) values only.
    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        Ok(self
            .calculate_bands(candles)?
            .into_iter()
            .map(|(_, m, _)| m)
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
    fn bollinger_period_zero_invalid() {
        assert!(BollingerBands::new(0, 2.0).is_err());
    }

    #[test]
    fn bollinger_negative_multiplier_invalid() {
        assert!(BollingerBands::new(20, -1.0).is_err());
    }

    #[test]
    fn bollinger_insufficient_data() {
        let bb = BollingerBands::new(5, 2.0).unwrap();
        assert!(bb.calculate(&candles_from_closes(&[1.0; 4])).is_err());
    }

    #[test]
    fn bollinger_flat_prices_zero_width() {
        let bb = BollingerBands::new(3, 2.0).unwrap();
        let candles = candles_from_closes(&[10.0_f64; 5]);
        let bands = bb.calculate_bands(&candles).unwrap();
        for (upper, middle, lower) in &bands {
            assert!((upper - 10.0).abs() < 1e-9);
            assert!((middle - 10.0).abs() < 1e-9);
            assert!((lower - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn bollinger_bands_symmetry() {
        let bb = BollingerBands::new(3, 2.0).unwrap();
        let candles = candles_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let bands = bb.calculate_bands(&candles).unwrap();
        for (upper, middle, lower) in &bands {
            // upper - middle == middle - lower (symmetric around SMA)
            assert!((upper - middle - (middle - lower)).abs() < 1e-9);
        }
    }
}
