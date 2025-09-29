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
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub struct UserConfig;

impl UserConfig {
    /// Read in the config file for the first time.
    fn read_config(
        config_path: &Path
    ) -> Result<HashMap<String, String>> {
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
    fn reload_config(
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
            match event_result {
                Ok(event) => {
                    if event.kind.is_modify() {
                        let now = Instant::now();

                        // Only reload if at least 50ms have passed since last reload
                        if now.duration_since(last_reload) > Duration::from_millis(50) {
                            // Tiny sleep to let editor finish writing
                            std::thread::sleep(Duration::from_millis(20));

                            info!("Configuration file modified, reloading...");
                            if let Err(e) = Self::reload_config(&config_path, &keybindings) {
                                error!("Failed to reload keybindings: {}", e);
                            } else {
                                info!("Keybindings reloaded successfully from {:?}", config_path);
                            }

                            last_reload = now;
                        }
                    }
                }
                Err(e) => error!("Configuration watch error: {:?}", e),
            }
        }

        info!("Configuration watcher thread exiting.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Helper to create a temporary config file.
    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new()
            .expect("Failed to create temporary file.");
        writeln!(file, "{}", content)
            .expect("Failed to write to temporary file.");
        file
    }

    #[test]
    fn parse_config_should_succeed_with_valid_content() {
        let config_content = r#"
            # This is a comment
            Super_L + w : firefox
            Control_L+Shift_L + n : newsboat -r
            Super_L  + n :nvim -o3 +5
        "#;
        let keybindings =
            UserConfig::parse_config(config_content).expect("Parsing valid config should succeed.");

        let expected = HashMap::from([
            ("Super_L w".to_string(), "firefox".to_string()),
            ("Control_L Shift_L n".to_string(), "newsboat -r".to_string()),
            ("Super_L n".to_string(), "nvim -o3 +5".to_string()),
        ]);

        assert_eq!(keybindings, expected);
    }

    #[test]
    fn parse_config_should_return_empty_map_for_empty_input() {
        let config_content = "";
        let keybindings =
            UserConfig::parse_config(config_content).expect("Parsing empty config should succeed");
        assert!(keybindings.is_empty());
    }

    #[test]
    fn parse_config_should_ignore_comments_and_whitespace() {
        let config_content = r#"
            # Only comments and whitespaces!!!

            # Another comment.

        "#;
        let keybindings = UserConfig::parse_config(config_content).expect(
            "Parsing an empty config works, but results in empty \
                     keybindings structure.",
        );

        assert!(keybindings.is_empty());
    }

    #[test]
    fn parse_config_should_fail_without_colon_separator() {
        let config_content = "invalid line";
        let result = UserConfig::parse_config(config_content);
        assert!(result.is_err(), "Parsing invalid line should fail");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid key-value pair on line 1"));
    }

    #[test]
    fn parse_config_should_fail_when_key_is_empty() {
        let config_content = ":command";
        let result = UserConfig::parse_config(config_content);
        assert!(result.is_err(), "Parsing line with empty key should fail");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid key-value pair on line 1"));
    }

    #[test]
    fn parse_config_should_fail_when_value_is_empty() {
        let config_content = "key:";
        let result = UserConfig::parse_config(config_content);
        assert!(result.is_err(), "Parsing line with empty value should fail");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid key-value pair on line 1"));
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
        let config_path = temp_file.path().to_path_buf();
        let result = UserConfig::read_config(&config_path);
        assert!(
            result.is_err(),
            "Reading invalid config content should fail"
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to parse config file at"));
    }

    #[test]
    fn new_user_should_initialize_correctly_with_valid_config() {
        let temp_file = create_temp_config("hotkey: execute_something");
        let config_path = temp_file.path().to_path_buf();
        let user_config = UserConfig::new(config_path.clone())
            .expect("UserConfig::new should succeed with a valid config file");

        assert_eq!(user_config.config_path, config_path);
        let keybindings_guard = user_config.keybindings.read().unwrap();
        assert_eq!(
            keybindings_guard.get("hotkey"),
            Some(&"execute_something".to_string())
        );
    }

    #[test]
    fn reload_should_update_keybindings_when_file_changes() {
        let temp_file = create_temp_config("key1: command1\n");
        let config_file_path = temp_file.path().to_path_buf();

        let mut user_config = UserConfig::new(config_file_path.clone())
            .expect("Failed to create UserConfig for reload test");

        // Verify initial content.
        let keybindings_initial = user_config.keybindings.read().unwrap();
        assert_eq!(
            keybindings_initial.get("key1"),
            Some(&"command1".to_string())
        );

        // Release the read lock.
        drop(keybindings_initial);

        // Update the config file.
        fs::write(&config_file_path, "key2: command2\n")
            .expect("Failed to write updated config file");

        // Reload the config.
        user_config
            .reload_config()
            .expect("Reloading config should succeed");

        // Verify updated content.
        let keybindings_reloaded = user_config.keybindings.read().unwrap();
        assert_eq!(
            keybindings_reloaded.get("key2"),
            Some(&"command2".to_string())
        );
        assert_eq!(keybindings_reloaded.get("key1"), None);
    }
}
