PKGFLAGS := $(shell pkg-config --cflags --libs libinput libudev xkbcommon)

all:
	gcc -o ./bin/input ./src/input.c $(PKGFLAGS)

run: 
	./bin/input

clean:
	rm ./bin/input
