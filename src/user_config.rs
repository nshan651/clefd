use anyhow::{anyhow, Context, Result};
use log::{error, info};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, RwLock};

pub struct UserConfig {
    pub keybindings: Arc<RwLock<HashMap<String, String>>>,
    config_path: PathBuf,
    file_watcher: Option<Box<dyn Watcher + Send + Sync>>,
}

impl UserConfig {
    /// Initializes a new `UserConfig`.
    pub fn new(config_path: PathBuf) -> Result<Self> {
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
        let mut keybindings_guard = self.keybindings.write().map_err(|e| {
            anyhow!(
                "Failed to acquire a write lock for the \
                  keybindings struct during reload: {}",
                e
            )
        })?;

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
            return None;
        }

        // Split on first colon only.
        let mut parts = line.splitn(2, ':');

        // Remove intermediate whitespace and '+' chars.
        let key: String = parts
            .next()?
            .trim()
            .replace(char::is_whitespace, "")
            .replace('+', " ");

        let value: String = parts.next()?.trim().to_string();

        if key.is_empty() || value.is_empty() {
            return Some(Err(anyhow!(
                "Invalid key-value pair on line {}: '{}'",
                line_num + 1,
                line
            )));
        }

        Some(Ok((key, value)))
    }

    pub fn start_watcher(user_config: &Arc<RwLock<Self>>) -> Result<()> {
        let watch_path = {
            // Acquire a read lock to get the config file path, then clone it.
            let config_guard = user_config
                .read()
                .map_err(|e| anyhow!("Failed to acquire read lock for config path: {}", e))?;
            config_guard.config_path.clone()
        };

        // Set up a channel to receive events from the file watcher.
        let (sender, receiver) = mpsc::channel();

        // Create the file watcher.
        let mut watcher = RecommendedWatcher::new(sender, notify::Config::default())
            .context("Failed to create file watcher")?;

        // Register the watch path.
        watcher
            .watch(&watch_path, RecursiveMode::NonRecursive)
            .context(format!("Failed to watch config file at {:?}", watch_path))?;

        info!("Watching configuration file for changes: {:?}", watch_path);

        // Pass a handler to the watcher thread, allowing it to interact with UserConfig.
        let user_config_handler = Arc::clone(user_config);

        // Spawn a new thread to process file watcher events
        std::thread::spawn(move || {
            UserConfig::watcher_event_handler(receiver, user_config_handler, watch_path);
        });

        // Store the watcher handle within the UserConfig instance to keep it alive.
        let mut config_guard = user_config.write().map_err(|e| {
            anyhow!(
                "Failed to acquire write lock to store \
                  watcher handle: {}",
                e
            )
        })?;

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
        watch_path: PathBuf,
    ) {
        for event_result in receiver {
            match event_result {
                Ok(event) => {
                    if event.kind.is_modify() {
                        info!("Configuration file modified, reloading...");
                        // Capture the reload attempt in a closure.
                        let reload_attempt = (|| {
                            // Acquire a write lock on the UserConfig to call
                            // its internal reload method.
                            let mut config_guard = user_config.write().map_err(|e| {
                                anyhow!(
                                    "Failed to acquire write lock on UserConfig \
                     for reload: {}",
                                    e
                                )
                            })?;

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
