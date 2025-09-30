//! Provides loading and live reloading of user-defined keybinding configuration.
//!
//! The [`UserConfig`] struct provides a thread-safe mapping from key sequences
//! to commands, stored in an [`Arc<RwLock<_>>`] for safe concurrent access.
//! Configurations are loaded from a simple, human-editable file format, and
//! the module automatically watches the file for changes, reloading keybindings
//! on the fly. Parsing errors and I/O issues are surfaced using [`anyhow`] and
//! logged via [`log`] to help users diagnose problems quickly.
use crate::keybindings::Keybindings;
use anyhow::{anyhow, Context, Result};
use log::{error, info, debug};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::collections::HashMap;

pub struct UserConfig;

impl UserConfig {
    /// Read in the config file for the first time.
    fn read_config(config_path: &Path) -> Result<HashMap<String, String>> {
        let content = fs::read_to_string(config_path)
            .context(format!("Failed to read config at {:?}", config_path))?;

        // Parse the config into a mapping of keybindings and commands to execute.
        content
            .lines()
            .enumerate()
            .filter_map(|(line_num, line)| Self::parse_line(line, line_num))
            .collect::<Result<HashMap<String, String>>>()
    }

    /// Re-parse the config file when changes are detected.
    pub fn reload_config(
        config_path: &Path,
        keybindings: &Keybindings
    ) -> Result<()> {
        info!("Reloading keybindings from {:?}", config_path);

        let updated_keybindings = Self::read_config(config_path)?;

        // Acquire a write lock on the keybindings to replace its contents.
        let mut guard = keybindings.write()
            .expect("Failed to acquire write lock to update keybindings.");

        *guard = updated_keybindings;
        Ok(())
    }

    fn parse_line(line: &str, line_num: usize) -> Option<Result<(String, String)>> {
        let line = line.trim();

        // Ignore whitespace and comments.
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        // Split on first colon only.
        let mut parts = line.splitn(2, ':');
        let key_part = parts.next();
        let value_part = parts.next();

        if key_part.is_none() || value_part.is_none() {
            return Some(Err(anyhow!(
                "Invalid key-value pair on line {}: '{}'",
                line_num + 1,
                line
            )));
        }

        // Remove intermediate whitespace and '+' chars.
        let key: String = key_part
            .unwrap()
            .trim()
            .replace(char::is_whitespace, "")
            .replace('+', " ");

        let value: String = value_part.unwrap().trim().to_string();

        if key.is_empty() || value.is_empty() {
            return Some(Err(anyhow!(
                "Invalid key-value pair on line {}: '{}'",
                line_num + 1,
                line
            )));
        }

        Some(Ok((key, value)))
    }

    pub fn start_watcher(
        config_path: PathBuf,
        keybindings: Keybindings
    ) -> Result<RecommendedWatcher> {
        // Initial reading of keybindings.
        Self::reload_config(&config_path, &keybindings)
            .expect("Could not reload config.");

        // Set up a channel to receive events from the file watcher.
        let (tx, rx) = mpsc::channel();

        // Create the file watcher.
        let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())
            .context("Failed to create file watcher")?;

        // Register the watch path.
        watcher
            .watch(config_path.parent().unwrap(), RecursiveMode::NonRecursive)
            .context(format!("Failed to watch config file at {:?}", config_path))?;

        info!("Watching configuration file for changes: {:?}", config_path);

        std::thread::spawn(move || {
            Self::watcher_event_handler(rx, config_path, keybindings);
        });

        Ok(watcher)
    }

    /// Handles events on the watcher thread.
    ///
    /// We want to filter on file save, and event kind `is_modify()` should be
    /// sufficient for this. Note that different OSes send different variations
    /// of EventKind::Modify.
    fn watcher_event_handler(
        receiver: mpsc::Receiver<Result<notify::Event, notify::Error>>,
        config_path: PathBuf,
        keybindings: Keybindings,
    ) {
        let mut last_reload = Instant::now() - Duration::from_secs(1);

        for event_result in receiver {
            let event = match event_result {
                Ok(ev) => ev,
                Err(e) => {
                    error!("Configuration watch error: {:?}", e);
                    continue;
                }
            };

            // Only handle modify events.
            if !event.kind.is_modify() {
                continue;
            }

            let now = Instant::now();

            // Debounce check, only reload every 50 ms.
            if now.duration_since(last_reload) <= Duration::from_millis(50) {
                debug!("Skipping rapid successive modify event");
                continue;
            }

            // Allow the editor to finish writing.
            std::thread::sleep(Duration::from_millis(20));

            info!("Configuration file modified, reloading...");
            if let Err(e) = Self::reload_config(&config_path, &keybindings) {
                error!("Failed to reload keybindings: {}", e);
            }
            else {
                info!("Keybindings reloaded successfully from {:?}", config_path);
            }

            last_reload = now;

        }
        info!("Configuration watcher thread exiting.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    /// Helper to create a temporary config file.
    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new()
            .expect("Failed to create temporary file.");
        writeln!(file, "{}", content)
            .expect("Failed to write to temporary file.");
        file
    }

    #[test]
    fn read_config_should_succeed_with_valid_file() {
        let temp_file = create_temp_config("test_key: test_command");
        let config_path = temp_file.path().to_path_buf();
        let keybindings = UserConfig::read_config(&config_path)
            .expect("Reading valid config file should succeed");

        let mut expected = HashMap::new();
        expected.insert("test_key".to_string(), "test_command".to_string());
        assert_eq!(keybindings, expected);
    }

    #[test]
    fn read_config_should_fail_for_nonexistent_file() {
        let non_existent_path = PathBuf::from("non_existent_config.txt");
        let result = UserConfig::read_config(&non_existent_path);
        assert!(result.is_err(), "Reading non-existent file should fail");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to read config at"));
    }

    #[test]
    fn read_config_should_fail_with_invalid_content() {
        let temp_file = create_temp_config("invalid line content");
        let config_file_path = temp_file.path().to_path_buf();
        let result = UserConfig::read_config(&config_file_path);
        assert!(
            result.is_err(),
            "Reading invalid config content should fail"
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Invalid key-value pair on line"),
            "Unwrapped error should contain specific string",
        );
    }

    #[test]
    fn reload_should_update_keybindings_when_file_changes() {
        let temp_file = create_temp_config("key1: command1\n");
        let config_path = temp_file.path().to_path_buf();
        let keybindings: Keybindings = Arc::new(RwLock::new(HashMap::new()));

        // Load the config.
        UserConfig::reload_config(&config_path, &keybindings)
            .expect("Reloading config should succeed");

        // Update the config file.
        fs::write(&config_path, "key2: command2\n")
            .expect("Failed to write updated config file");

        // Reload config.
        UserConfig::reload_config(&config_path, &keybindings)
            .expect("Reloading config should succeed");

        // Verify updated content.
        let keybindings_reloaded = keybindings.read().unwrap();
        assert_eq!(
            keybindings_reloaded.get("key2"),
            Some(&"command2".to_string())
        );
        assert_eq!(keybindings_reloaded.get("key1"), None);
    }
}
