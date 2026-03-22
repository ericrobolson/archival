.PHONY: build release run test clean

build:
	cargo build

release:
	cargo build --release

run: build
	cargo run -- $(ARGS)

test:
	cargo test

clean:
	cargo clean

archival-clean:
	cargo run -- . --clean

archival-test: 
	cargo run -- . --llm-cmd "claude --print"  --verbose

install:
	cargo build --release
	mkdir -p ~/bin
	cp target/release/archival ~/bin/archival

uninstall:
	rm ~/bin/archival
