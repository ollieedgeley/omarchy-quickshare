SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.DELETE_ON_ERROR:
.NOTPARALLEL:

TIMEOUT ?= timeout --foreground 60s
ANDROID_TEST_TIMEOUT ?= timeout --foreground 300s
NODE_BIN ?= $(CURDIR)/node_modules/.bin
AST_GREP ?= $(NODE_BIN)/ast-grep
CODEGRAPH ?= $(NODE_BIN)/codegraph
ESLINT ?= $(NODE_BIN)/eslint
PRETTIER ?= $(NODE_BIN)/prettier
MARKDOWNLINT ?= $(NODE_BIN)/markdownlint-cli2
RUFF ?= $(CURDIR)/.cache/tools/ruff-0.16.0/ruff
TEST_ENV_CACHE ?= $(CURDIR)/.cache/test-env
QUICKSHELL ?= quickshell
REPOSITORY_FILES = git ls-files --cached --others --exclude-standard -z

.PHONY: help setup hooks-install ruff-provision sources-fetch wire-codegen
.PHONY: format format-app format-tooling
.PHONY: format-check format-app-check format-tooling-check check
.PHONY: lint-rust lint-javascript lint-python lint-ast lint-docs
.PHONY: lint-structure lint-structure-app lint-structure-tooling
.PHONY: lint-sources lint-oracle lint-nearshare lint-nearby-linux
.PHONY: lint-diverse-lan lint-proxies
.PHONY: lint-dbus
.PHONY: lint-bluetooth-radio lint-network lint-android
.PHONY: test test-rust test-contracts test-tooling test-ast-rules
.PHONY: test-source-cache test-plugin-release test-local-install plugin-export
.PHONY: install-local
.PHONY: test-oracle-toolchain test-oracle-reference
.PHONY: test-nearshare-reference test-nearby-linux-tooling rust-lan-provision
.PHONY: test-rust-lan test-rust-lan-outbound test-rust-lan-inbound
.PHONY: test-nearby-linux-connections test-nearby-linux-sharing
.PHONY: test-nearby-linux-sharing-actions sharing-fixtures-update
.PHONY: test-nearby-linux-sharing-fixtures
.PHONY: test-oracle-bluetooth test-oracle-ble test-oracle-lan
.PHONY: test-oracle-hotspot test-oracle-wifi-direct
.PHONY: test-oracle-bwu-handler test-oracle-bwu-fallback
.PHONY: test-proxy-toxiproxy test-dbus-bluez test-dbus-networkmanager
.PHONY: test-bluetooth-controller test-bluetooth-ble
.PHONY: test-bluetooth-classic test-network-wmediumd test-network-netem
.PHONY: test-network-lan test-network-hotspot-client
.PHONY: test-network-hotspot-owner test-network-wifi-direct-client
.PHONY: verify-app verify-tooling verify build commit-msg
.PHONY: pre-commit pre-commit-prepare pre-commit-structure
.PHONY: pre-commit-format pre-commit-javascript pre-commit-python
.PHONY: pre-commit-ast
.PHONY: pre-commit-rust pre-commit-test pre-push

help: ## List public and targeted gates.
	@awk 'BEGIN {FS = ":.*## "} \
		/^[a-zA-Z0-9_.-]+:.*## / {printf "%-28s %s\n", $$1, $$2}' \
		$(MAKEFILE_LIST)

setup: ## Install pinned development tools and activate repository hooks.
	@npm ci
	@rustup toolchain install 1.98.0 --profile minimal \
		--component rustfmt --component clippy --component rust-analyzer
	@$(MAKE) ruff-provision
	@$(MAKE) hooks-install
	@$(MAKE) sources-fetch
	@$(MAKE) oracle-provision
	@$(MAKE) oracle-reference-provision
	@$(MAKE) nearshare-provision
	@$(MAKE) nearby-linux-provision
	@$(MAKE) proxy-provision
	@$(MAKE) dbus-provision
	@$(MAKE) bluetooth-radio-provision
	@$(MAKE) network-provision
	@$(MAKE) live-bwu-kvm-provision

hooks-install: ## Activate Husky and initialize the staged CodeGraph mirror.
	@$(NODE_BIN)/husky
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) \
		node tools/hooks/prepare-staged.mjs --initialize

