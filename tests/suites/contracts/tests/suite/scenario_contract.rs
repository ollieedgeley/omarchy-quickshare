#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use quickshare_test_support::sharing::contract::verify_scenario;
    use quickshare_test_support::sharing::fake::ScriptedPeerFake;
    use quickshare_test_support::sharing::scenario::{
        Scenario, ValidationError,
    };

    #[test]
    fn cancellations_run_through_the_fast_fake() {
        verify_fixture("cancellations.json");
    }

    #[test]
    fn consent_decisions_run_through_the_fast_fake() {
        verify_fixture("decisions.json");
    }

    #[test]
    fn contradictory_scenarios_fail_before_reaching_a_driver() {
        let mut scenario = load_scenarios("routes.json").remove(0);
        scenario.expected.transferred_bytes = 0;
        assert_eq!(
            scenario.validate(),
            Err(ValidationError::InconsistentExpectedOutcome),
        );
    }

    fn load_scenarios(name: &str) -> Vec<Scenario> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sharing/scenarios/v1")
            .join(name);
        let source_result = fs::read_to_string(&path);
        assert!(source_result.is_ok(), "failed to read {}", path.display());
        let Ok(source) = source_result else {
            return Vec::new();
        };
        let scenario_result = serde_json::from_str(&source);
        assert!(
            scenario_result.is_ok(),
            "failed to parse {}",
            path.display(),
        );
        let Ok(scenarios) = scenario_result else {
            return Vec::new();
        };
        scenarios
    }

    #[test]
    fn route_matrix_runs_both_directions_through_the_fast_fake() {
        verify_fixture("routes.json");
    }

    fn verify_fixture(name: &str) {
        for scenario in load_scenarios(name) {
            let result =
                verify_scenario(&mut ScriptedPeerFake::default(), &scenario);
            assert_eq!(result, Ok(()), "{}", scenario.id);
        }
    }
}
