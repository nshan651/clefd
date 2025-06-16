PROG := clefd
PKGFLAGS := $(shell pkg-config --cflags --libs libinput libudev xkbcommon)

all:
	gcc -o ./bin/$(PROG) ./src/$(PROG).c $(PKGFLAGS)

server:
	./bin/$(PROG)

client:
	guile ./src/clef.scm

clean:
	rm ./bin/$(PROG)
