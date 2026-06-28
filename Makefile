# Valori-Kernel development shortcuts
.PHONY: dev build watch check test clean server cluster cluster-down stress

## Build and install the Python FFI extension (development mode)
dev:
	cd python && maturin develop --release

## Build a release wheel (.whl) for distribution
build:
	cd python && maturin build --release

## Auto-rebuild on every Rust file change (requires: cargo install cargo-watch)
watch:
	cd python && cargo watch -s "maturin develop --release"

## Type-check the default workspace members without a full compile
check:
	cargo check --workspace --exclude valori-embedded --exclude valori-ffi

## Run the full test suite (excludes firmware + PyO3 crates)
test:
	cargo test --workspace --exclude valori-embedded --exclude valori-ffi

## Run a single standalone node as an HTTP server on 0.0.0.0:3000
server:
	VALORI_BIND=0.0.0.0:3000 cargo run --release -p valori-node

## Bring up a 3-node Raft cluster in Docker
cluster:
	docker compose up -d --build

## Tear the cluster down and wipe its volumes
cluster-down:
	docker compose down -v

## Run the million-vector stress test (capped for a quick local run)
stress:
	.venv/bin/python3 scripts/stress_test_million.py --max-n 50000 --skip-charts

## Clean build artifacts
clean:
	cargo clean
