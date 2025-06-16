/*
 * Read keyboard input events using libinput.
 *
 * This program requires the user to be part of the `input` group
 * in order to access /dev/input/event* devices.
 */
#include <errno.h>
#include <fcntl.h> // For O_RDONLY, O_WRONLY, O_RDWR, O_NONBLOCK
#include <libinput.h>
#include <libudev.h>
#include <poll.h>
#include <stdio.h>
#include <string.h> // For strerror
#include <unistd.h>
#include <signal.h>
#include <xkbcommon/xkbcommon.h>

// Define the path for our named pipe
#define FIFO_PATH "/tmp/clef-daemon.fifo"

/**
 * This function is called by libinput to open an input device file.
 * It needs to return a file descriptor.
 */
static int open_restricted(const char *path, int flags, void *user_data)
{
  int fd = open(path, flags);
  if (fd < 0) {
    fprintf(stderr, "Failed to open %s: %s\n", path, strerror(errno));
    return -errno;
  }
  return fd;
}

/**
 * Called by libinput to close a file descriptor that was previously opened.
 */
static void close_restricted(int fd, void *user_data)
{
  close(fd);
}

/**
 * Define the libinput interface.
 * These are the functions libinput will use to interact with the system
 * for opening and closing device files.
 */
static const struct libinput_interface interface = {
  .open_restricted = open_restricted,
  .close_restricted = close_restricted,
};

int key_mapper(int fifo_fd, struct xkb_state *state, xkb_keycode_t keycode) {
  xkb_keysym_t keysym;
  char keysym_name[64];

  // Note that evdev(event device)/libinput uses keycodes starting from 0, but
  // XKB uses keycodes starting from 8
  keycode += 8;
  keysym = xkb_state_key_get_one_sym(state, keycode);

  // Translate keysym to a string.
  xkb_keysym_get_name(keysym, keysym_name, sizeof(keysym_name));

  printf("xkb_keycode: %d, keysym: %d, key_name: %s\n\n",
	 keycode,
	 keysym,
         keysym_name);

  // Write the key name followed by a newline to the named pipe.
  // dprintf is a convenient function to print formatted output to a file descriptor.
  if (dprintf(fifo_fd, "%s\n", keysym_name) < 0) {
    perror("Failed to write to FIFO.");
  }

  return 0;
}


/**
 * Create an event loop to read event devices and key presses.
 */
