use anyhow::{anyhow, Context, Result};
use input::{event::keyboard::{KeyboardEvent, KeyboardEventTrait}, Libinput, LibinputInterface};
use signal_hook::{consts::{SIGINT, SIGTERM}, iterator::Signals};
use std::fs::OpenOptions;
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use xkbcommon::xkb;

/// A simple interface for libinput to open and close devices.
/// This is required by libinput to interact with the underlying system devices.
struct Interface;

impl LibinputInterface for Interface {
    /// Opens a device file at the given path with the specified flags.
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .read(true)
            .write(true) // Required by libinput, even for read-only devices!
            .custom_flags(flags)
            .open(path)
            .map(|file| file.into())
            .map_err(|err| err.raw_os_error().unwrap_or(1))
    }

    /// Closes a device file represented by the OwnedFd.
    fn close_restricted(&mut self, _fd: OwnedFd) {
        // OwnedFd automatically closes when dropped - no unsafe code needed.
    }
}

/// Handles a single keyboard event.
///
/// This function is a placeholder for your actual chord processing logic.
/// It takes the current XKB state and the keyboard event from libinput.
///
/// # Arguments
/// * `state` - A mutable reference to the XKB state.
/// * `event` - The keyboard event to process.
fn keyboard_event_handler(state: &mut xkb::State, event: &KeyboardEvent) {
    let keycode = event.key();
    let key_state = event.key_state();
    println!(
        "Key Event: code={}, state={:?}",
        keycode,
        key_state,
    );

    // Example of using xkbcommon to get the symbol for the key.
    // Note: The keycode from libinput needs a +8 offset to match XKB keycodes.
    let keysym = state.key_get_one_sym((keycode + 8).into());
    let key_name = xkb::keysym_get_name(keysym);
    println!("  -> Keysym: {} ({})", key_name, keysym.raw());
}

/// Main event loop to read key events and process chords.
///
/// This function sets up libinput with a udev backend and enters a loop
/// to listen for keyboard events.
///
/// # Arguments
/// * `state` - The XKB state object.
/// * `keep_running` - An atomic boolean to control the event loop.
fn run_event_loop(mut state: xkb::State,
		  keep_running: Arc<AtomicBool>) -> Result<()> {
    // Create a libinput context with a udev backend.
    // This allows libinput to discover and manage input devices automatically.
    let mut libinput = Libinput::new_with_udev(Interface);

    // Assign the default seat "seat0" to the context. A "seat" represents
    // a collection of input devices used by a single user.
    libinput.udev_assign_seat("seat0")
        .map_err(|_| anyhow!("Failed to assign seat 'seat0'"))?;

    println!("Event loop started. Waiting for keyboard input...");

    // Process incoming libinput events.
    while keep_running.load(Ordering::SeqCst) {
        // Dispatch events from libinput.
        libinput.dispatch().context("Failed to dispatch libinput events")?;

        // Iterate over all available events from libinput.
        for event in &mut libinput {
            // We only care about keyboard events.
            if let input::Event::Keyboard(kb_event) = event {
                keyboard_event_handler(&mut state, &kb_event);
            }
        }
    }

    Ok(())
}

/// Main entry point for the application.
fn main() -> Result<()> {
    // Set up an atomic boolean to control the main loop.
    // This allows us to gracefully shut down from a signal handler.
    let keep_running = Arc::new(AtomicBool::new(true));
    let keep_running_clone = keep_running.clone();

    // Register a signal handler for SIGINT and SIGTERM.
    // This ensures that the application can clean up and shut down gracefully
    // when the user presses Ctrl+C or the system sends a termination signal.
    let mut signals = Signals::new(&[SIGINT, SIGTERM])
        .context("Failed to register signal handlers")?;

    // Spawn a thread to listen for signals.
    std::thread::spawn(move || {
        for _ in signals.forever() {
            println!("\nSignal received, shutting down daemon...");
            keep_running_clone.store(false, Ordering::SeqCst);
        }
    });

    println!("Daemon started.");

    // Initialize the XKB context.
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

    // Create a keymap from the system's current keyboard configuration.
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "", // rules
        "", // model
        "", // layout
        "", // variant
        None, // options
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ).ok_or_else(|| anyhow!("Failed to create XKB keymap"))?;

    // Create an XKB state object from the keymap.
    let state = xkb::State::new(&keymap);

    // Run the main event loop.
    if let Err(e) = run_event_loop(state, keep_running) {
        eprintln!("An error occurred: {:?}", e);
    }

    println!("Daemon has shut down.");

    Ok(())
}
