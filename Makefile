# SPDX-License-Identifier: GPL-3.0-or-later
# Entry points for building and checking QuakeC Studio (docs/conventions.md, CONV-8).
# Logic that needs Rust lives in xtask; `make ci` runs exactly what CI runs.

CARGO ?= cargo
FUZZ_SECONDS ?= 60

.DEFAULT_GOAL := help
.PHONY: help fmt lint check test test-integration test-fuzz test-coverage test-mutants \
	audit docs-build bench oracles ci

help: ## List the targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

fmt: ## Format every crate
	$(CARGO) fmt --all

lint: ## Run clippy on every target with warnings denied
	$(CARGO) clippy --locked --workspace --all-targets --all-features -- -D warnings

check: ## Type-check every target
	$(CARGO) check --locked --workspace --all-targets --all-features

test: ## Run every test
	$(CARGO) test --locked --workspace --all-features

test-integration: ## Run the cross-crate integration tests (qs-tests)
	$(CARGO) test --locked --package qs-tests

test-fuzz: ## Run each fuzz target for FUZZ_SECONDS (needs nightly and cargo-fuzz)
	$(CARGO) xtask fuzz --time $(FUZZ_SECONDS)

test-coverage: ## Measure test coverage (needs cargo-llvm-cov)
	$(CARGO) llvm-cov --locked --workspace --all-features

test-mutants: ## Run mutation testing (needs cargo-mutants)
	$(CARGO) mutants --workspace

audit: ## Check dependencies (cargo-deny), SPDX and ported-file headers, and asset attribution
	$(CARGO) deny --locked check advisories bans licenses sources
	$(CARGO) xtask licenses
	$(CARGO) xtask attribution

docs-build: ## Build the API docs with warnings denied, and the mdBook once docs/book.toml exists
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --locked --workspace --no-deps --all-features
	@if [ -f docs/book.toml ]; then mdbook build docs; fi

bench: ## Run the performance-budget benchmarks and record them in the history (docs/perf-budgets.md)
	$(CARGO) xtask bench

oracles: ## Build the test-only oracles (id's VM and qcc, FTEQCC) and check the VM oracle (needs curl and a C compiler)
	$(CARGO) xtask oracles

ci: ## Run exactly what CI runs (cargo xtask ci)
	$(CARGO) xtask ci
