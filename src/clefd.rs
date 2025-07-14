use anyhow::{anyhow, Context, Result};
use input::{event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait}, Libinput, LibinputInterface};
use signal_hook::{consts::{SIGINT, SIGTERM}, iterator::Signals};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, mpsc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;
use std::process::Command;
use xkbcommon::xkb;
use xkbcommon::xkb::{keysyms, Keysym, Keycode};
use dirs;
use notify::{Watcher, RecursiveMode, RecommendedWatcher};
use log::{info, debug, error};


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
        // OwnedFd automatically closes when dropped.
    }
}

struct UserConfig {
    keybindings: Arc<RwLock<HashMap<String, String>>>,
    config_path: PathBuf,
    file_watcher: Option<Box<dyn Watcher + Send + Sync>>,
}

impl UserConfig {
    /// Initializes a new `UserConfig`.
    fn new(config_path: PathBuf) -> Result<Self> {
	info!("Loading user configuration from {:?}", config_path);
	let keybindings = Self::read_config(&config_path)?;

	Ok(UserConfig {
	    keybindings: Arc::new(RwLock::new(keybindings)),
	    config_path,
	    file_watcher: None,
	})
    }

    /// Read in the config file for the first time.
    fn read_config(config_path: &Path) -> Result<HashMap<String, String>> {
	let content = fs::read_to_string(config_path)
	    .context(format!("Failed to read config at {:?}", config_path))?;
	Self::parse_config(&content)
	    .context(format!("Failed to parse config file at {:?}", config_path))
    }

    /// Re-parse the config file when changes are detected.
    fn reload_config(&mut self) -> Result<()> {
        info!("Reloading keybindings from {:?}", self.config_path);

	let updated_keybindings = Self::read_config(&self.config_path)?;

	// Acquire a write lock on the keybindings to replace its contents.
	let mut keybindings_guard = self.keybindings.write()
	    .map_err(|e| anyhow!("Failed to acquire a write lock for the \
				  keybindings struct during reload: {}", e))?;

	*keybindings_guard = updated_keybindings;
	Ok(())
    }
	
    /// Parse the config into a mapping of keybindings and commands to execute.
    fn parse_config(content: &str) -> Result<HashMap<String, String>> {
	content
            .lines()
            .enumerate()
            .filter_map(|(line_num, line)| Self::parse_line(line, line_num))
            .collect::<Result<HashMap<String, String>>>()
    }

    fn parse_line(line: &str, line_num: usize) -> Option<Result<(String, String)>> {
	let line = line.trim();

	// Ignore whitespace and comments.
	if line.is_empty() || line.starts_with('#') {
            return None
	}

	// Split on first colon only.
	let mut parts = line.splitn(2, ':');

	// Remove intermediate whitespace and '+' chars.
	let key: String = parts.next()?
	    .trim()
	    .replace(char::is_whitespace, "")
	    .replace('+', " ");

	let value: String = parts.next()?
	    .trim()
	    .to_string();


	if key.is_empty() || value.is_empty() {
            return Some(Err(anyhow!(
		"Invalid key-value pair on line {}: '{}'",
		line_num + 1,
		line)));
	}

	Some(Ok((key, value)))
    }

    fn start_watcher(user_config: &Arc<RwLock<Self>>) -> Result<()> {
	let watch_path = {
            // Acquire a read lock to get the config file path, then clone it.
            let config_guard = user_config.read()
                .map_err(|e| anyhow!(
		    "Failed to acquire read lock for config path: {}", e))?;
            config_guard.config_path.clone()
        };

        // Set up a channel to receive events from the file watcher.
        let (sender, receiver) = mpsc::channel();

        // Create the file watcher.
        let mut watcher = RecommendedWatcher::new(sender,
						  notify::Config::default())
            .context("Failed to create file watcher")?;

        // Register the watch path.
        watcher.watch(&watch_path, RecursiveMode::NonRecursive)
            .context(format!("Failed to watch config file at {:?}", watch_path))?;

        info!("Watching configuration file for changes: {:?}", watch_path);

        // Pass a handler to the watcher thread, allowing it to interact with UserConfig.
        let user_config_handler = Arc::clone(user_config);

	// Spawn a new thread to process file watcher events
        std::thread::spawn(move || {
	    UserConfig::watcher_event_handler(receiver,
					      user_config_handler,
					      watch_path);
	});


	// Store the watcher handle within the UserConfig instance to keep it alive.
        let mut config_guard = user_config.write()
            .map_err(|e| anyhow!("Failed to acquire write lock to store \
				  watcher handle: {}", e))?;

        config_guard.file_watcher = Some(Box::new(watcher));
	Ok(())
    }

