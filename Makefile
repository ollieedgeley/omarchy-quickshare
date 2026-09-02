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

.PHONY: help setup hooks-install ruff-provision sources-fetch
.PHONY: oracle-provision oracle-reference-provision oracle-up oracle-down
.PHONY: proxy-provision proxy-up proxy-down
.PHONY: dbus-provision dbus-up dbus-down
.PHONY: bluetooth-radio-provision bluetooth-radio-up bluetooth-radio-down
.PHONY: network-provision network-up network-down
.PHONY: android-preflight android-bootstrap android-license android-provision
.PHONY: android-orchestrator-provision
.PHONY: android-seed android-up android-down test-android-nearby
.PHONY: format format-app format-tooling
.PHONY: format-check format-app-check format-tooling-check check
.PHONY: lint-rust lint-javascript lint-python lint-ast lint-docs
.PHONY: lint-structure lint-structure-app lint-structure-tooling
.PHONY: lint-sources lint-oracle lint-proxies lint-dbus
.PHONY: lint-bluetooth-radio lint-network lint-android
.PHONY: test test-rust test-contracts test-tooling test-ast-rules
.PHONY: test-source-cache
.PHONY: test-oracle-toolchain test-oracle-reference
.PHONY: test-oracle-bluetooth test-oracle-ble test-oracle-lan
.PHONY: test-oracle-hotspot test-oracle-wifi-direct
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
	@$(MAKE) proxy-provision
	@$(MAKE) dbus-provision
	@$(MAKE) bluetooth-radio-provision
	@$(MAKE) network-provision

hooks-install: ## Activate Husky and initialize the staged CodeGraph mirror.
	@$(NODE_BIN)/husky
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) \
		node tools/hooks/prepare-staged.mjs --initialize

ruff-provision: ## Install the pinned standalone Python quality tool.
	@tools/setup/ruff.sh

sources-fetch: ## Download, hash-check, and extract every pinned test source.
	@node tools/gates/sources.mjs fetch

oracle-provision: ## Build the pinned oracle toolchain image.
	@node tests/environments/oracle/environment.mjs provision

oracle-reference-provision: ## Build the pinned Google UKEY2 artifacts.
	@node tests/environments/oracle/environment.mjs reference-provision

oracle-up: ## Start and readiness-check the prepared oracle environment.
	@node tests/environments/oracle/environment.mjs up

oracle-down: ## Stop the prepared oracle environment within 60 seconds.
	@node tests/environments/oracle/environment.mjs down

proxy-provision: ## Build pinned Toxiproxy and prove its offline rebuild.
	@node tests/environments/proxies/environment.mjs provision

proxy-up: ## Start and readiness-check Toxiproxy, measured outside test time.
	@node tests/environments/proxies/environment.mjs up

proxy-down: ## Stop Toxiproxy, measured outside test time.
	@node tests/environments/proxies/environment.mjs down

dbus-provision: ## Build the pinned BlueZ and NetworkManager private-bus image.
	@node tests/environments/bluez/dbus-environment.mjs provision

dbus-up: ## Start and readiness-check the private D-Bus environment.
	@node tests/environments/bluez/dbus-environment.mjs up

dbus-down: ## Stop the private D-Bus environment outside test time.
	@node tests/environments/bluez/dbus-environment.mjs down

bluetooth-radio-provision: ## Build the pinned BlueZ and Bumble radio image.
	@node tests/environments/bluez/radio-environment.mjs provision

bluetooth-radio-up: ## Boot and readiness-check the isolated radio guest.
	@node tests/environments/bluez/radio-environment.mjs up

bluetooth-radio-down: ## Stop the isolated radio guest and report teardown.
	@node tests/environments/bluez/radio-environment.mjs down

network-provision: ## Build pinned Wi-Fi tools and deterministic wmediumd.
	@node tests/environments/network/environment.mjs provision

network-up: ## Start isolated hwsim radios and report readiness time.
	@node tests/environments/network/environment.mjs up

network-down: ## Remove isolated radios and report teardown time.
	@node tests/environments/network/environment.mjs down

android-preflight: ## Check host support for the pinned Android AVD lab.
	@$(TIMEOUT) node tests/environments/android/environment.mjs preflight

android-bootstrap: ## Fetch and verify the pinned Android host tools.
	@node tests/environments/android/environment.mjs bootstrap

android-orchestrator-provision: ## Build the pinned Mobly controller image.
	@node tests/environments/android/environment.mjs orchestrator-provision

android-license: ## Interactively review the Android SDK license.
	@node tests/environments/android/environment.mjs license

android-provision: ## Install the SDK, create AVDs, and build the probe.
	@node tests/environments/android/environment.mjs provision

android-seed: ## Cold-boot both AVDs and save their Quick Boot state.
	@node tests/environments/android/environment.mjs seed

android-up: ## Boot both AVDs, wait for readiness, and install the probe.
	@node tests/environments/android/environment.mjs up

android-down: ## Stop both AVDs and report teardown time.
	@node tests/environments/android/environment.mjs down

format: format-app format-tooling ## Rewrite files with pinned formatters.

format-app: ## Rewrite Rust application files with rustfmt.
	@cargo fmt --all

format-tooling: ## Rewrite tooling and repository-document files.
	@$(RUFF) format .
	@$(PRETTIER) --write . --ignore-unknown

format-check: format-app-check format-tooling-check ## Check formatted files.

format-app-check: ## Check only Rust application formatting.
	@$(TIMEOUT) cargo fmt --all -- --check

format-tooling-check: ## Check tooling and repository-document formatting.
	@$(TIMEOUT) $(RUFF) format --check .
	@$(TIMEOUT) $(PRETTIER) --check . --ignore-unknown

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
verify-tooling: lint-structure-tooling lint-sources lint-oracle lint-proxies
verify-tooling: lint-dbus lint-bluetooth-radio lint-network lint-android
verify-tooling: test-tooling test-ast-rules

verify: ## Run the complete local quality suite.
verify: verify-app verify-tooling test-source-cache
verify: test-oracle-toolchain test-oracle-reference
verify: test-oracle-bluetooth test-oracle-ble test-oracle-lan
verify: test-oracle-hotspot test-oracle-wifi-direct test-proxy-toxiproxy
verify: test-dbus-bluez test-dbus-networkmanager
verify: test-bluetooth-controller test-bluetooth-ble
verify: test-bluetooth-classic test-network-wmediumd test-network-netem
verify: test-network-lan test-network-hotspot-client
verify: test-network-hotspot-owner test-network-wifi-direct-client

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
