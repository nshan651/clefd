PROG := clefd
PKGFLAGS := $(shell pkg-config --cflags --libs libinput libudev xkbcommon)
CC := gcc
CFLAGS := -Wall -Wextra -std=c11
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
SYSTEMD_UNIT_DIR ?= /etc/systemd/system

.PHONY: all clean install uninstall

all:
	gcc -o ./bin/$(PROG) ./src/$(PROG).c $(PKGFLAGS)

clean:
	rm -f ./bin/$(PROG)

server:
	./bin/$(PROG)

client:
	guile ./src/clef.scm

install:
	install -Dm755 ./bin/$(PROG) $(BINDIR)/$(PROG)
	install -Dm644 dist/systemd/$(PROG).service $(SYSTEMD_UNIT_DIR)/$(PROG).service

	-systemctl enable $(PROG).service
	-systemctl start $(PROG).service

	systemctl daemon-reload


uninstall:
	systemctl stop $(PROG).service
	systemctl disable $(PROG).service

	systemctl daemon-reload

	rm -f $(BINDIR)/$(PROG) \ 
		$(SYSTEMD_UNIT_DIR)/$(PROG).service