ruff-provision: ## Install the pinned standalone Python quality tool.
	@tools/setup/ruff.sh

sources-fetch: ## Download, hash-check, and extract every pinned test source.
	@node tools/gates/sources.mjs fetch

wire-codegen: ## Regenerate committed Nearby and UKEY2 Rust bindings.
	@tools/codegen/generate-wire.sh

format: format-app format-tooling ## Rewrite files with pinned formatters.

format-app: ## Rewrite Rust application files with rustfmt.
	@cargo fmt --all

format-tooling: ## Rewrite tooling and repository-document files.
	@$(RUFF) format .
	@set -o pipefail; $(REPOSITORY_FILES) | \
		xargs --null --no-run-if-empty $(PRETTIER) --write \
			--ignore-unknown --

format-check: format-app-check format-tooling-check ## Check formatted files.

format-app-check: ## Check only Rust application formatting.
	@$(TIMEOUT) cargo fmt --all -- --check

format-tooling-check: ## Check tooling and repository-document formatting.
	@$(TIMEOUT) $(RUFF) format --check .
	@set -o pipefail; $(REPOSITORY_FILES) | \
		$(TIMEOUT) xargs --null --no-run-if-empty $(PRETTIER) --check \
			--ignore-unknown --

check: ## Compiler-check the complete Cargo workspace.
	@$(TIMEOUT) cargo check --workspace --all-targets --all-features --locked

lint-rust: ## Run compiler, rustdoc, rust-analyzer, and strict Clippy checks.
	@$(TIMEOUT) node tools/gates/rust-lints.mjs

lint-javascript: ## Run every current ESLint core rule as an error.
	@$(TIMEOUT) $(ESLINT) . --max-warnings 0 --no-warn-ignored

lint-python: ## Run every enabled Ruff rule against Python tooling.
	@$(TIMEOUT) $(RUFF) check .

lint-ast: ## Run the full error-only ast-grep scan.
	@$(TIMEOUT) node tools/gates/ast-schema.mjs
	@$(TIMEOUT) $(AST_GREP) scan --config sgconfig.yml --error \
		--min-severity=error --max-results=1 --inspect=summary .

lint-docs: ## Run Markdown policy checks.
	@$(TIMEOUT) $(MARKDOWNLINT) '**/*.md' '#node_modules' '#target' '#.cache'

lint-structure: lint-structure-app lint-structure-tooling
lint-structure: ## Check all structure contracts.

lint-structure-app: ## Check application structure contracts.
	@$(TIMEOUT) node tools/gates/structure.mjs app

lint-structure-tooling: ## Check tooling structure contracts.
	@$(TIMEOUT) node tools/gates/structure.mjs tooling

lint-sources: ## Validate immutable source definitions.
	@$(TIMEOUT) node tools/gates/sources.mjs check

lint-oracle: ## Validate pinned oracle image inputs without starting Docker.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs validate

lint-nearshare: ## Validate the pinned diverse peer without starting Docker.
	@$(TIMEOUT) node tests/environments/nearshare/environment.mjs validate

lint-nearby-linux: ## Validate Google-derived Linux peer inputs statically.
	@$(TIMEOUT) node tests/environments/nearby-linux/environment.mjs validate

lint-diverse-lan: ## Validate isolated diverse-LAN interop inputs statically.
	@$(TIMEOUT) node tests/environments/diverse-lan/environment.mjs validate

lint-proxies: ## Validate pinned proxy environment inputs without starting it.
	@$(TIMEOUT) node tests/environments/proxies/environment.mjs validate

lint-dbus: ## Validate pinned private D-Bus environment inputs.
	@$(TIMEOUT) node tests/environments/bluez/dbus-environment.mjs validate

lint-bluetooth-radio: ## Validate the pinned BlueZ and Bumble radio image.
	@$(TIMEOUT) node tests/environments/bluez/radio-environment.mjs validate

lint-network: ## Validate pinned virtual Wi-Fi environment inputs.
	@$(TIMEOUT) node tests/environments/network/environment.mjs validate

lint-android: ## Validate pinned Android SDK, probe, and AVD inputs.
	@$(TIMEOUT) node tests/environments/android/environment.mjs validate

