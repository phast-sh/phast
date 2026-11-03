#---------------------------------------------------------------------
# Common development commands. Run `make` (or `make help`) for the list.
#
# Pass arguments to the binary with ARGS, e.g.:
#   make run ARGS="install -v"
#   make run-release ARGS="cli version"
#
# Narrow any test target with TEST, which is forwarded verbatim, e.g.:
#   make test-cli TEST=install                 # one file
#   make test-e2e TEST=update NETWORK=1        # one workflow, with a live registry
#   make test-e2e TEST="update -- --nocapture" # and show what it printed
#
# A skipped workflow still reports ok, since a test cannot be marked ignored at runtime,
# so --nocapture is the only way to tell a skip from a pass.
#---------------------------------------------------------------------

BIN  := phast
ARGS ?=
TEST ?=

.DEFAULT_GOAL := help

.PHONY: help debug release size run-debug run-release test test-unit test-cli test-e2e test-bench fmt-check fmt clippy rustdoc leaks audit schema schema-check docs docs-serve clean

help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*## "} {printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

#---------------------------------------------------------------------
# BUILD
#---------------------------------------------------------------------

debug: ## Debug build of the phast binary
	cargo build -p $(BIN)

release: ## Optimized release build
	cargo build -p $(BIN) --release

size: release ## Size of the release binary, the number users download
	@ls -lh target/release/$(BIN) | awk '{printf "  %-8s %s\n", $$5, $$9}'

#---------------------------------------------------------------------
# RUN
#---------------------------------------------------------------------

run-debug: ## Run the debug binary (ARGS="...")
	cargo run -p $(BIN) -- $(ARGS)

run-release: ## Run the release binary (ARGS="...")
	cargo run -p $(BIN) --release -- $(ARGS)

#---------------------------------------------------------------------
# TEST
#---------------------------------------------------------------------

test: debug ## Run every tier (TEST=<filter>)
	cargo test --workspace $(TEST)

test-unit: ## Library APIs only, no binary and no network (TEST=<filter>)
	cargo test --workspace --exclude phast-cli-tests --exclude phast-e2e-tests --exclude phast-perf-tests $(TEST)

test-cli: debug ## Per-command contract against a mocked registry (TEST=<filter>)
	cargo test -p phast-cli-tests $(TEST)

test-e2e: debug ## Workflows (TEST=<filter>); NETWORK=1 also runs the ones needing a live registry
	$(if $(NETWORK),PHAST_TEST_NETWORK=1 )cargo test -p phast-e2e-tests $(TEST)

test-bench: ## Time installs against Composer on real projects (PROJECT=<name>, RUNS=<n>; needs hyperfine)
	PROJECT=$(PROJECT) RUNS=$(RUNS) ./testing/perf/bench.sh

#---------------------------------------------------------------------
# CHECK
#---------------------------------------------------------------------

fmt-check: ## Dry-run format the workspace
	cargo fmt --check

fmt: ## Format the workspace
	cargo fmt

clippy: ## Lint with clippy, warnings as errors
	cargo clippy --all-targets -- -D warnings

rustdoc: ## Build the Rust API docs, warnings as errors (broken links, private leaks)
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

leaks: ## Scan the whole git history for leaked credentials (needs gitleaks)
	gitleaks git --redact --no-banner --verbose .

audit: ## Check Cargo.lock against the RustSec advisory database (needs cargo-audit)
	cargo audit

#---------------------------------------------------------------------
# VALIDATE
#---------------------------------------------------------------------

schema-check: ## Fail if the committed schema has drifted from the types
	cargo run -q -p phast-core --features schema --bin schema -- --check

schema: ## Regenerate schema/v1/manifest.json from the manifest types
	cargo run -q -p phast-core --features schema --bin schema

#---------------------------------------------------------------------
# DOCUMENT
#---------------------------------------------------------------------

docs: ## Build the documentation site into site/ (fails on warnings)
	uv run --group docs mkdocs build --strict

docs-serve: ## Live-preview the docs at http://127.0.0.1:8000
	uv run --group docs mkdocs serve

#---------------------------------------------------------------------
# CLEAN
#---------------------------------------------------------------------

clean: ## Remove build artifacts
	cargo clean
