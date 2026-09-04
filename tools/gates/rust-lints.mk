.PHONY: lint-rust-clippy lint-rust-docs lint-rust-analyzer

lint-rust-clippy: ## Run strict Clippy for the workspace or selected packages.
	@RUST_LINT_STEP=clippy $(TIMEOUT) node tools/gates/rust-lints.mjs

lint-rust-docs: ## Fail rustdoc warnings for the workspace public API.
	@RUST_LINT_STEP=docs $(TIMEOUT) node tools/gates/rust-lints.mjs

lint-rust-analyzer: ## Run rust-analyzer diagnostics for the workspace.
	@RUST_LINT_STEP=analyzer $(TIMEOUT) node tools/gates/rust-lints.mjs

lint-rust: lint-rust-clippy lint-rust-docs lint-rust-analyzer ## Split Clippy, rustdoc, and rust-analyzer under the 60s child budget.
