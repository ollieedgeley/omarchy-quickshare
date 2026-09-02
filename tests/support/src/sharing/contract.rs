//! Contract runner shared by fast and environment-backed drivers.

use core::fmt::Debug;

use super::scenario::{ExpectedOutcome, Scenario, ValidationError};

/// A scenario-validation, driver, or public-evidence failure.
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ScenarioError<DriverError> {
    /// The selected driver failed to execute the scenario.
    Driver(DriverError),
    /// The driver's public evidence differs from the contract.
    EvidenceMismatch,
    /// The semantic scenario is invalid.
    InvalidScenario(ValidationError),
}

/// A driver that observes a scenario through a public transfer seam.
pub trait ScenarioDriver {
    /// The driver's environment or protocol error.
    type Error: Debug;

    /// Executes a scenario and returns public terminal evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the peer or environment cannot finish the script.
    fn run(
        &mut self,
        scenario: &Scenario,
    ) -> Result<ExpectedOutcome, Self::Error>;
}

/// Runs one scenario and compares only its public evidence.
///
/// # Errors
///
/// Returns an error for invalid input, driver failure, or mismatched evidence.
#[inline]
pub fn verify_scenario<Driver>(
    driver: &mut Driver,
    scenario: &Scenario,
) -> Result<(), ScenarioError<Driver::Error>>
where
    Driver: ScenarioDriver,
{
    scenario
        .validate()
        .map_err(ScenarioError::InvalidScenario)?;
    let observed = driver.run(scenario).map_err(ScenarioError::Driver)?;
    if observed != scenario.expected {
        return Err(ScenarioError::EvidenceMismatch);
    }
    Ok(())
}
