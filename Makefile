.PHONY: pre-commit build test

# Run before every commit; CI runs the same three checks.
pre-commit:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test
	@echo "All pre-commit checks passed."

build:
	cargo build --release

test:
	cargo test
