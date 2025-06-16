PROG := input
PKGFLAGS := $(shell pkg-config --cflags --libs libinput libudev xkbcommon)

all:
	gcc -o ./bin/$(PROG) ./src/$(PROG).c $(PKGFLAGS)

run: 
	./bin/$(PROG)

clean:
	rm ./bin/$(PROG)
