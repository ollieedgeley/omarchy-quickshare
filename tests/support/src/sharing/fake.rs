//! Deterministic fast fake for semantic transfer scenarios.

use core::convert::Infallible;

use super::contract::ScenarioDriver;
use super::scenario::{ExpectedOutcome, Scenario};

/// A deterministic peer fake used for millisecond-fast feedback.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
#[expect(
    clippy::module_name_repetitions,
    reason = "Test-support policy requires the Fake role in the type name"
)]
pub struct ScriptedPeerFake;

impl ScenarioDriver for ScriptedPeerFake {
    type Error = Infallible;

    #[inline]
    fn run(
        &mut self,
        scenario: &Scenario,
    ) -> Result<ExpectedOutcome, Self::Error> {
        Ok(scenario.scripted_outcome())
    }
}