int key_reader(int fifo_fd, struct xkb_state *state) {
  struct libinput *li;
  struct libinput_event *event;
  struct udev *udev;

  // Create a udev context.
  udev = udev_new();
  if (!udev) {
    fprintf(stderr, "Failed to create udev context\n");
    return 1;
  }

  // Create a libinput context.
  // We pass our interface (for opening/closing files) and the udev context.
  // The user_data pointer (NULL here) can be used to pass custom data to
  // open_restricted and close_restricted if needed.
  // NOTE: Show seats on linux with `loginctl list-seats`
  li = libinput_udev_create_context(&interface, NULL, udev);
  if (!li) {
    fprintf(stderr, "Failed to create libinput context\n");
    udev_unref(udev);
    return 1;
  }

  // Assign a "seat" to the libinput context.
  // A seat is a collection of input devices (e.g., a keyboard, mouse, touchscreen)
  // that typically belong to a single user. "seat0" is the default.
  libinput_udev_assign_seat(li, "seat0");

  while (1) {

    libinput_dispatch(li);
    event = libinput_get_event(li);

    if (!event) {
      // No event available right now.
      // Optimally, use select() or poll() on libinput_get_fd(li) to wait for
      // events efficiently.
      // For this basic example, we'll just continue the loop.
      // A small sleep can prevent busy-waiting if no events are immediately available.
      usleep(1000); // Sleep for 1ms
      continue;
    }

    if (libinput_event_get_type(event) == LIBINPUT_EVENT_KEYBOARD_KEY) {
      struct libinput_event_keyboard *kb_event =
          libinput_event_get_keyboard_event(event);
      uint32_t keycode = libinput_event_keyboard_get_key(kb_event);
      enum libinput_key_state key_state =
          libinput_event_keyboard_get_key_state(kb_event);
      uint32_t time = libinput_event_keyboard_get_time(kb_event);

      printf("Keyboard Key: time=%u, keycode=%u (%s)\n", time, keycode,
             (key_state == LIBINPUT_KEY_STATE_PRESSED) ? "pressed"
                                                       : "released");
      key_mapper(fifo_fd, state, keycode);
    }

    /*
    // Process the event.
    enum libinput_event_type type = libinput_event_get_type(event);

    switch (type) {

    case LIBINPUT_EVENT_NONE:
      fprintf(stderr, "Receieved LIBINPUT_EVENT_NONE\n");
      break;

    case LIBINPUT_EVENT_DEVICE_ADDED: {
      struct libinput_device *dev = libinput_event_get_device(event);
      const char* dev_name = libinput_device_get_name(dev);
      const char* sys_name = libinput_device_get_sysname(dev);
      printf("Device added: %s (%s)\n",
	     dev_name,
	     sys_name);
    } break;

    case LIBINPUT_EVENT_DEVICE_REMOVED: {
      struct libinput_device *dev = libinput_event_get_device(event);
      const char* dev_name = libinput_device_get_name(dev);
      const char* sys_name = libinput_device_get_sysname(dev);
      printf("Device removed: %s (%s)\n", libinput_device_get_name(dev),
	     libinput_device_get_sysname(dev));
    } break;

    case LIBINPUT_EVENT_KEYBOARD_KEY: {
      printf("INPUT EVENT...");
      struct libinput_event_keyboard *kb_event =
          libinput_event_get_keyboard_event(event);
      uint32_t keycode = libinput_event_keyboard_get_key(kb_event);
      enum libinput_key_state key_state =
          libinput_event_keyboard_get_key_state(kb_event);
      uint32_t time = libinput_event_keyboard_get_time(kb_event);

      printf("Keyboard Key: time=%u, keycode=%u (%s)\n", time, keycode,
             (key_state == LIBINPUT_KEY_STATE_PRESSED) ? "pressed"
                                                       : "released");
      key_mapper(state, keycode);
    } break;

    default:
      printf("Event type: %d\n", type);
      break;
    }
    */

    libinput_event_destroy(event);
  }

  libinput_unref(li);
  udev_unref(udev);

  return 0;
}

int main(int argc, char *argv[]) {
  // Ignore the SIGPIPE signal. This prevents the daemon from crashing if a
  // process reading its stdout (like in `daemon | logger`) disconnects.
  // The write() or printf() call will instead fail with an EPIPE error,
  // but the program will not terminate.
  /* signal(SIGPIPE, SIG_IGN); */

  // Create the named pipe (FIFO).
  // The 0666 permission allows any user to read/write.
  if (mkfifo(FIFO_PATH, 0666) == -1) {
    // If the error is EEXIST, the file already exists, which is fine.
    if (errno != EEXIST) {
      perror("mkfifo failed");
      return 1;
    }
  }

  printf("Daemon started. Waiting for a client to connect to %s...\n", FIFO_PATH);

  // Open the FIFO for writing.
  // This call will block until a client opens the pipe for reading.
  int fifo_fd = open(FIFO_PATH, O_WRONLY);
  if (fifo_fd == -1) {
    perror("Failed to open FIFO for writing");
    return 1;
  }

  printf("Client connected. Ready to send keypresses.\n");

  // Setup XKB.
  struct xkb_context *ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
  struct xkb_keymap *keymap =
    xkb_keymap_new_from_names(ctx, NULL, XKB_KEYMAP_COMPILE_NO_FLAGS);
  struct xkb_state *state = xkb_state_new(keymap);

  if (!ctx || !keymap || !state) {
    fprintf(stderr, "Failed to initialize XKB\n");
    xkb_state_unref(state);
    xkb_keymap_unref(keymap);
    xkb_context_unref(ctx);
    return 1;
  }

  key_reader(fifo_fd, state);

  // Free mem.
  xkb_state_unref(state);
  xkb_keymap_unref(keymap);
  xkb_context_unref(ctx);
  return 0;
}
