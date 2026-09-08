# Included by the repository Makefile; run commands from the repository root.
.PHONY: desktop-prepare desktop-start desktop-status desktop-logs desktop-inspect desktop-stop desktop-prepare-inspect desktop-prepare-stop
desktop-prepare: ## Prepare an identified Omarchy base from the pinned ISO.
	@python3 tests/environments/desktop/environment.py prepare
desktop-start: ## Boot a private writable Omarchy desktop, or reuse a healthy guest.
	@python3 tests/environments/desktop/environment.py start
desktop-status: ## Report selected lab ownership and live readiness.
	@python3 tests/environments/desktop/environment.py status
desktop-logs: ## Read selected lab retained logs and reachable guest journals.
	@python3 tests/environments/desktop/environment.py logs
desktop-inspect: ## Report owned desktop endpoints, live readiness and screenshot.
	@python3 tests/environments/desktop/environment.py inspect
desktop-stop: ## Stop only DESKTOP_LAB, retaining its writable state.
	@python3 tests/environments/desktop/environment.py stop
desktop-prepare-inspect: ## Inspect a retained preparation guest independently.
	@python3 tests/environments/desktop/environment.py prepare-inspect
desktop-prepare-stop: ## Stop only the retained preparation guest.
	@python3 tests/environments/desktop/environment.py prepare-stop

.PHONY: native-build native-deploy
NATIVE_COMPONENT ?= all
export NATIVE_COMPONENT
native-build: ## Build NATIVE_COMPONENT=all|cli|daemon and record artifact identity.
	@python3 tests/environments/desktop/build_native.py
native-deploy: ## Deploy NATIVE_COMPONENT=all|cli|daemon into DESKTOP_LAB.
	@python3 tests/environments/desktop/deploy_native.py $(if $(NATIVE_ARTIFACT),--artifact "$(NATIVE_ARTIFACT)")

.PHONY: panel-deploy
panel-deploy: ## Deploy QML and verify its loaded revision without a native build.
	@python3 tests/environments/desktop/deploy_panel.py $(if $(PANEL_SOURCE),--source "$(PANEL_SOURCE)")

.PHONY: desktop-peer-prepare desktop-peer-start desktop-peer-status
.PHONY: desktop-peer-inspect desktop-peer-logs desktop-peer-stop desktop-peer-remove
desktop-peer-prepare: ## Prepare the identified independent reference runtime.
	@python3 tests/environments/desktop/reference_peer.py prepare
desktop-peer-start: ## Start the independent peer on DESKTOP_LAB's private LAN.
	@python3 tests/environments/desktop/reference_peer.py start
desktop-peer-status: ## Report retained peer identity and current container state.
	@python3 tests/environments/desktop/reference_peer.py status
desktop-peer-inspect: ## Inspect selected peer resources and guest connectivity.
	@python3 tests/environments/desktop/reference_peer.py inspect
desktop-peer-logs: ## Read bounded independent peer diagnostics.
	@python3 tests/environments/desktop/reference_peer.py logs
desktop-peer-stop: ## Stop only the selected peer while retaining its files.
	@python3 tests/environments/desktop/reference_peer.py stop
desktop-peer-remove: ## Remove selected peer containers and network, retaining evidence.
	@python3 tests/environments/desktop/reference_peer.py remove

.PHONY: desktop-scenario
desktop-scenario: ## Send a private file through the application and verify peer receipt.
	@$(VITEST) run --config vitest.process.config.mjs \
		tests/scenarios/desktop.e2e.mjs

.PHONY: desktop-resources desktop-cleanup-inspect desktop-cleanup
desktop-resources: ## Report joint owned VM, peer and worker allocations.
	@python3 tests/environments/desktop/resource_budget.py report
desktop-cleanup-inspect: ## Preview eligible expired owned data without deleting it.
	@python3 tests/environments/desktop/retention.py dry-run
desktop-cleanup: ## Apply retention policy only to eligible owned data.
	@python3 tests/environments/desktop/retention.py apply

.PHONY: desktop-candidate desktop-clean-install desktop-ui-scenario
desktop-candidate: ## Assemble local candidate packages from identified native bytes.
	@python3 tests/environments/desktop/candidate.py $(if $(NATIVE_ARTIFACT),--artifact "$(NATIVE_ARTIFACT)") $(if $(DESKTOP_PLUGIN),--plugin "$(DESKTOP_PLUGIN)")
desktop-clean-install: ## Install DESKTOP_CANDIDATE into a new DESKTOP_LAB from DESKTOP_BASE.
	@python3 tests/environments/desktop/clean_install.py $(if $(DESKTOP_CANDIDATE),--candidate "$(DESKTOP_CANDIDATE)") $(if $(DESKTOP_LAB),--lab "$(DESKTOP_LAB)") $(if $(DESKTOP_BASE),--base "$(DESKTOP_BASE)")
desktop-ui-scenario: ## Exercise the installed panel identified by DESKTOP_INSTALLATION.
	@DESKTOP_SCENARIO_INSTALLATION="$${DESKTOP_INSTALLATION:?set DESKTOP_INSTALLATION}" \
		$(VITEST) run --config vitest.process.config.mjs \
		tests/scenarios/desktop.e2e.mjs

.PHONY: desktop-clean-job desktop-clean-recover
desktop-clean-job: ## Run the installed-candidate journey and clean only its owned resources.
	@python3 tests/environments/desktop/clean_job.py run $(if $(DESKTOP_CANDIDATE),--candidate "$(DESKTOP_CANDIDATE)") $(if $(DESKTOP_BASE),--base "$(DESKTOP_BASE)")
desktop-clean-recover: ## Recover an abandoned DESKTOP_JOB without replacing its original result.
	@python3 tests/environments/desktop/clean_job.py recover $(if $(DESKTOP_JOB),--job "$(DESKTOP_JOB)")
