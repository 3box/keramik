CARGO = CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse RUSTFLAGS="-D warnings" cargo

.PHONY: all
all: build check-fmt check-clippy test

.PHONY: test
test:
	# Test with default features
	${CARGO} test --locked
	# Test with all features
	${CARGO} test --locked --all-features
	# Test without default features
	${CARGO} test --locked --no-default-features

.PHONY: check-fmt
check-fmt:
	cargo fmt --all -- --check

.PHONY: check-clippy
check-clippy:
	# Check with default features
	${CARGO} clippy --workspace --all-targets
	# Check with all features
	${CARGO} clippy --workspace --all-targets --all-features

.PHONY: build
build: runner operator

.PHONY: runner
runner:
	${CARGO} build --bin keramik-runner --release --locked  --config net.git-fetch-with-cli=true

.PHONY: operator
operator:
	${CARGO} build --bin keramik-operator --release --locked  --config net.git-fetch-with-cli=true
