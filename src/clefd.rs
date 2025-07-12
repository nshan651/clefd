use anyhow::{anyhow, Context, Error, Result};
use input::{event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait}, Libinput, LibinputInterface};
use signal_hook::{consts::{SIGINT, SIGTERM}, iterator::Signals};
use std::collections::{HashSet, HashMap};
use std::fs::OpenOptions;
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::fs;
use std::process::Command;
use xkbcommon::xkb;
use xkbcommon::xkb::{keysyms, Keysym, Keycode};

const MAX_PRESSED_KEYS: usize = 16;

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

/// Manages the state of currently pressed keys for chord detection.
struct ChordState {
    pressed_keys: HashSet<xkb::Keycode>,
}

impl ChordState {
    /// Creates a new, empty ChordState.
    fn new() -> Self {
        Self {
            pressed_keys: HashSet::with_capacity(MAX_PRESSED_KEYS),
        }
    }

    /// Adds a keycode to the set of currently pressed keys.
    /// The HashSet automatically handles duplicates.
    fn add_key(&mut self, keycode: xkb::Keycode) {
        if self.pressed_keys.len() >= MAX_PRESSED_KEYS {
            eprintln!("Warning: Maximum number of pressed keys exceeded.");
            return;
        }
        self.pressed_keys.insert(keycode);
        println!("  -> Current Keys: {:?}", self.pressed_keys);
    }

    /// Removes a keycode from the set of currently pressed keys.
    fn remove_key(&mut self, keycode: xkb::Keycode) {
        self.pressed_keys.remove(&keycode);
        println!("  -> Current Keys: {:?}", self.pressed_keys);
    }

    /// Constructs a chord string and sends it if it's valid.
    ///
    /// A valid chord consists of one or more modifiers and EXACTLY ONE non-modifier
    /// key. The resulting string is canonical: modifiers are sorted alphabetically,
    /// followed by the single non-modifier key, all space-separated.
    fn send_chord_event(&self, xkb_state: &xkb::State) {
        let mut modifier_names = Vec::new();
        let mut key_names = Vec::new();

        // Separate all currently pressed keys into modifiers and regular keys.
        for &keycode in self.pressed_keys.iter() {
            let keysym = xkb_state.key_get_one_sym(keycode);
            let name = xkb::keysym_get_name(keysym);

            if is_modifier_keysym(keysym) {
                modifier_names.push(name);
            } else {
                key_names.push(name);
            }
        }

        // A valid chord trigger has exactly one non-modifier key.
        if key_names.len() != 1 {
            return;
        }

        // Sort modifiers alphabetically for a canonical representation.
        modifier_names.sort_unstable();

        // Combine the sorted modifiers and the single key name.
        let mut chord_parts = modifier_names;
        chord_parts.extend(key_names);
        let keychord = chord_parts.join(" ");

        // Write the chord string.
        println!("Dispatching chord: {}", keychord);
    }
}

/// Checks if a given keysym is a modifier key.
fn is_modifier_keysym(keysym: Keysym) -> bool {
    matches!(keysym.into(),
        keysyms::KEY_Shift_L | keysyms::KEY_Shift_R |
        keysyms::KEY_Control_L | keysyms::KEY_Control_R |
        keysyms::KEY_Alt_L | keysyms::KEY_Alt_R |
        keysyms::KEY_Meta_L | keysyms::KEY_Meta_R |
        keysyms::KEY_Super_L | keysyms::KEY_Super_R |
        keysyms::KEY_Hyper_L | keysyms::KEY_Hyper_R |
        keysyms::KEY_Scroll_Lock |
        keysyms::KEY_Caps_Lock |
        keysyms::KEY_Shift_Lock
    )
}

