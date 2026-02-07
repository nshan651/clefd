PKGNAME := clefd
BINDIR ?= /usr/local/bin
SYSTEMD_UNIT_DIR ?= $(HOME)/.config/systemd/user
INIT_SYS = $(shell ps -p 1 -o comm=)

.PHONY: all build test bench lint doc format update clean install uninstall

all: build test lint doc

build:
	cargo build --release

test:
	cargo test

lint:
	cargo check
	cargo clippy

doc:
	cargo doc

cov:
	cargo tarpaulin --out Html --fail-under 70

bench:
	cargo bench

format:
	cargo fmt

update:
	cargo update

clean:
	rm -rf ./target

install:
	install -Dm755 ./target/release/$(PKGNAME) $(BINDIR)/$(PKGNAME)
	install -Dm644 dist/systemd/$(PKGNAME).service $(SYSTEMD_UNIT_DIR)/$(PKGNAME).service

uninstall:
	rm -f $(BINDIR)/$(PKGNAME) \
		$(SYSTEMD_UNIT_DIR)/$(PKGNAME).service
