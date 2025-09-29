// command_runner.rs
use std::sync::mpsc::channel;
use anyhow::{anyhow, Context, Result};
use log::debug;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::thread;

pub struct CommandRunner {
    keybindings: Arc<RwLock<HashMap<String, String>>>,
    tx: Sender<Child>,
}

impl CommandRunner {
    /// Initializes the command runner, including the child reaper thread.
    pub fn new(keybindings: Arc<RwLock<HashMap<String, String>>>) -> Self {
        let (tx, rx) = channel::<Child>();

        thread::spawn(move || {
            for mut child in rx {
                let _ = child.wait();
            }
        });

        CommandRunner {
            keybindings,
            tx,
        }
    }

    /// Execute an action based on the key press.
    pub fn exec_action(&self, keychord: &str) -> Result<()> {
        // Acquire a lock on the keybindings.
        let keybindings_guard = self.keybindings
            .read()
            .map_err(|e| anyhow!("Failed to acquire read lock on keybindings map: {}", e))?;

        let raw_command: &String = match keybindings_guard.get(keychord) {
            Some(cmd) => cmd,
            _ => return Ok(()),
        };

        // Split on whitespace.
        let parts: Vec<&str> = raw_command.split_whitespace().collect();
        let program = &parts[0];
        let args = &parts[1..];

        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
            
        debug!("Executing '{}' with args: {:?}", program, args);

        let child = command
            .spawn()
            .context(format!("Failed to spawn command '{}'", raw_command))?;

        debug!("Spawned process '{}' (PID {})", raw_command, child.id());

        // Send the child to the reaper.
        self.tx.send(child)
            .map_err(|e| anyhow!("Failed to send child process to reaper: {}", e))?;

        Ok(())
    }
}