/// Handles a single keyboard event.
///
/// This function is a placeholder for your actual chord processing logic.
/// It takes the current XKB state and the keyboard event from libinput.
///
/// # Arguments
/// - `state` - A mutable reference to the XKB state.
/// - `event` - The keyboard event to process.
fn keyboard_event_handler(state: &mut xkb::State,
			  chord_state: &mut ChordState,
			  event: &KeyboardEvent) {

    // Note: The keycode from libinput needs a +8 offset to match XKB keycodes.
    let xkb_code: Keycode = (event.key() + 8).into();
    let key_state: KeyState = event.key_state();

    println!(
        "Key Event: code={}, state={:?}",
        xkb_code.raw(),
        key_state,
    );

    match key_state {
        KeyState::Pressed => {
            // Add the key to the keycord state.
            chord_state.add_key(xkb_code);
	    println!("PRESSED!");

            // Check if the pressed key is a non-modifier. If so, it's the
            // trigger for the chord.
            let keysym = state.key_get_one_sym(xkb_code);
            let key_name = xkb::keysym_get_name(keysym);
            println!("  -> Keysym: {} ({})", key_name, keysym.raw());

            if !is_modifier_keysym(keysym) {
                chord_state.send_chord_event(state);
            }
        }
        KeyState::Released => {
            // Remove the key from our state.
            chord_state.remove_key(xkb_code);
	    println!("RELEASED!");
        }
    }
}

/// Main event loop to read key events and process chords.
///
/// This function sets up libinput with a udev backend and enters a loop
/// to listen for keyboard events.
///
/// # Arguments
/// - `state` - The XKB state object.
/// - `keep_running` - An atomic boolean to control the event loop.
fn run_event_loop(mut state: xkb::State,
		  chord_state: &mut ChordState,
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
                keyboard_event_handler(&mut state, chord_state, &kb_event);
            }
        }
    }

    Ok(())
}

fn parse_line(line: &str, line_num: usize) -> Option<Result<(&str, &str),
							    Box<dyn std::error::Error>>>  {
    let line = line.trim();

    // Ignore whitespace and comments.
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    // Split on first colon only.
    let mut parts = line.splitn(2, ':');
    let key: &str = parts.next()?.trim();
    let value: &str = parts.next()?.trim();

    // Validate that both the key press and command exist.
    if key.is_empty() || value.is_empty() {
        return Some(Err(format!(
            "Invalid key-value pair on line {}: '{}'",
            line_num + 1,
            line
        ).into()));
    }

    Some(Ok((key, value)))
}

/// Returns a HashMap of keybindings and commands based on a user's config.
///
/// # Arguments
/// - `content` - The content of a user-defined config file as a single str.
fn parse_config_content<'a>(content: &'a str) -> Result<HashMap<&'a str, &'a str>,
							Box<dyn std::error::Error>> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_num, line)| parse_line(line, line_num))
        .collect()
}

/// Handle a single keypress.
fn handle_keychord(keychord: &str, keybindings: &HashMap<&str, &str>) -> Result<()> {
    let raw_command = keybindings.get(keychord)
	.ok_or_else(|| anyhow!("keychord '{}' not found in keybindings.", keychord))?;

    // Split on whitespace.
    let parts: Vec<&str> = raw_command.split_whitespace().collect();
    let program = &parts[0];
    let args = &parts[1..];

    let mut command = Command::new(program);
    command.args(args);

    println!("Attempting to execute: '{}' with args: {:?}", program, args);

    let mut child = command.spawn()
        .context(format!(
	    "Failed to spawn command '{}' (executable: '{}').",
	    raw_command,
	    program))?;

    let status = child.wait()
        .context(format!(
	    "Failed to wait for command '{}' to complete.",
	    raw_command))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Command '{}' exited with non-zero status: {:?}",
            raw_command, status
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let filename = "/home/nick/git/clef/clef.conf";
    let content = fs::read_to_string(filename)?;
    let keybindings = parse_config_content(&content).unwrap();

    handle_keychord("F5", &keybindings)
	.unwrap_or_else(|e| eprintln!("Error handling keypress: {:?}", e));

    Ok(())
}

/*
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
    let xkb_state = xkb::State::new(&keymap);

    // Run the main event loop.
    // Create the state manager for our chord logic.
    let mut chord_state = ChordState::new();
    
    // Run the main event loop.
    if let Err(e) = run_event_loop(xkb_state, &mut chord_state, keep_running) {
        eprintln!("An error occurred: {:?}", e);
    }

    println!("Daemon has shut down.");

    Ok(())
}
*/
