BINARY := synthesist

.PHONY: build install clean test lint check

build:
	cargo build --release
	cp target/release/$(BINARY) $(BINARY)

install:
	cargo install --path .

clean:
	cargo clean
	rm -f $(BINARY)

test: build
	cargo test

lint:
	cargo clippy -- -D warnings

check: build
	./$(BINARY) help > /dev/null

dev: build
	./$(BINARY) help

skill: build
	./$(BINARY) skill
