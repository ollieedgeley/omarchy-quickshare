.PHONY: commit-msg pre-commit pre-commit-prepare pre-commit-structure pre-push
.PHONY: pre-commit-source-format pre-commit-source-lint pre-commit-source-ast
.PHONY: pre-commit-source-analysis pre-commit-test-analysis
.PHONY: pre-commit-test-format pre-commit-test-lint pre-commit-test-ast
.PHONY: pre-commit-domain-analysis pre-commit-test
.PHONY: pre-commit-test-libraries pre-commit-test-app pre-commit-test-tooling
.PHONY: pre-commit-test-integrations

commit-msg: ## Validate COMMIT_MSG_FILE as a Conventional Commit message.
	@$(TIMEOUT) node tools/hooks/commit-msg.mjs "$(COMMIT_MSG_FILE)"

pre-commit-prepare: ## Prepare and CodeGraph-sync the staged snapshot.
	@CODEGRAPH=$(CODEGRAPH) $(TIMEOUT) node tools/hooks/prepare-staged.mjs

pre-commit-structure: ## Check staged file and repository structure contracts.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs structure

pre-commit-source-format: ## Check formatter output for staged source files.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs format-source

pre-commit-source-lint: ## Lint staged source files and Rust owners.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs lint-source

pre-commit-source-ast: ## Scan staged sources with applicable AST rules.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs ast-source

pre-commit-source-analysis: ## Analyze only staged source files.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs analysis-source

pre-commit-test-format: ## Check formatter output for staged test files.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs format-tests

pre-commit-test-lint: ## Lint staged test files and new Rust owners.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs lint-tests

pre-commit-test-ast: ## Scan staged tests with applicable AST rules.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs ast-tests

pre-commit-test-analysis: ## Analyze only staged test files.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs analysis-tests

pre-commit-domain-analysis: ## Reanalyze complete staged language domains.
	@RUFF=$(RUFF) $(TIMEOUT) node tools/hooks/run-staged.mjs analysis-domain

pre-commit-test-libraries: ## Run selected Rust library and shared-contract tests.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs test-libraries

pre-commit-test-app: ## Run selected Rust application tests.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs test-app

pre-commit-test-tooling: ## Run staged and affected Node tests.
	@$(TIMEOUT) node tools/hooks/run-staged.mjs test-tooling

pre-commit-test-integrations: ## Run affected prepared-environment child gates.
	@node tools/hooks/run-staged.mjs test-integrations

pre-commit-test: pre-commit-test-libraries pre-commit-test-app \
	pre-commit-test-tooling pre-commit-test-integrations
pre-commit-test: ## Run staged and conservatively affected domain tests.

pre-commit: pre-commit-prepare pre-commit-structure \
	pre-commit-source-format pre-commit-source-lint pre-commit-source-ast \
	pre-commit-source-analysis pre-commit-test-format pre-commit-test-lint \
	pre-commit-test-ast pre-commit-test-analysis pre-commit-domain-analysis \
	pre-commit-test
pre-commit: ## Check the staged snapshot and its conservatively affected tests.

pre-push: ## Verify, then build, every exact local commit tip being pushed.
	@mkdir -p .cache/gates
	@flock --exclusive .cache/gates/pre-push.lock node tools/hooks/pre-push.mjs
