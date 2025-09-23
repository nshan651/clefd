PKGNAME := clefd
BINDIR ?= /usr/local/bin
SYSTEMD_UNIT_DIR ?= $(HOME)/.config/systemd/user
INIT_SYS = $(shell ps -p 1 -o comm=)

all:
	cargo build --release

test:
	cargo test

check:
	cargo fmt -- --check
	cargo clippy --all-targets --all-features -- -D warnings

cov:
	cargo tarpaulin --out Html --fail-under 70

clean:
	rm -rf ./target

install:
	install -Dm755 ./target/release/$(PKGNAME) $(BINDIR)/$(PKGNAME)
	install -Dm644 dist/systemd/$(PKGNAME).service $(SYSTEMD_UNIT_DIR)/$(PKGNAME).service

uninstall:
	rm -f $(BINDIR)/$(PKGNAME) \
		$(SYSTEMD_UNIT_DIR)/$(PKGNAME).service

.PHONY: all clean install uninstall
