use error_stack::{Report, bail};

use crate::error::IndicatorError;
use crate::indicator::{Indicator, volumes};
use crate::model::Candle;

/// Volume Moving Average — simple average of trading volume over a period.
pub struct VolumeMA {
    period: usize,
}

impl VolumeMA {
    pub fn new(period: usize) -> Result<Self, Report<IndicatorError>> {
        if period == 0 {
            bail!(IndicatorError::InvalidParameter {
                name: "period must be > 0".into(),
            });
        }
        Ok(Self { period })
    }

    /// Returns `true` for each position where the current volume exceeds
    /// `surge_multiplier * volume_ma`.
    #[allow(dead_code)]
    pub fn detect_surges(&self, candles: &[Candle], surge_multiplier: f64) -> Vec<bool> {
        let vols = volumes(candles);
        if vols.len() < self.period {
            return vec![];
        }
        vols.windows(self.period)
            .enumerate()
            .map(|(i, window)| {
                let ma = window.iter().sum::<f64>() / self.period as f64;
                let current_vol = vols[i + self.period - 1];
                current_vol > ma * surge_multiplier
            })
            .collect()
    }
}

impl Indicator for VolumeMA {
    fn name(&self) -> &str {
        "volume_ma"
    }

    fn required_candles(&self) -> usize {
        self.period
    }

    /// Returns volume MA values.
    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>> {
        let vols = volumes(candles);
        if vols.len() < self.period {
            bail!(IndicatorError::InsufficientData {
                required: self.period,
                available: vols.len(),
            });
        }
        Ok(vols
            .windows(self.period)
            .map(|w| w.iter().sum::<f64>() / self.period as f64)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExchangeKind, TimeFrame};
    use chrono::Utc;

    fn candles_with_volumes(vols: &[f64]) -> Vec<Candle> {
        vols.iter()
            .enumerate()
            .map(|(i, &v)| Candle {
                exchange: ExchangeKind::Upbit,
                symbol: "TEST".into(),
                timeframe: TimeFrame::Min1,
                open_time: Utc::now() + chrono::Duration::minutes(i as i64),
                open: 100.0,
                high: 100.0,
                low: 100.0,
                close: 100.0,
                volume: v,
            })
            .collect()
    }

    #[test]
    fn volume_ma_period_zero_invalid() {
        assert!(VolumeMA::new(0).is_err());
    }

    #[test]
    fn volume_ma_insufficient_data() {
        let vma = VolumeMA::new(5).unwrap();
        assert!(vma.calculate(&candles_with_volumes(&[1.0; 4])).is_err());
    }

    #[test]
    fn volume_ma_known_value() {
        let vma = VolumeMA::new(3).unwrap();
        let candles = candles_with_volumes(&[1.0, 2.0, 3.0, 4.0]);
        let values = vma.calculate(&candles).unwrap();
        // (1+2+3)/3 = 2.0, (2+3+4)/3 = 3.0
        assert!((values[0] - 2.0).abs() < 1e-9);
        assert!((values[1] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn surge_detection() {
        let vma = VolumeMA::new(3).unwrap();
        // avg of first 3 = (1+1+1)/3 = 1.0; 4th volume = 5.0 -> surge at 2x
        let candles = candles_with_volumes(&[1.0, 1.0, 1.0, 5.0]);
        let surges = vma.detect_surges(&candles, 2.0);
        assert_eq!(surges.len(), 2);
        assert!(!surges[0]); // window [1,1,1]: current=1, ma=1 -> not surge
        assert!(surges[1]); // window [1,1,5]: current=5, ma=1 -> surge
    }
}
