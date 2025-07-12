PROG := clefd
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
SYSTEMD_UNIT_DIR ?= /etc/systemd/system

all:
	cargo build --release

clean:
	rm -f ./bin/$(PROG)

install: all
	install -Dm755 ./bin/$(PROG) $(BINDIR)/$(PROG)
	#install -Dm644 dist/systemd/$(PROG).service $(SYSTEMD_UNIT_DIR)/$(PROG).service

	#-systemctl enable $(PROG).service
	#-systemctl start $(PROG).service

	#systemctl daemon-reload


uninstall:
	#systemctl stop $(PROG).service
	#systemctl disable $(PROG).service

	#systemctl daemon-reload
	rm -f $(BINDIR)/$(PROG) \
		$(SYSTEMD_UNIT_DIR)/$(PROG).service

.PHONY: all clean install uninstall