test-rust: ## Run complete workspace Rust tests and doc tests.
	@$(TIMEOUT) cargo test --workspace --all-targets --all-features --locked
	@$(TIMEOUT) cargo test --workspace --doc --all-features --locked

test-contracts: ## Run shared transfer scenarios against fast test doubles.
	@$(TIMEOUT) cargo test -p quickshare-contract-tests --test suite --locked

test-tooling: ## Run quality-gate and hook contract tests.
	@$(TIMEOUT) npm run test:tooling

test-plugin-release: ## Check the plugin export and native status states.
	@QUICKSHELL=$(QUICKSHELL) $(TIMEOUT) node --test \
		tools/release/tests/plugin-release-contract.test.mjs

test-local-install: ## Check local binary and systemd-user-service installation.
	@$(TIMEOUT) node --test tools/release/tests/local-install-contract.test.mjs

install-local: ## Build, install, and start the local user service.
	@node tools/release/local-install.mjs

install-local-simulation: ## Install the local service with simulated peers.
	@node tools/release/local-install.mjs --simulate

plugin-export: ## Create the validated local plugin Git repository.
	@node tools/release/plugin-export.mjs

test-ast-rules: ## Run ast-grep rule fixtures and committed snapshots.
	@$(TIMEOUT) $(AST_GREP) test --config sgconfig.yml

test-source-cache: ## Hash-check prepared source archives.
	@$(TIMEOUT) node tools/gates/sources.mjs verify-cache

test-oracle-toolchain: ## Test the prepared oracle toolchain.
	@trap 'node tests/environments/oracle/environment.mjs down' EXIT; \
		node tests/environments/oracle/environment.mjs up; \
		$(TIMEOUT) node tests/environments/oracle/environment.mjs self-test

test-oracle-reference: ## Test UKEY2 and a secure-session exchange.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs reference-self-test
	@UKEY2_SHELL="$(TEST_ENV_CACHE)/oracle/bin/ukey2_shell" \
		RUSTFLAGS="--cfg quickshare_oracle_reference" \
		$(TIMEOUT) cargo test -p quickshare-crypto --test oracle_interop \
		--locked

test-nearshare-reference: ## Test full LAN Sharing in both peer roles.
	@trap 'node tests/environments/nearshare/environment.mjs down' EXIT; \
		node tests/environments/nearshare/environment.mjs up; \
		$(TIMEOUT) node tests/environments/nearshare/environment.mjs self-test

