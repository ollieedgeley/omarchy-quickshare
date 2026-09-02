SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.DELETE_ON_ERROR:
.NOTPARALLEL:

TIMEOUT ?= timeout --foreground 60s
NODE_BIN ?= $(CURDIR)/node_modules/.bin
AST_GREP ?= $(NODE_BIN)/ast-grep
CODEGRAPH ?= $(NODE_BIN)/codegraph
PRETTIER ?= $(NODE_BIN)/prettier
MARKDOWNLINT ?= $(NODE_BIN)/markdownlint-cli2

.PHONY: help setup hooks-install sources-fetch oracle-provision oracle-up oracle-down
.PHONY: format format-check check lint-rust lint-ast lint-docs lint-structure lint-sources lint-oracle
.PHONY: test test-rust test-tooling test-ast-rules test-source-cache test-oracle-toolchain verify build commit-msg
.PHONY: pre-commit pre-commit-prepare pre-commit-structure pre-commit-format pre-commit-ast
.PHONY: pre-commit-rust pre-commit-test pre-push

help: ## List public and targeted gates.
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_.-]+:.*## / {printf "%-28s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup: ## Install pinned development tools and activate repository hooks.
	@npm ci
	@rustup toolchain install 1.98.0 --profile minimal --component rustfmt --component clippy --component rust-analyzer
	@$(MAKE) hooks-install
	@$(MAKE) sources-fetch
	@$(MAKE) oracle-provision

hooks-install: ## Activate Husky and initialize the reusable staged CodeGraph mirror.
	@$(NODE_BIN)/husky
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) node tools/hooks/prepare-staged.mjs --initialize

sources-fetch: ## Download, hash-check, and extract every pinned test source.
	@node tools/gates/sources.mjs fetch

oracle-provision: ## Build the pinned oracle toolchain image; one-time provisioning may exceed 60 seconds.
	@node tests/environments/oracle/environment.mjs provision

oracle-up: ## Start and readiness-check the prepared oracle environment within 60 seconds.
	@node tests/environments/oracle/environment.mjs up

oracle-down: ## Stop the prepared oracle environment within 60 seconds.
	@node tests/environments/oracle/environment.mjs down

format: ## Deliberately rewrite supported files with pinned formatters.
	@cargo fmt --all
	@$(PRETTIER) --write . --ignore-unknown

format-check: ## Check Rust, Markdown, YAML, and JSON formatting.
	@$(TIMEOUT) cargo fmt --all -- --check
	@$(TIMEOUT) $(PRETTIER) --check . --ignore-unknown

check: ## Compiler-check the complete Cargo workspace.
	@$(TIMEOUT) cargo check --workspace --all-targets --all-features --locked

lint-rust: ## Run compiler, rustdoc, rust-analyzer, and strict Clippy checks.
	@$(TIMEOUT) node tools/gates/rust-lints.mjs

lint-ast: ## Run the full error-only ast-grep scan.
	@$(TIMEOUT) node tools/gates/ast-schema.mjs
	@$(TIMEOUT) $(AST_GREP) scan --config sgconfig.yml --error --min-severity=error --max-results=1 --inspect=summary .

lint-docs: ## Run Markdown policy checks.
	@$(TIMEOUT) $(MARKDOWNLINT) '**/*.md' '#node_modules' '#target' '#.cache'

lint-structure: ## Check line, directory, dependency, and configuration contracts.
	@$(TIMEOUT) node tools/gates/structure.mjs

lint-sources: ## Validate immutable source revisions, hashes, licenses, and purposes.
	@$(TIMEOUT) node tools/gates/sources.mjs check

lint-oracle: ## Validate pinned oracle image inputs without starting Docker.
	@$(TIMEOUT) node tests/environments/oracle/environment.mjs validate

test-rust: ## Run complete workspace Rust tests and doc tests.
	@$(TIMEOUT) cargo test --workspace --all-targets --all-features --locked
	@$(TIMEOUT) cargo test --workspace --doc --all-features --locked

test-tooling: ## Run quality-gate and hook contract tests.
	@$(TIMEOUT) npm run test:tooling

test-ast-rules: ## Run ast-grep rule fixtures and committed snapshots.
	@$(TIMEOUT) $(AST_GREP) test --config sgconfig.yml

test-source-cache: ## Hash-check the prepared reference and simulator source archives.
	@$(TIMEOUT) node tools/gates/sources.mjs verify-cache

test-oracle-toolchain: ## Warm-start, test, and stop the prepared oracle toolchain.
	@trap 'node tests/environments/oracle/environment.mjs down' EXIT; \
		node tests/environments/oracle/environment.mjs up; \
		$(TIMEOUT) node tests/environments/oracle/environment.mjs self-test

test: test-rust test-tooling test-ast-rules ## Run all local tests.

verify: format-check lint-docs lint-structure lint-sources lint-oracle check lint-rust lint-ast test test-source-cache test-oracle-toolchain ## Run the complete local quality suite.

build: ## Build the complete locked workspace after verification.
	@$(TIMEOUT) cargo build --workspace --all-targets --all-features --locked

commit-msg: ## Validate COMMIT_MSG_FILE as a Conventional Commit message.
	@$(TIMEOUT) node tools/hooks/commit-msg.mjs "$(COMMIT_MSG_FILE)"

pre-commit: ## Check the staged snapshot and its conservatively affected tests.
	@$(MAKE) pre-commit-prepare
	@$(MAKE) pre-commit-structure
	@$(MAKE) pre-commit-format
	@$(MAKE) pre-commit-ast
	@$(MAKE) pre-commit-rust
	@$(MAKE) pre-commit-test

pre-commit-prepare: ## Materialize, validate, and CodeGraph-sync the staged snapshot.
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) node tools/hooks/prepare-staged.mjs

pre-commit-structure: ## Check staged file and repository structure contracts.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs structure

pre-commit-format: ## Check formatter output for staged files only.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs format

pre-commit-ast: ## Run strict ast-grep rules against staged Rust files.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs ast

pre-commit-rust: ## Run compiler-backed diagnostics for staged Rust owners.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs rust

pre-commit-test: ## Run changed and conservatively affected tests.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs test

pre-push: ## Verify, then build, every exact local commit tip being pushed.
	@node tools/hooks/pre-push.mjs