    /// Handles events on the watcher thread.
    ///
    /// We want to filter on file save, and event kind `is_modify()` should be
    /// sufficient for this. Note that different OSes send different variations
    /// of EventKind::Modify.
    fn watcher_event_handler(
        receiver: mpsc::Receiver<Result<notify::Event, notify::Error>>,
	user_config: Arc<RwLock<UserConfig>>,
	watch_path: PathBuf) {

        for event_result in receiver {
            match event_result {
                Ok(event) => {
                    if event.kind.is_modify() {
                        info!("Configuration file modified, reloading...");
                        // Capture the reload attempt in a closure. 
                        let reload_attempt = (|| {
                            // Acquire a write lock on the UserConfig to call
			    // its internal reload method.
                            let mut config_guard = user_config.write()
                                .map_err(|e| anyhow!(
				    "Failed to acquire write lock on UserConfig \
				     for reload: {}", e))?;

                            config_guard.reload_config()
                        })();

                        if let Err(e) = reload_attempt {
                            error!("Failed to reload keybindings: {}", e);
                        } else {
                            info!("Keybindings reloaded successfully from {:?}", watch_path);
                        }
                    }
                }
                Err(e) => error!("Configuration watch error: {:?}", e),
            }
        }
        info!("Configuration watcher thread exiting.");
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
            return
        }
        self.pressed_keys.insert(keycode);
    }

    /// Removes a keycode from the set of currently pressed keys.
    fn remove_key(&mut self, keycode: xkb::Keycode) {
        self.pressed_keys.remove(&keycode);
    }

    /// Constructs a chord string and sends it if it's valid.
    ///
    /// A valid chord consists of one or more modifiers and EXACTLY ONE non-modifier
    /// key. The resulting string is canonical: modifiers are sorted alphabetically,
    /// followed by the single non-modifier key, all space-separated.
    fn get_keychord(&self, xkb_state: &xkb::State) -> Option<String> {
        let mut modifier_names = Vec::new();
        let mut key_names = Vec::new();

        // Separate all currently pressed keys into modifiers and regular keys.
        for &keycode in self.pressed_keys.iter() {
            let keysym = xkb_state.key_get_one_sym(keycode);
            let name = xkb::keysym_get_name(keysym);

            if Self::is_modifier_keysym(keysym) {
                modifier_names.push(name);
            } else {
                key_names.push(name);
            }
        }

	// A valid key sequence always ends with exactly one non-modifier key.
        if key_names.len() != 1 {
            return None
        }

        // Sort modifiers alphabetically for a canonical representation.
        modifier_names.sort_unstable();

        // Combine the sorted modifiers and the single key name.
        let mut chord_parts = modifier_names;
        chord_parts.extend(key_names);
        let keychord = chord_parts.join(" ");

	Some(keychord)
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
		 keysyms::KEY_Caps_Lock | keysyms::KEY_Shift_Lock
	)
    }
}

/// Define a KeyboardClient, which includes the user's config data and a global
/// chord state.
struct KeyboardClient {
    // user_config: UserConfig,
    user_config: Arc<RwLock<UserConfig>>,
    chord_state: ChordState,
}

impl KeyboardClient {
    fn new(user_config: Arc<RwLock<UserConfig>>, chord_state: ChordState) -> Self {
	Self {
	    user_config,
	    chord_state,
	}
    }

    /// Handles a single keyboard event.
    ///
    /// This function is a placeholder for your actual chord processing logic.
    /// It takes the current XKB state and the keyboard event from libinput.
    ///
    /// # Arguments
    /// - `state` - A mutable reference to the XKB state.
    /// - `event` - The keyboard event to process.
    fn keyboard_event_handler(&mut self,
			      state: &mut xkb::State,
			      event: &KeyboardEvent) -> Result<()> {

	// The keycode from libinput needs a +8 offset to match XKB keycodes.
	let xkb_code: Keycode = (event.key() + 8).into();
	let key_state: KeyState = event.key_state();
	let keysym = state.key_get_one_sym(xkb_code);
	let key_name = xkb::keysym_get_name(keysym);

	match key_state {
            KeyState::Pressed => {
		debug!("key event: {:?}, state={:?}, name={}",
		       xkb_code,
		       key_state,
		       key_name,
		);
		self.chord_state.add_key(xkb_code);

		// A non-modifier signals the end of a key sequence.
		if !ChordState::is_modifier_keysym(keysym) {
                    if let Some(keychord) = self.chord_state.get_keychord(state) {
			self.exec_action(&keychord)?;
		    }
		}
            }
            KeyState::Released => {
		debug!("key event: {:?}, state={:?}, name={}",
		       xkb_code,
		       key_state,
		       key_name,
		);
		self.chord_state.remove_key(xkb_code);
            }
	}

	Ok(())
    }

