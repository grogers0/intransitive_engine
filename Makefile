.PHONY: all clean test build build-debug run run-debug

all: build-debug test

clean:
	cargo clean

build-debug:
	cargo build --workspace

build:
	cargo build --workspace --release

test:
	cargo test --workspace

run-debug:
	cargo run

run:
	cargo run --release
