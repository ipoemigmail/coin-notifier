use chrono::{Duration, Utc};
use error_stack::Report;
use futures::future::BoxFuture;

use crate::error::StorageError;
use crate::storage::Storage;
use crate::strategy::{AlertRule, ConditionType};

/// Result of evaluating an alert rule against an indicator value.
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub triggered: bool,
    pub alert_name: String,
    pub indicator_value: f64,
    pub message: String,
}

/// Evaluate a rule against the current (and optionally previous) indicator value.
pub fn evaluate(
    rule: &AlertRule,
    current_value: f64,
    previous_value: Option<f64>,
) -> EvaluationResult {
    let triggered = is_triggered(&rule.condition, current_value, previous_value);

    let message = if triggered {
        format!(
            "[{}] {} {} — indicator={:.4}",
            rule.name, rule.exchange, rule.symbol, current_value
        )
    } else {
        format!(
            "[{}] not triggered — indicator={:.4}",
            rule.name, current_value
        )
    };

    EvaluationResult {
        triggered,
        alert_name: rule.name.clone(),
        indicator_value: current_value,
        message,
    }
}

fn is_triggered(condition: &ConditionType, current: f64, previous: Option<f64>) -> bool {
    match condition {
        ConditionType::Above(threshold) => current > *threshold,
        ConditionType::Below(threshold) => current < *threshold,
        ConditionType::CrossAbove(threshold) => {
            previous.is_some_and(|prev| prev <= *threshold) && current > *threshold
        }
        ConditionType::CrossBelow(threshold) => {
            previous.is_some_and(|prev| prev >= *threshold) && current < *threshold
        }
        ConditionType::Between { low, high } => current > *low && current < *high,
    }
}

/// Check if the cooldown period for an alert rule has elapsed.
///
/// Returns `true` when an alert should be fired (either never fired before, or
/// cooldown has passed).
pub fn should_alert<'a>(
    storage: &'a dyn Storage,
    rule: &'a AlertRule,
) -> BoxFuture<'a, Result<bool, Report<StorageError>>> {
    Box::pin(async move {
        let last_time = storage.last_alert_time(&rule.name).await?;
        let cooldown = Duration::minutes(rule.cooldown_minutes as i64);
        match last_time {
            Some(t) if Utc::now() - t < cooldown => Ok(false),
            _ => Ok(true),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ExchangeKind;
    use crate::strategy::{ConditionType, IndicatorParams};

    fn make_rule(condition: ConditionType) -> AlertRule {
        AlertRule {
            name: "test-rule".into(),
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
            condition,
            cooldown_minutes: 5,
        }
    }

    #[test]
    fn above_triggers_when_value_exceeds_threshold() {
        let rule = make_rule(ConditionType::Above(70.0));
        let result = evaluate(&rule, 75.0, None);
        assert!(result.triggered);
    }

    #[test]
    fn above_does_not_trigger_at_threshold() {
        let rule = make_rule(ConditionType::Above(70.0));
        let result = evaluate(&rule, 70.0, None);
        assert!(!result.triggered);
    }

    #[test]
    fn below_triggers_when_value_under_threshold() {
        let rule = make_rule(ConditionType::Below(30.0));
        let result = evaluate(&rule, 25.0, None);
        assert!(result.triggered);
    }

    #[test]
    fn below_does_not_trigger_at_threshold() {
        let rule = make_rule(ConditionType::Below(30.0));
        let result = evaluate(&rule, 30.0, None);
        assert!(!result.triggered);
    }

    #[test]
    fn cross_above_triggers_when_crossing_upward() {
        let rule = make_rule(ConditionType::CrossAbove(70.0));
        let result = evaluate(&rule, 71.0, Some(69.0));
        assert!(result.triggered);
    }

    #[test]
    fn cross_above_does_not_trigger_when_already_above() {
        let rule = make_rule(ConditionType::CrossAbove(70.0));
        let result = evaluate(&rule, 75.0, Some(72.0));
        assert!(!result.triggered);
    }

    #[test]
    fn cross_above_does_not_trigger_without_previous() {
        let rule = make_rule(ConditionType::CrossAbove(70.0));
        let result = evaluate(&rule, 75.0, None);
        assert!(!result.triggered);
    }

    #[test]
    fn cross_below_triggers_when_crossing_downward() {
        let rule = make_rule(ConditionType::CrossBelow(30.0));
        let result = evaluate(&rule, 29.0, Some(31.0));
        assert!(result.triggered);
    }

    #[test]
    fn cross_below_does_not_trigger_when_already_below() {
        let rule = make_rule(ConditionType::CrossBelow(30.0));
        let result = evaluate(&rule, 25.0, Some(27.0));
        assert!(!result.triggered);
    }

    #[test]
    fn between_triggers_inside_range() {
        let rule = make_rule(ConditionType::Between {
            low: 40.0,
            high: 60.0,
        });
        let result = evaluate(&rule, 50.0, None);
        assert!(result.triggered);
    }

    #[test]
    fn between_does_not_trigger_at_boundary() {
        let rule = make_rule(ConditionType::Between {
            low: 40.0,
            high: 60.0,
        });
        assert!(!evaluate(&rule, 40.0, None).triggered);
        assert!(!evaluate(&rule, 60.0, None).triggered);
    }

    #[test]
    fn between_does_not_trigger_outside_range() {
        let rule = make_rule(ConditionType::Between {
            low: 40.0,
            high: 60.0,
        });
        assert!(!evaluate(&rule, 30.0, None).triggered);
        assert!(!evaluate(&rule, 70.0, None).triggered);
    }
}
