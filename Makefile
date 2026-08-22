# Canonical commands. CI runs these targets and nothing else, so "it passed
# locally" and "it passed in CI" mean the same thing. If you find yourself
# typing a bare cargo invocation that CI also runs, add it here instead.

SHELL := /bin/bash
.DEFAULT_GOAL := help
UI := apps/desktop/ui

.PHONY: help
help: ## List targets
	@grep -hE '^[a-z][a-z0-9_-]*:.*?## ' $(MAKEFILE_LIST) \
	  | awk -F':.*?## ' '{printf "  \033[1m%-18s\033[0m %s\n", $$1, $$2}'

# ─── The gate ────────────────────────────────────────────────────────────

.PHONY: gate
gate: fmt-check lint test evals ## Everything CI enforces. Run before every PR.
	@echo "✓ gate passed"

.PHONY: fmt
fmt: ## Format
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Fail on unformatted code
	cargo fmt --all -- --check

.PHONY: lint
lint: ## Clippy, warnings are errors, on both feature sets
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy -p riposte-detect --features onnx -- -D warnings

.PHONY: test
test: ## Whole workspace
	cargo test --workspace

.PHONY: evals
evals: ## M1 acceptance criteria and the compliance floor
	cargo run -q -p riposte-evals -- check

.PHONY: calibrate
calibrate: ## Full gate calibration report (add EMBEDDER=bge|hybrid)
	cargo run -q -p riposte-evals $(if $(filter-out lexical,$(EMBEDDER)),--features onnx,) \
	  -- calibrate $(if $(EMBEDDER),--embedder $(EMBEDDER),)

.PHONY: bench
bench: ## Gate latency against the 10 ms budget
	cargo bench -p riposte-detect

.PHONY: push
push: gate audit ## Gate, then push the current branch. Refuses on a red gate.
	@branch=$$(git rev-parse --abbrev-ref HEAD); \
	if [ "$$branch" = "main" ]; then \
	  echo "refusing: main is protected, branch first — see CONTRIBUTING.md"; exit 1; \
	fi; \
	git push -u origin "$$branch"

.PHONY: pr
pr: push ## Gate, push, and open the pull request
	gh pr create --fill

# ─── Supply chain ────────────────────────────────────────────────────────

.PHONY: audit
audit: ## Advisories, licences, banned crates, source allow-list
	cargo deny check

.PHONY: secret-scan
secret-scan: ## Fail on anything that looks like a credential
	@command -v gitleaks >/dev/null 2>&1 \
	  && gitleaks detect --no-banner --redact --exit-code 1 \
	  || echo "gitleaks not installed — CI enforces this; see docs/DEV-SETUP.md"

# ─── App ─────────────────────────────────────────────────────────────────

.PHONY: ui
ui: ## Build the overlay frontend
	npm --prefix $(UI) ci
	npm --prefix $(UI) run build

.PHONY: overlay
overlay: ## Run the overlay
	cargo run -p riposte-desktop

.PHONY: cli
cli: ## Build the headless driver
	cargo build -p riposte-cli

# ─── Housekeeping ────────────────────────────────────────────────────────

.PHONY: doctor
doctor: ## Report what this machine is missing
	@echo "rustc      $$(rustc --version 2>/dev/null || echo MISSING)"
	@echo "cargo      $$(cargo --version 2>/dev/null || echo MISSING)"
	@echo "node       $$(node --version 2>/dev/null || echo MISSING)"
	@echo "npm        $$(npm --version 2>/dev/null || echo MISSING)"
	@for t in rust-analyzer cargo-deny cargo-nextest sccache gitleaks; do \
	  printf "%-10s %s\n" "$$t" "$$(command -v $$t >/dev/null 2>&1 && echo ok || echo MISSING)"; \
	done
	@echo "See docs/DEV-SETUP.md for what each one is for."

.PHONY: clean
clean:
	cargo clean
	rm -rf $(UI)/dist $(UI)/node_modules
