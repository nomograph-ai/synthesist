BINARY := claim

.PHONY: help build install clean test lint check release

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*##' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*##"}; {printf "  %-15s %s\n", $$1, $$2}'

build: ## Build release binary
	cargo build --release
	cp target/release/$(BINARY) $(BINARY)

install: ## Install binary via cargo
	cargo install --path .

clean: ## Remove all build artifacts (cargo target/ + dist/ + binary)
	cargo clean
	rm -f $(BINARY)
	rm -rf dist/ 2>/dev/null || true

test: ## Run all tests
	cargo test

lint: ## Run clippy
	cargo clippy -- -D warnings

check: ## Quick cargo check
	cargo check

.DEFAULT_GOAL := help