test-nearby-linux-tooling: ## Test Nearby Linux environment contracts.
	@$(TIMEOUT) node --test tests/environments/nearby-linux/*.test.mjs

test-nearby-linux-connections: ## Exchange exact bytes over Connections LAN.
	@trap 'node tests/environments/nearby-linux/environment.mjs down' EXIT; \
		node tests/environments/nearby-linux/environment.mjs up; \
		$(TIMEOUT) node tests/environments/nearby-linux/environment.mjs \
			connections-self-test

test-nearby-linux-sharing: ## Accept Sharing transfers in both peer roles.
	@trap 'node tests/environments/nearby-linux/environment.mjs down' EXIT; \
		node tests/environments/nearby-linux/environment.mjs up; \
		$(TIMEOUT) node tests/environments/nearby-linux/environment.mjs self-test

test-nearby-linux-sharing-actions: ## Reject and cancel Sharing both ways.
	@trap 'node tests/environments/nearby-linux/environment.mjs down' EXIT; \
		node tests/environments/nearby-linux/environment.mjs up; \
		$(TIMEOUT) node tests/environments/nearby-linux/environment.mjs \
			sharing-actions-self-test

sharing-fixtures-update: ## Regenerate pinned Google-derived Sharing fixtures.
	@node tests/environments/nearby-linux/sharing/fixtures/runner.mjs update

test-nearby-linux-sharing-fixtures: ## Compare pinned Sharing fixtures.
	@$(TIMEOUT) node \
		tests/environments/nearby-linux/sharing/fixtures/runner.mjs compare

test-diverse-lan: ## Exchange bytes across diverse same-LAN reference peers.
	@trap 'node tests/environments/diverse-lan/environment.mjs down' EXIT; \
		node tests/environments/diverse-lan/environment.mjs up; \
		$(TIMEOUT) node tests/environments/diverse-lan/environment.mjs self-test

rust-lan-provision: ## Build the current Rust daemon into the LAN test image.
	@node tests/environments/diverse-lan/rust/rust-lan.mjs provision

test-rust-lan: rust-lan-provision test-rust-lan-outbound
test-rust-lan: test-rust-lan-inbound ## Verify daemon LAN transfers both ways.

test-rust-lan-outbound: ## Send from Rust to the Google-derived LAN peer.
	@$(TIMEOUT) node --test \
		tests/environments/diverse-lan/rust/rust-lan-outbound.e2e.mjs

test-rust-lan-inbound: ## Receive from the Google-derived LAN peer in Rust.
	@$(TIMEOUT) node --test \
		tests/environments/diverse-lan/rust/rust-lan-inbound.e2e.mjs

test-oracle-bluetooth: ## Check Google's simulated Bluetooth Classic medium.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs \
		medium-self-test bluetooth

test-oracle-ble: ## Check Google's simulated BLE medium.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs medium-self-test ble

test-oracle-lan: ## Check Google's simulated Wi-Fi LAN medium.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs medium-self-test lan

test-oracle-hotspot: ## Check Google's simulated Wi-Fi hotspot medium.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs \
		medium-self-test hotspot

test-oracle-wifi-direct: ## Check Google's simulated Wi-Fi Direct medium.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs \
		medium-self-test wifi-direct

test-oracle-bwu-handler: ## Check simulated BT, Direct, and LAN BWU handlers.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs \
		bwu-handler-self-test

test-oracle-bwu-fallback: ## Check selected simulated Google BWU fallback.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs \
		bwu-fallback-self-test

test-proxy-toxiproxy: ## Test bidirectional TCP cutoff and recovery.
	@trap 'node tests/environments/proxies/environment.mjs down' EXIT; \
		node tests/environments/proxies/environment.mjs up; \
		$(TIMEOUT) node tests/environments/proxies/environment.mjs self-test

test-dbus-bluez: ## Check the BlueZ mock through the real bluetoothctl client.
	@trap 'node tests/environments/bluez/dbus-environment.mjs down' EXIT; \
		node tests/environments/bluez/dbus-environment.mjs up; \
		$(TIMEOUT) node tests/environments/bluez/dbus-environment.mjs self-test bluez

test-dbus-networkmanager: ## Check the NetworkManager mock through real nmcli.
	@trap 'node tests/environments/bluez/dbus-environment.mjs down' EXIT; \
		node tests/environments/bluez/dbus-environment.mjs up; \
		$(TIMEOUT) node tests/environments/bluez/dbus-environment.mjs \
		self-test networkmanager

test-bluetooth-controller: ## Check isolated controllers through real BlueZ.
	@trap 'node tests/environments/bluez/radio-environment.mjs down' EXIT; \
		node tests/environments/bluez/radio-environment.mjs up; \
		$(TIMEOUT) node tests/environments/bluez/radio-environment.mjs \
		self-test controller

test-bluetooth-ble: ## Exchange exact bytes both ways over virtual BLE GATT.
	@trap 'node tests/environments/bluez/radio-environment.mjs down' EXIT; \
		node tests/environments/bluez/radio-environment.mjs up; \
		$(TIMEOUT) node tests/environments/bluez/radio-environment.mjs self-test ble

test-bluetooth-classic: ## Exchange exact bytes both ways over virtual RFCOMM.
	@trap 'node tests/environments/bluez/radio-environment.mjs down' EXIT; \
		node tests/environments/bluez/radio-environment.mjs up; \
		$(TIMEOUT) node tests/environments/bluez/radio-environment.mjs \
		self-test classic

test-network-wmediumd: ## Prove hwsim 802.11 delivery, isolation, and recovery.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs self-test medium

test-network-netem: ## Prove deterministic UDP loss and recovery through netem.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs self-test netem

test-network-lan: ## Associate two hwsim clients and transfer TCP both ways.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs self-test lan

test-network-hotspot-client: ## Join a hwsim hotspot and transfer both ways.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs \
		self-test hotspot-client

test-network-hotspot-owner: ## Host a real hwsim hotspot and transfer both ways.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs \
		self-test hotspot-owner

test-network-wifi-direct-client: ## Join a remote P2P group and transfer.
	@trap 'node tests/environments/network/environment.mjs down' EXIT; \
		node tests/environments/network/environment.mjs up; \
		$(TIMEOUT) node tests/environments/network/environment.mjs \
		self-test wifi-direct-client

test-android-nearby: ## Exchange verified bytes and files through both AVDs.
	@trap 'node tests/environments/android/environment.mjs down' EXIT; \
		node tests/environments/android/environment.mjs up; \
		$(ANDROID_TEST_TIMEOUT) node \
			tests/environments/android/environment.mjs self-test

test: test-rust test-tooling test-ast-rules ## Run all local tests.

verify-app: ## Run application gates without test environments.
verify-app: format-app-check lint-structure-app check lint-rust
verify-app: lint-ast test-rust

verify-tooling: ## Run tooling static and fast contract gates.
verify-tooling: format-tooling-check lint-javascript lint-python lint-docs
verify-tooling: lint-structure-tooling lint-sources lint-oracle lint-nearshare
verify-tooling: lint-nearby-linux lint-diverse-lan lint-proxies
verify-tooling: lint-dbus lint-bluetooth-radio lint-network lint-android
verify-tooling: test-tooling test-ast-rules test-nearby-linux-tooling

verify: ## Run the complete local quality suite.
verify: format-check lint-structure lint-javascript lint-python lint-docs \
	lint-ast lint-sources lint-oracle lint-nearshare lint-nearby-linux \
	lint-diverse-lan lint-proxies lint-dbus lint-bluetooth-radio \
	lint-network lint-android check lint-rust test-ast-rules \
	test-nearby-linux-tooling test-tooling test-rust test-source-cache \
	test-oracle-toolchain test-oracle-reference test-nearshare-reference \
	test-nearby-linux-connections test-nearby-linux-sharing \
	test-nearby-linux-sharing-actions test-nearby-linux-sharing-fixtures \
	test-diverse-lan test-rust-lan test-oracle-bluetooth test-oracle-ble \
	test-oracle-lan test-oracle-hotspot test-oracle-wifi-direct \
	test-oracle-bwu-handler test-oracle-bwu-fallback test-proxy-toxiproxy \
	test-dbus-bluez test-dbus-networkmanager test-bluetooth-controller \
	test-bluetooth-ble test-bluetooth-classic test-network-wmediumd \
	test-network-netem test-network-lan test-network-hotspot-client \
	test-network-hotspot-owner test-network-wifi-direct-client

build: ## Build the complete locked workspace after verification.
	@$(TIMEOUT) cargo build --workspace --all-targets --all-features --locked

commit-msg: ## Validate COMMIT_MSG_FILE as a Conventional Commit message.
	@$(TIMEOUT) node tools/hooks/commit-msg.mjs "$(COMMIT_MSG_FILE)"

pre-commit: ## Check the staged snapshot and its conservatively affected tests.
	@$(MAKE) pre-commit-prepare
	@$(MAKE) pre-commit-structure
	@$(MAKE) pre-commit-format
	@$(MAKE) pre-commit-javascript
	@$(MAKE) pre-commit-python
	@$(MAKE) pre-commit-ast
	@$(MAKE) pre-commit-rust
	@$(MAKE) pre-commit-test

pre-commit-prepare: ## Prepare and CodeGraph-sync the staged snapshot.
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) node tools/hooks/prepare-staged.mjs

pre-commit-structure: ## Check staged file and repository structure contracts.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs structure

pre-commit-format: ## Check formatter output for staged files only.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs format

pre-commit-javascript: ## Run strict ESLint against staged JavaScript only.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs javascript

pre-commit-python: ## Run strict Ruff checks against staged Python only.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs python

pre-commit-ast: ## Run strict ast-grep rules against staged Rust files.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs ast

pre-commit-rust: ## Run compiler-backed diagnostics for staged Rust owners.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs rust

pre-commit-test: ## Run changed and conservatively affected tests.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs test

pre-push: ## Verify, then build, every exact local commit tip being pushed.
	@node tools/hooks/pre-push.mjs

include tools/gates/environments.mk
