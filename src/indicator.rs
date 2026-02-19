pub mod bollinger;
pub mod ma;
pub mod macd;
pub mod rsi;
pub mod volume;

use error_stack::Report;

use crate::error::IndicatorError;
use crate::model::Candle;

/// A technical analysis indicator that operates on a slice of candles.
///
/// Candles must be in ascending chronological order (oldest first).
pub trait Indicator: Send {
    /// Unique name of this indicator (e.g., "rsi", "sma").
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Minimum number of candles required to produce at least one output value.
    fn required_candles(&self) -> usize;

    /// Calculate indicator values from candles.
    ///
    /// Returns one value per output point. The number of values may be less
    /// than the number of input candles depending on the indicator's lookback.
    fn calculate(&self, candles: &[Candle]) -> Result<Vec<f64>, Report<IndicatorError>>;
}

/// Extract close prices from a slice of candles.
pub fn close_prices(candles: &[Candle]) -> Vec<f64> {
    candles.iter().map(|c| c.close).collect()
}

/// Extract volumes from a slice of candles.
pub fn volumes(candles: &[Candle]) -> Vec<f64> {
    candles.iter().map(|c| c.volume).collect()
}
