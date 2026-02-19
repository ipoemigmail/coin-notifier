use crate::model::ExchangeKind;
use crate::notifier::Notifier;
use crate::strategy::condition::EvaluationResult;

pub struct TerminalNotifier;

impl Notifier for TerminalNotifier {
    fn notify(&self, exchange: ExchangeKind, symbol: &str, price: f64, result: &EvaluationResult) {
        tracing::warn!(
            exchange = %exchange,
            symbol = symbol,
            alert = %result.alert_name,
            indicator_value = result.indicator_value,
            price = price,
            "ALERT: {}",
            result.message,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::condition::evaluate;
    use crate::strategy::{AlertRule, ConditionType, IndicatorParams};

    #[test]
    fn terminal_notifier_does_not_panic() {
        let notifier = TerminalNotifier;
        let rule = AlertRule {
            name: "test".into(),
            exchange: ExchangeKind::Upbit,
            symbol: "KRW-BTC".into(),
            indicator_name: "rsi".into(),
            indicator_params: IndicatorParams {
                period: Some(14),
                fast_period: None,
                slow_period: None,
                signal_period: None,
                std_dev_multiplier: None,
                surge_multiplier: None,
            },
            condition: ConditionType::Below(30.0),
            cooldown_minutes: 5,
        };
        let result = evaluate(&rule, 28.5, None);
        // Should not panic
        notifier.notify(ExchangeKind::Upbit, "KRW-BTC", 120_500_000.0, &result);
    }
}
