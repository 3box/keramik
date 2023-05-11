.PHONY: all
all: build check-fmt check-clippy test


.PHONY: test
test:
	# Test with default features
	cargo test --locked
	# Test with all features
	cargo test --locked --all-features

.PHONY: check-fmt
check-fmt:
	cargo fmt --all -- --check

.PHONY: check-clippy
check-clippy:
	# Check with default features
	cargo clippy --workspace --all-targets -- -D warnings
	# Check with all features
	cargo clippy --workspace --all-targets --all-features -- -D warnings

.PHONY: build
build: runner operator

.PHONY: runner
runner:
	RUSTFLAGS="-D warnings" cargo build --bin keramik-runner --release --locked

.PHONY: operator
operator:
	RUSTFLAGS="-D warnings" cargo build --bin keramik-operator --release --locked
