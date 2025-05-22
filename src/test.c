/*
 * Basic C program to read keyboard input events using libinput.
 *
 * Compilation:
 * gcc -o libinput_reader libinput_reader.c $(pkg-config --cflags --libs libinput libudev)
 *
 * Running:
 * This program usually needs root privileges to access /dev/input/event* devices.
 * sudo ./libinput_reader
 * Alternatively, your user might need to be in the 'input' group,
 * and udev rules might need to be set up correctly.
 *
 * Dependencies:
 * libinput-dev (or equivalent for your distribution)
 * libudev-dev (or equivalent for your distribution)
 */

#include <stdio.h>
#include <fcntl.h>      // For O_RDONLY, O_WRONLY, O_RDWR, O_NONBLOCK
#include <unistd.h>     // For open, close, read, write
#include <errno.h>      // For errno
#include <string.h>     // For strerror
#include <libudev.h>    // For udev interactions
#include <libinput.h>   // The star of the show

// This function is called by libinput to open an input device file.
// It needs to return a file descriptor.
static int open_restricted(const char *path, int flags, void *user_data) {
    int fd = open(path, flags);
    if (fd < 0) {
        fprintf(stderr, "Failed to open %s (%s)\n", path, strerror(errno));
        return -errno; // libinput expects negative errno on failure
    }
    return fd;
}

// This function is called by libinput to close a file descriptor
// that was previously opened by open_restricted.
static void close_restricted(int fd, void *user_data) {
    close(fd);
}

// Define the libinput interface.
// These are the functions libinput will use to interact with the system
// for opening and closing device files.
static const struct libinput_interface interface = {
    .open_restricted = open_restricted,
    .close_restricted = close_restricted,
};

int main(void) {
    struct udev *udev;
    struct libinput *li;
    struct libinput_event *event;

    // 1. Create a udev context.
    // udev is used by libinput to find and manage input devices.
    udev = udev_new();
    if (!udev) {
        fprintf(stderr, "Failed to initialize udev\n");
        return 1;
    }

    // 2. Create a libinput context.
    // We pass our interface (for opening/closing files) and the udev context.
    // The user_data pointer (NULL here) can be used to pass custom data to
    // open_restricted and close_restricted if needed.
    li = libinput_udev_create_context(&interface, NULL, udev);
    if (!li) {
        fprintf(stderr, "Failed to initialize libinput from udev\n");
        udev_unref(udev);
        return 1;
    }

    // 3. Assign a "seat" to the libinput context.
    // A seat is a collection of input devices (e.g., a keyboard, mouse, touchscreen)
    // that typically belong to a single user. "seat0" is the default.
    if (libinput_udev_assign_seat(li, "seat0") != 0) {
        fprintf(stderr, "Failed to assign seat0\n");
        libinput_unref(li);
        udev_unref(udev);
        return 1;
    }

    printf("libinput initialized. Listening for events (Press Ctrl+C to exit)...\n");

    // 4. Main event loop.
    // libinput_dispatch() prepares events, libinput_get_event() retrieves them.
    while (1) {
        // Dispatch any pending events.
        libinput_dispatch(li);
        event = libinput_get_event(li);

        if (!event) {
            // No event available right now.
            // In a real application, you might use select() or poll() on
            // libinput_get_fd(li) to wait for events efficiently.
            // For this basic example, we'll just continue the loop.
            // A small sleep can prevent busy-waiting if no events are immediately available.
            usleep(1000); // Sleep for 1ms
            continue;
        }

        // 5. Process the event.
        enum libinput_event_type type = libinput_event_get_type(event);

        switch (type) {
            case LIBINPUT_EVENT_NONE:
                // Should not happen.
                fprintf(stderr, "Received LIBINPUT_EVENT_NONE\n");
                break;

            case LIBINPUT_EVENT_DEVICE_ADDED:
                {
                    struct libinput_device *dev = libinput_event_get_device(event);
                    printf("Device added: %s (%s)\n",
                           libinput_device_get_name(dev),
                           libinput_device_get_sysname(dev));
                }
                break;

            case LIBINPUT_EVENT_DEVICE_REMOVED:
                {
                    struct libinput_device *dev = libinput_event_get_device(event);
                    printf("Device removed: %s (%s)\n",
                           libinput_device_get_name(dev),
                           libinput_device_get_sysname(dev));
                }
                break;

            case LIBINPUT_EVENT_KEYBOARD_KEY:
                {
                    struct libinput_event_keyboard *kb_event = libinput_event_get_keyboard_event(event);
                    uint32_t keycode = libinput_event_keyboard_get_key(kb_event);
                    enum libinput_key_state key_state = libinput_event_keyboard_get_key_state(kb_event);
                    uint32_t time = libinput_event_keyboard_get_time(kb_event);

                    printf("Keyboard Key: time=%u, keycode=%u (%s)\n",
                           time,
                           keycode,
                           (key_state == LIBINPUT_KEY_STATE_PRESSED) ? "pressed" : "released");
                }
                break;

            // Add cases for other event types you're interested in:
            // LIBINPUT_EVENT_POINTER_MOTION, LIBINPUT_EVENT_POINTER_BUTTON,
            // LIBINPUT_EVENT_TOUCH_DOWN, LIBINPUT_EVENT_GESTURE_SWIPE_BEGIN, etc.
            default:
                // For this basic example, we'll just print the type of other events.
                printf("Event type: %d\n", type);
                break;
        }

        // 6. Destroy the event object after processing.
        libinput_event_destroy(event);
    }

    // 7. Clean up.
    // This part is unreachable in this simple infinite loop example,
    // but crucial in a real application.
    // A signal handler for SIGINT (Ctrl+C) could trigger this cleanup.
    libinput_unref(li); // Destroys the libinput context
    udev_unref(udev);   // Destroys the udev context

    return 0;
}
