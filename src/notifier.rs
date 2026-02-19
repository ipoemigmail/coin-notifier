pub mod terminal;

use crate::model::ExchangeKind;
use crate::strategy::condition::EvaluationResult;

/// Sink for alert notifications.
pub trait Notifier: Send + Sync {
    fn notify(&self, exchange: ExchangeKind, symbol: &str, price: f64, result: &EvaluationResult);
}
