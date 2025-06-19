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
#include <stdbool.h>
#include <xkbcommon/xkbcommon.h>

// Define the path for our named pipe.
#define FIFO_PATH "/tmp/clefd.fifo"

// Maximum number of keys that can be held down simultaneously in a chord.
#define MAX_PRESSED_KEYS 16

// Array to store the keycodes of currently pressed keys.
static xkb_keycode_t pressed_keys[MAX_PRESSED_KEYS];
// Counter for the number of keys currently pressed.
static int num_pressed_keys = 0;
// Flag to control the main event loop.
static volatile sig_atomic_t keep_running = 1;

/**
 * @brief Comparison function for qsort to sort modifier names alphabetically.
 */
static int compare_strings(const void *a, const void *b) {
  return strcmp(*(const char **)a, *(const char **)b);
}

/**
 * @brief Signal handler for graceful shutdown
 */
void sigterm_handler(int signum) {
    fprintf(stderr, "Received signal %d, initiating shutdown...\n", signum);
    keep_running = 0;
}

/**
 * @brief Opens a device file. Required by libinput.
 * @return File descriptor on success, or -errno on failure.
 */
static int open_restricted(const char *path, int flags, void *user_data) {
  int fd = open(path, flags);
  if (fd < 0) {
    fprintf(stderr, "Failed to open %s: %s\n", path, strerror(errno));
    return -errno;
  }
  return fd;
}

/**
 * @brief Closes a device file. Required by libinput.
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

/**
 * @brief Checks if a keysym is a modifier key.
 */
static bool is_modifier_keysym(xkb_keysym_t keysym) {
  switch (keysym) {
  case XKB_KEY_Shift_L:
  case XKB_KEY_Shift_R:
  case XKB_KEY_Control_L:
  case XKB_KEY_Control_R:
  case XKB_KEY_Alt_L:
  case XKB_KEY_Alt_R:
  case XKB_KEY_Super_L:
  case XKB_KEY_Super_R:
  case XKB_KEY_Meta_L:
  case XKB_KEY_Meta_R:
  case XKB_KEY_Hyper_L:
  case XKB_KEY_Hyper_R:
  case XKB_KEY_Caps_Lock:
  case XKB_KEY_Num_Lock:
  case XKB_KEY_Scroll_Lock:
    return true;
  default:
    return false;
  }
}

/**
 * @brief Adds a keycode to our list of currently pressed keys.
 * @param keycode The XKB keycode to add.
 */
void add_key(xkb_keycode_t keycode) {
  if (num_pressed_keys >= MAX_PRESSED_KEYS) {
    fprintf(stderr, "Warning: Maximum number of pressed keys exceeded.\n");
    return;
  }
  // Prevent duplicates.
  for (int i = 0; i < num_pressed_keys; i++) {
    if (pressed_keys[i] == keycode) {
      return;
    }
  }
  pressed_keys[num_pressed_keys++] = keycode;
}

/**
 * @brief Removes a keycode from our list of currently pressed keys.
 * @param keycode The XKB keycode to remove.
 */
void remove_key(xkb_keycode_t keycode) {
  int found_idx = -1;
  for (int i = 0; i < num_pressed_keys; i++) {
    if (pressed_keys[i] == keycode) {
      found_idx = i;
      break;
    }
  }

  if (found_idx == -1) {
    return;
  }

  // Shift remaining elements to fill the gap.
  for (int i = found_idx; i < num_pressed_keys - 1; i++) {
    pressed_keys[i] = pressed_keys[i + 1];
  }
  num_pressed_keys--;
}

/**
 * @brief Constructs a chord string and sends it if it's valid.
 *
 * A valid chord consists of one or more modifiers and EXACTLY ONE non-modifier
 * key. The resulting string is canonical: modifiers are sorted alphabetically,
 * followed by the single non-modifier key, all space-separated.
 *
 * @param fifo_fd The file descriptor for the named pipe.
 * @param state The current XKB state for keycode-to-keysym translation.
 */
