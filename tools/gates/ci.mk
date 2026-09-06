CI_PILOT_PROFILE ?= core
CI_PILOT_ATTEMPT ?= cold

.PHONY: ci-pilot test-ci-pilot

ci-pilot: ## Measure one runner profile's preparation and real child gates.
	@node tools/gates/ci/pilot.mjs "$(CI_PILOT_PROFILE)" "$(CI_PILOT_ATTEMPT)"

test-ci-pilot: ## Check pilot dispatch, evidence, and failure contracts.
	@$(TIMEOUT) node --test tools/gates/tests/ci-pilot-contract.test.mjs
