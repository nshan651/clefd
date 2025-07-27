use crate::chord_state::ChordState;
use crate::user_config::UserConfig;
use anyhow::{anyhow, Context, Result};
use input::{
    event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
    Libinput, LibinputInterface,
};
use log::{debug, info};
use std::fs::OpenOptions;
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use xkbcommon::xkb;
use xkbcommon::xkb::Keycode;

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

/// Define a KeyboardClient, which includes the user's config data and a global
/// chord state.
pub struct KeyboardClient {
    user_config: Arc<RwLock<UserConfig>>,
    chord_state: ChordState,
}

impl KeyboardClient {
    pub fn new(user_config: Arc<RwLock<UserConfig>>, chord_state: ChordState) -> Self {
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
    fn keyboard_event_handler(
        &mut self,
        state: &mut xkb::State,
        event: &KeyboardEvent,
    ) -> Result<()> {
        // The keycode from libinput needs a +8 offset to match XKB keycodes.
        let xkb_code: Keycode = (event.key() + 8).into();
        let key_state: KeyState = event.key_state();
        let keysym = state.key_get_one_sym(xkb_code);
        let key_name = xkb::keysym_get_name(keysym);

        match key_state {
            KeyState::Pressed => {
                debug!(
                    "key event: {:?}, state={:?}, name={}",
                    xkb_code, key_state, key_name,
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
                debug!(
                    "key event: {:?}, state={:?}, name={}",
                    xkb_code, key_state, key_name,
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
    pub fn keyboard_event_listener(
        &mut self,
        mut state: xkb::State,
        keep_running: Arc<AtomicBool>,
    ) -> Result<()> {
        // Create a libinput context with a udev backend.
        // This allows libinput to discover and manage input devices automatically.
        let mut libinput = Libinput::new_with_udev(Interface);

        // Assign the default seat "seat0" to the context. A "seat" represents
        // a collection of input devices used by a single user.
        libinput
            .udev_assign_seat("seat0")
            .map_err(|_| anyhow!("Failed to assign seat 'seat0'"))?;

        info!("Event loop started. Waiting for keyboard input...");

        // Process incoming libinput events.
        while keep_running.load(Ordering::SeqCst) {
            // Dispatch events from libinput.
            libinput
                .dispatch()
                .context("Failed to dispatch libinput events")?;

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
        let user_config_guard = self
            .user_config
            .read()
            .map_err(|e| anyhow!("Failed to acquire read lock on user config: {}", e))?;

        // Now acquire a lock on the keybindings themselves.
        let keybindings_guard = user_config_guard
            .keybindings
            .read()
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

        let mut child = command
            .spawn()
            .context(format!("Failed to spawn command '{}'", raw_command))?;

        let status = child.wait().context(format!(
            "Failed to wait for command '{}' to complete.",
            raw_command
        ))?;

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
