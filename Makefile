CARGO = CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse RUSTFLAGS="--cfg tokio_unstable -D warnings" cargo

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

.PHONY: update
update:
	${CARGO} update

.PHONY: check-clippy
check-clippy:
	# Check with default features
	${CARGO} clippy --workspace
	# Check with all features
	${CARGO} clippy --workspace --all-features

.PHONY: build
build: update runner operator

.PHONY: runner
runner:
	${CARGO} build --bin keramik-runner --release --locked  --config net.git-fetch-with-cli=true

.PHONY: operator
operator:
	${CARGO} build --bin keramik-operator --release --locked  --config net.git-fetch-with-cli=true

.PHONY: book
book:
	mdbook build

.PHONY: book-serve
book-serve:
	mdbook serve --open
