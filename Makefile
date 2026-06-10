# Valori-Kernel development shortcuts
.PHONY: dev build watch check test clean

## Build and install the Python FFI extension (development mode)
dev:
	cd ffi && maturin develop --release

## Build a release wheel (.whl) for distribution
build:
	cd ffi && maturin build --release

## Auto-rebuild on every Rust file change (requires: cargo install cargo-watch)
watch:
	cd ffi && cargo watch -s "maturin develop --release"

## Type-check all crates without full compile
check:
	cargo check -p valori-node -p valoricore-ffi

## Run the node as an HTTP server (Python can talk to it without FFI)
server:
	cargo run --release -p valori-node -- --port 8080

## Run stress test
stress:
	.venv/bin/python3 stress_test_million.py --max-n 50000 --skip-charts

## Clean build artifacts
clean:
	cargo clean
