VITEST ?= $(NODE_BIN)/vitest
QMLTESTRUNNER ?= /usr/lib/qt6/bin/qmltestrunner
RUST_PACKAGE ?= quickshare-contract-tests
RUST_TEST_FILTER ?=

.PHONY: test-release-tools test-environment-tools test-rust-focused
.PHONY: test-qml-components

test-tooling: ## Run retained fast tooling, release and environment tests.
	@$(TIMEOUT) $(VITEST) run

test-release-tools: ## Run retained release behavior without live scenarios.
	@$(TIMEOUT) $(VITEST) run --project release

test-environment-tools: ## Run retained environment helpers without labs.
	@$(TIMEOUT) $(VITEST) run --project environments

test-rust-focused: ## Select a Rust package and optional test-name filter.
	@$(TIMEOUT) cargo test --package "$(RUST_PACKAGE)" --locked \
		"$(RUST_TEST_FILTER)"

ifeq ($(RUST_PACKAGE),omarchy-quickshare)
test-rust-focused: native-debug-prepare
endif

test-qml-components: ## Exercise real QML components with controlled inputs.
	@QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
		$(TIMEOUT) $(QMLTESTRUNNER) -input tests/components \
		-import tests/components/imports -o -,txt

test-plugin-release: ## Check plugin exports and simulated status states.
	@QUICKSHELL=$(QUICKSHELL) $(TIMEOUT) $(VITEST) run \
		tools/release/tests/plugin-release-contract.test.mjs

test-plugin-capture: ## Check capture journeys with prepared debug binaries.
	@QT_QPA_PLATFORM=offscreen QUICKSHELL=$(QUICKSHELL) $(VITEST) run \
		--config vitest.process.config.mjs \
		tools/release/tests/capture-journey.e2e.mjs

test-local-install: ## Check installation with simulated service commands.
	@$(TIMEOUT) $(VITEST) run \
		tools/release/tests/local-install-contract.test.mjs

test-source-build: ## Check source closure with simulated builds.
	@$(TIMEOUT) $(VITEST) run \
		tools/release/tests/source-build-contract.test.mjs

test-native-release: ## Check generated archives and fixture binary metadata.
	@$(TIMEOUT) $(VITEST) run \
		tools/release/tests/native-release-contract.test.mjs

test-nearby-linux-tooling: ## Test retained Nearby Linux helpers recursively.
	@$(TIMEOUT) $(VITEST) run --project environments \
		tests/environments/nearby-linux

test-rust-lan-outbound: ## Send from Rust to the Google-derived LAN peer.
test-rust-lan-inbound: ## Receive from the Google-derived LAN peer in Rust.
test-rust-lan-inbound-content: ## Receive reference text and URL bytes.
test-rust-lan-outbound-content: ## Send Rust text and URL bytes.
test-rust-lan-rejection: ## Reject before payload persistence in both roles.
test-rust-lan-retry: ## Continue after Retry 12 with file data in both roles.
test-rust-lan-cancellation-inbound: ## Reference sender cancels before consent.
test-rust-lan-cancellation-outbound: ## Rust sender cancels before consent.
test-rust-lan-failure-inbound: ## Reference sender disappears before consent.
test-rust-lan-failure-outbound: ## Reference receiver disappears before consent.
$(RUST_LAN_GATES): test-rust-lan-%:
	@$(VITEST) run --config vitest.process.config.mjs \
		$(RUST_LAN_TEST_DIR)/rust-lan-$*.e2e.mjs
