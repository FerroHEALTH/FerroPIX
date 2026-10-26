// SPDX-FileCopyrightText: Cadasto B.V.
// SPDX-License-Identifier: BUSL-1.1

//! Readiness over a registry of indicators.
//!
//! An indicator is a named, bounded check of one subsystem FerroPIX needs to
//! serve: the database, the audit spool's headroom, the MLLP listener's
//! socket, the feed sender's backlog (`docs/architecture.md` §9). Liveness
//! answers `200` while the process is up; readiness runs every registered
//! indicator and answers `503` while any is down, with the
//! `ferropix_operator_api::health::Readiness` document naming each one. No
//! specification governs health probes: our own design, after FerroBRIDGE's
//! registry.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use ferropix_operator_api::health::{IndicatorState, Readiness, State};

/// The upper bound on one indicator check: a check that runs past it is
/// reported down, so a wedged subsystem cannot hang the probe.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// The future one check returns, boxed so the registry can hold indicators
/// behind `dyn` (<https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility>).
pub type Check<'a> = Pin<Box<dyn Future<Output = IndicatorState> + Send + 'a>>;

/// One named, bounded check of one subsystem.
pub trait HealthIndicator: fmt::Debug + Send + Sync {
    /// Returns the name this indicator answers under in the readiness
    /// document.
    fn name(&self) -> &'static str;

    /// Runs the check, which the registry bounds to [`CHECK_TIMEOUT`].
    fn check(&self) -> Check<'_>;
}

/// The indicators readiness evaluates.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    /// The indicators, in registration order.
    indicators: Arc<[Arc<dyn HealthIndicator>]>,
}

impl Registry {
    /// Returns a registry over `indicators`.
    #[must_use]
    pub fn new(indicators: Vec<Arc<dyn HealthIndicator>>) -> Self {
        Self {
            indicators: indicators.into(),
        }
    }

    /// Returns this registry with `indicator` registered after the others.
    #[must_use]
    pub fn and(self, indicator: Arc<dyn HealthIndicator>) -> Self {
        let mut indicators = self.indicators.to_vec();
        indicators.push(indicator);
        Self::new(indicators)
    }

    /// Returns the names of the registered indicators, in registration order.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.indicators
            .iter()
            .map(|indicator| indicator.name())
            .collect()
    }

    /// Runs every indicator, one after another and each bounded to
    /// [`CHECK_TIMEOUT`], and folds the results into one report.
    ///
    /// The probe asks on the request: a cache filled by a background loop
    /// would call a dead subsystem ready until its next tick.
    pub async fn evaluate(&self) -> Readiness {
        let mut indicators = BTreeMap::new();
        let mut state = State::Up;
        for indicator in self.indicators.iter() {
            let outcome = tokio::time::timeout(CHECK_TIMEOUT, indicator.check())
                .await
                .unwrap_or_else(|_elapsed| {
                    IndicatorState::down(format!(
                        "the check did not answer within {} ms",
                        CHECK_TIMEOUT.as_millis()
                    ))
                });
            if outcome.state == State::Down {
                state = State::Down;
            }
            indicators.insert(indicator.name().to_owned(), outcome);
        }
        Readiness { state, indicators }
    }
}

/// Returns the status `report` answers with: `200` while the aggregate is
/// up, `503` while it is down.
#[must_use]
pub const fn status(report: &Readiness) -> http::StatusCode {
    match report.state {
        State::Up => http::StatusCode::OK,
        State::Down => http::StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use ferropix_operator_api::health::{IndicatorState, State};
    use http::StatusCode;

    use super::{CHECK_TIMEOUT, Check, HealthIndicator, Registry, status};

    #[derive(Debug)]
    struct Fixed(&'static str, State);

    impl HealthIndicator for Fixed {
        fn name(&self) -> &'static str {
            self.0
        }

        fn check(&self) -> Check<'_> {
            let state = self.1;
            Box::pin(async move {
                match state {
                    State::Up => IndicatorState::up(),
                    State::Down => IndicatorState::down("the stub is down"),
                }
            })
        }
    }

    #[derive(Debug)]
    struct Wedged;

    impl HealthIndicator for Wedged {
        fn name(&self) -> &'static str {
            "wedged"
        }

        fn check(&self) -> Check<'_> {
            Box::pin(async {
                tokio::time::sleep(CHECK_TIMEOUT + Duration::from_secs(60)).await;
                IndicatorState::up()
            })
        }
    }

    #[tokio::test]
    async fn an_empty_registry_is_up() {
        let report = Registry::default().evaluate().await;
        assert_eq!(State::Up, report.state);
        assert_eq!(StatusCode::OK, status(&report));
        assert!(report.indicators.is_empty());
    }

    #[tokio::test]
    async fn one_down_indicator_takes_the_aggregate_down_and_every_state_is_named() {
        let registry = Registry::default()
            .and(Arc::new(Fixed("database", State::Down)))
            .and(Arc::new(Fixed("audit_spool", State::Up)));
        let report = registry.evaluate().await;
        assert_eq!(State::Down, report.state);
        assert_eq!(StatusCode::SERVICE_UNAVAILABLE, status(&report));
        assert_eq!(
            Some(State::Down),
            report.indicators.get("database").map(|state| state.state)
        );
        assert_eq!(
            Some(State::Up),
            report
                .indicators
                .get("audit_spool")
                .map(|state| state.state)
        );
        assert_eq!(vec!["database", "audit_spool"], registry.names());
    }

    #[tokio::test(start_paused = true)]
    async fn a_check_past_its_bound_is_down() {
        let report = Registry::new(vec![Arc::new(Wedged)]).evaluate().await;
        assert_eq!(State::Down, report.state);
        assert!(
            report
                .indicators
                .get("wedged")
                .and_then(|state| state.detail.as_deref())
                .is_some_and(|detail| detail.contains("did not answer"))
        );
    }
}
