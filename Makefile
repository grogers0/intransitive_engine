.PHONY: all clean test build build-dev build-debug run run-debug

all: build-dev test

clean:
	cargo clean

build-dev:
	cargo build --workspace

build:
	cargo build --workspace --release

build-debug:
	cargo build --workspace --profile release-debug

test:
	cargo test --workspace

run: build
	./rpsbot.py -- target/release/rpsi_bot

run-debug: build-debug
	./rpsbot.py -- target/release-debug/rpsi_bot


