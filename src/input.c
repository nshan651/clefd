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
#include <xkbcommon/xkbcommon.h>

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

int key_mapper(struct xkb_state *state, xkb_keycode_t keycode) {
  xkb_keysym_t keysym;
  char keysym_name[64];

  // a=30, z=44
  /* keycode = 30; */

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

  return 0;
}


/**
 * Create an event loop to read event devices and key presses.
 */
int key_reader(struct xkb_state *state) {
  struct libinput *li;
  struct libinput_event *event;
  struct udev *udev;
  struct pollfd pfd;

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

  // Get the file descriptor for polling.
  //pfd.fd = libinput_get_fd(li);

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

    libinput_event_destroy(event);
  }

  libinput_unref(li);
  udev_unref(udev);

  return 0;
}

int main(int argc, char *argv[]) {
  struct xkb_context *ctx;
  struct xkb_keymap *keymap;
  struct xkb_state *state;

  ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
  if (!ctx) {
    fprintf(stderr, "Failed to create xkb_context\n");
    return 1;
  }

  // Use system keyboard layout by passing NULL for xkb rules.
  keymap = xkb_keymap_new_from_names(ctx,
				     NULL,
				     XKB_KEYMAP_COMPILE_NO_FLAGS);
  if (!keymap) {
    fprintf(stderr, "Failed to create xkb_keymap\n");
    xkb_context_unref(ctx);
    return 1;
  }

  // The xkb state remembers things like which keyboard modifiers and LEDs are
  // active
  state = xkb_state_new(keymap);
  if (!state) {
    fprintf(stderr, "Failed to create xkb_state\n");
    xkb_keymap_unref(keymap);
    xkb_context_unref(ctx);
    return 1;
  }

  key_reader(state);

  // Free mem.
  xkb_state_unref(state);
  xkb_keymap_unref(keymap);
  xkb_context_unref(ctx);
  return 0;
}
