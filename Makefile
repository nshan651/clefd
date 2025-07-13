PROG := clefd
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
SYSTEMD_UNIT_DIR ?= $(HOME)/.config/systemd/user

all:
	cargo build --release

clean:
	rm -rf ./target

install:
	sudo install -Dm755 ./target/release/$(PROG) $(BINDIR)/$(PROG)
	install -Dm644 dist/systemd/$(PROG).service $(SYSTEMD_UNIT_DIR)/$(PROG).service
	systemctl --user enable --now $(PROG).service

uninstall:
	systemctl --user disable --now $(PROG).service
	rm -f $(BINDIR)/$(PROG) \
		$(SYSTEMD_UNIT_DIR)/$(PROG).service

.PHONY: all clean install uninstall