    /// Main event loop to read key events and process chords.
    ///
    /// This function sets up libinput with a udev backend and enters a loop
    /// to listen for keyboard events.
    ///
    /// # Arguments
    /// - `state` - The XKB state object.
    /// - `keep_running` - An atomic boolean to control the event loop.
    fn keyboard_event_listener(&mut self,
		      mut state: xkb::State,
		      keep_running: Arc<AtomicBool>) -> Result<()> {
	// Create a libinput context with a udev backend.
	// This allows libinput to discover and manage input devices automatically.
	let mut libinput = Libinput::new_with_udev(Interface);

	// Assign the default seat "seat0" to the context. A "seat" represents
	// a collection of input devices used by a single user.
	libinput.udev_assign_seat("seat0")
            .map_err(|_| anyhow!("Failed to assign seat 'seat0'"))?;

	info!("Event loop started. Waiting for keyboard input...");

	// Process incoming libinput events.
	while keep_running.load(Ordering::SeqCst) {
            // Dispatch events from libinput.
            libinput.dispatch().context("Failed to dispatch libinput events")?;

            // Iterate over all available events from libinput.
            for event in &mut libinput {
		// We only care about keyboard events.
		if let input::Event::Keyboard(kb_event) = event {
                    self.keyboard_event_handler(&mut state, &kb_event)
			.unwrap_or_else(|e| eprintln!("Failed to handle event: {}", e));
		}
            }
	}

	Ok(())
    }

    /// Execute an action based on the key press.
    fn exec_action(&self, keychord: &str) -> Result<()> {
	// Acquire a lock on the UserConfig.
	let user_config_guard = self.user_config.read()
            .map_err(|e| anyhow!(
		"Failed to acquire read lock on user config: {}", e))?;

	// Now acquire a lock on the keybindings themselves.
	let keybindings_guard = user_config_guard.keybindings.read()
	    .map_err(|e| anyhow!("Failed to acquire read lock on keybindings map: {}", e))?;

	let raw_command = match keybindings_guard.get(keychord) {
	    Some(cmd) => cmd,
	    None => return Ok(()),
	};

	// Split on whitespace.
	let parts: Vec<&str> = raw_command.split_whitespace().collect();
	let program = &parts[0];
	let args = &parts[1..];

	let mut command = Command::new(program);
	command.args(args);

	debug!("Executing '{}' with args: {:?}", program, args);

	let mut child = command.spawn()
            .context(format!(
		"Failed to spawn command '{}'",
		raw_command))?;

	let status = child.wait()
            .context(format!(
		"Failed to wait for command '{}' to complete.",
		raw_command))?;

	if status.success() {
            Ok(())
	} else {
            Err(anyhow!(
		"Command '{}' exited with non-zero status: {:?}",
		raw_command,
		status,
            ))
	}
    }
}

/// Main entry point for the application.
fn main() -> Result<()> {
    // Init info logging.
    env_logger::Builder::from_env(
	env_logger::Env::default().default_filter_or("info")).init();

    // Set up an atomic boolean to control the main loop.
    // This allows us to gracefully shut down from a signal handler.
    let keep_running = Arc::new(AtomicBool::new(true));
    let keep_running_handler = keep_running.clone();

    // Register a signal handler for SIGINT and SIGTERM to ensure graceful shutdowns.
    let mut signals = Signals::new(&[SIGINT, SIGTERM])
        .context("Failed to register signal handlers")?;

    // Spawn a thread to listen for signals.
    std::thread::spawn(move || {
        for _ in signals.forever() {
            info!("\nSignal received, shutting down daemon...");
            keep_running_handler.store(false, Ordering::SeqCst);
        }
    });

    info!("Daemon started...");

    // Initialize the XKB context.
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

    // Create a keymap from the system's current keyboard configuration.
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",   // rules
        "",   // model
        "",   // layout
        "",   // variant
        None, // options
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ).ok_or_else(|| anyhow!("Failed to create XKB keymap"))?;

    // Create an XKB state object from the keymap.
    let xkb_state = xkb::State::new(&keymap);

    // Parse config from XDG_CONFIG.
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine user config directory"))?;
    let config_path = config_dir.join("clef").join("clefrc");

    let user_config = UserConfig::new(config_path)?;
    let shared_user_config = Arc::new(RwLock::new(user_config));
    let chord_state = ChordState::new();

    UserConfig::start_watcher(&shared_user_config)?;

    let mut kb_client = KeyboardClient::new(
	shared_user_config,
	chord_state);
    
    // Run the main event loop.
    if let Err(e) = kb_client.keyboard_event_listener(xkb_state, keep_running) {
        eprintln!("An error occurred: {:?}", e);
    }

    info!("Daemon stopped.");

    Ok(())
}