void send_chord_event(int fifo_fd, struct xkb_state *state) {
  char chord_str[256] = {0};
  char *modifier_names[MAX_PRESSED_KEYS];
  char *key_names[MAX_PRESSED_KEYS];
  int num_modifiers = 0;
  int num_keys = 0;

  // A buffer to hold the actual string names, since our arrays hold pointers.
  char temp_name_buffer[MAX_PRESSED_KEYS][64];

  // Separate all currently pressed keys into modifiers and regular keys.
  for (int i = 0; i < num_pressed_keys; i++) {
    xkb_keycode_t keycode = pressed_keys[i];
    xkb_keysym_t keysym = xkb_state_key_get_one_sym(state, keycode);
    xkb_keysym_get_name(keysym, temp_name_buffer[i], 64);

    if (is_modifier_keysym(keysym)) {
      modifier_names[num_modifiers++] = temp_name_buffer[i];
    } else {
      key_names[num_keys++] = temp_name_buffer[i];
    }
  }

  // Reject all instances with multiple non-modifier keys.
  if (num_keys != 1) {
    return;
  }

  // Sort modifiers alphabetically for a canonical representation.
  qsort(modifier_names, num_modifiers, sizeof(char *), compare_strings);

  // Build the final chord string.
  for (int i = 0; i < num_modifiers; i++) {
    strcat(chord_str, modifier_names[i]);
    strcat(chord_str, " ");
  }

  // Append the single non-modifier key.
  strcat(chord_str, key_names[0]);

  // Write the chord string to the named pipe.
  printf("Dispatching chord: %s\n", chord_str);
  if (dprintf(fifo_fd, "%s\n", chord_str) < 0) {
    perror("Failed to write chord to FIFO");
  }
}

/**
 * @brief Handle keyboard events based on key presses or releases.
 *
 * @param fifo_fd The file descriptor for the named pipe.
 * @param state The current XKB state for keycode-to-keysym translation.
 * @param event The base event type.
 */
void keyboard_event_handler(int fifo_fd,
			    struct xkb_state *state,
			    struct libinput_event *event) {
  struct libinput_event_keyboard *kb_event =
      libinput_event_get_keyboard_event(event);
  // XKB keycodes are offset by 8 from libinput/evdev keycodes.
  xkb_keycode_t xkb_code = libinput_event_keyboard_get_key(kb_event) + 8;
  enum libinput_key_state key_state =
      libinput_event_keyboard_get_key_state(kb_event);
  uint32_t time = libinput_event_keyboard_get_time(kb_event);

  printf("Keyboard Key: time=%u, keycode=%u (%s)\n", time, xkb_code,
         (key_state == LIBINPUT_KEY_STATE_PRESSED) ? "pressed" : "released");

  if (key_state == LIBINPUT_KEY_STATE_PRESSED) {
    // Add the key to our state.
    add_key(xkb_code);

    // Check if the pressed key is a non-modifier. If so, it's the
    // trigger for the chord.
    xkb_keysym_t keysym = xkb_state_key_get_one_sym(state, xkb_code);
    if (!is_modifier_keysym(keysym)) {
      send_chord_event(fifo_fd, state);
    }
  }
  else if (key_state == LIBINPUT_KEY_STATE_RELEASED) {
    // Remove the key from our state.
    remove_key(xkb_code);
  }
}

/**
 * @brief Main event loop to read key events and process chords.
 *
 * @param fifo_fd File descriptor for the named pipe.
 * @param state The current XKB state object.
 * @return 0 on success, 1 on failure.
 */
int key_reader(int fifo_fd, struct xkb_state *state) {
  struct libinput *li;
  struct libinput_event *event;
  struct udev *udev;
  struct pollfd pfd;
  int ret;

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

  // Set up polling structure
  pfd.fd = libinput_get_fd(li);
  pfd.events = POLLIN;

  while (keep_running) {

    ret = poll(&pfd, 1, -1);

    if (ret < 0) {
      // Interrupted signal, retry polling.
      if (errno == EINTR) {
	continue;
      }
      fprintf(stderr, "poll() failed: %s\n", strerror(errno));
      break;
    }

    // Timeout, should never happen when set to -1.
    if (ret == 0) {
      continue;
    }

    // Check for errors on the polling fd.
    if (pfd.revents & (POLLERR | POLLHUP | POLLNVAL)) {
      fprintf(stderr, "Error on libinput file descriptor\n");
      break;
    }

    if (pfd.revents & POLLIN) {
      libinput_dispatch(li);

      while ((event = libinput_get_event(li)) != NULL) {
        // Only process keyboard events.
        if (libinput_event_get_type(event) == LIBINPUT_EVENT_KEYBOARD_KEY) {
          keyboard_event_handler(fifo_fd, state, event);
        }
        libinput_event_destroy(event);
      }
    }
  }

  libinput_unref(li);
  udev_unref(udev);

  return 0;
}

/**
 * @brief Main entry point.
 */
int main(int argc, char *argv[]) {
  // Register signal handler for graceful shutdown.
  signal(SIGTERM, sigterm_handler);
  signal(SIGINT, sigterm_handler);

  // Create the named pipe with read/write permission bits.
  if (mkfifo(FIFO_PATH, 0666) == -1) {
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
    unlink(FIFO_PATH);
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
    unlink(FIFO_PATH);
    return 1;
  }

  key_reader(fifo_fd, state);

  printf("Daemon shutting down...\n");

  // Cleanup.
  close(fifo_fd);
  unlink(FIFO_PATH);
  xkb_state_unref(state);
  xkb_keymap_unref(keymap);
  xkb_context_unref(ctx);
  return 0;
}
