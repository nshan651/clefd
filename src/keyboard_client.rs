//! Provides the main keyboard event loop and command execution logic.
//!
//! This module integrates user configuration, key state management, and input
//! handling to provide a shortcut detection and execution pipeline.
//!
//! The [`KeyboardClient`] maintains the core event loop that listens for
//! keyboard input via libinput, tracks multi-key chord sequences using
//! [`ChordState`], matches completed chords against user-defined keybindings
//! from [`UserConfig`], and executes the corresponding shell commands.
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
    /// Converts the libinput keycode to XKB format, tracks pressed/released
    /// keys, and triggers actions for completed key chords.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use std::sync::{Arc, RwLock};

    /// Write a temporary config file and return its PathBuf.
    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new()
            .expect("Failed to create temporary file.");
        writeln!(file, "{}", content)
            .expect("Failed to write to temporary file.");
        file
    }


    #[test]
    fn new_should_store_user_config_and_chord_state() {
        let temp_file = create_temp_config("# test empty config\n");
        let config_path = temp_file.path().to_path_buf();

        let user_config = UserConfig::new(config_path.clone())
            .expect("User config failed to initialize.");
        let shared_user_config = Arc::new(RwLock::new(user_config));
        let chord_state = crate::chord_state::ChordState::new();

        let kb_client = KeyboardClient::new(Arc::clone(&shared_user_config), chord_state);

        assert!(Arc::ptr_eq(&shared_user_config, &kb_client.user_config),
                "KeyboardClient did not retain the same ptr.");
    }

    #[test]
    fn exec_action_should_execute_success_and_failure() {
        let temp_file = create_temp_config("Control_L+x: /bin/true\nAlt_L+y: /bin/false\n");
        let config_path = temp_file.path().to_path_buf();

        let user_config = crate::user_config::UserConfig::new(config_path.clone())
            .expect("Failed to create a new user config.");
        let shared_user_config = Arc::new(RwLock::new(user_config));
        let chord_state = crate::chord_state::ChordState::new();
        let kb_client = KeyboardClient::new(Arc::clone(&shared_user_config),
                                            chord_state);

        // Act & Assert: success case (/bin/true)
        let res_ok = kb_client.exec_action("Control_L x");
        assert!(
            res_ok.is_ok(),
            "exec_action expected Ok for /bin/true, got: {:?}",
            res_ok
        );

        // Act & Assert: failure case (/bin/false)
        let res_err = kb_client.exec_action("Alt_L y");
        assert!(
            res_err.is_err(),
            "exec_action expected Err for /bin/false, got: {:?}",
            res_err
        );
    }

    #[test]
    fn exec_action_should_return_ok_when_keychord_not_found() {
        let temp_file = create_temp_config("");
        let config_path = temp_file.path().to_path_buf();

        let user_config = crate::user_config::UserConfig::new(config_path.clone())
            .expect("Failed to create a new user config.");
        let shared_user_config = Arc::new(RwLock::new(user_config));
        let chord_state = crate::chord_state::ChordState::new();
        let kb_client = KeyboardClient::new(Arc::clone(&shared_user_config),
                                            chord_state);

        let result = kb_client.exec_action("Control_L x");

        assert!(
            result.is_ok(),
            "exec_action should return Ok(()) when keychord not found, got: {:?}",
            result
        );
    }

    #[test]
    #[ignore]
    fn keyboard_event_handler_should_exec_on_non_modifier_key_press() {
        todo!("Figure out how to mock a KeyboardEvent!");
    }
}
