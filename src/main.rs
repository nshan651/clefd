use anyhow::{anyhow, Context, Result};
use clap::Parser;
use clefd::{chord_state::ChordState, keybindings::Keybindings};
use clefd::keyboard_client::KeyboardClient;
use clefd::user_config::UserConfig;
use log::info;
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::Signals,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use xkbcommon::xkb;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::process::Child;
use std::collections::HashMap;

#[derive(Parser, Debug)]
#[command(version, about = "A keyboard shortcut manager daemon.", long_about = None)]
struct Args {
    // Empty implementation.
}

fn run(keep_running: Arc<AtomicBool>,
       ready_tx: Option<Sender<()>>
) -> Result<()> {
    // Init info logging.
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"))
        .is_test(cfg!(test)) // Disable logs during testing.
        .try_init();

    // Set up an atomic boolean to control the main loop.
    // This allows us to gracefully shut down from a signal handler.
    let keep_running_handler = keep_running.clone();

    // Register a signal handler for SIGINT and SIGTERM to ensure graceful shutdowns.
    let mut signals = Signals::new(&[SIGINT, SIGTERM])
        .context("Failed to register signal handlers.")?;

    // Spawn a thread to listen for signals.
    std::thread::spawn(move || {
        for sig in signals.forever() {
            info!("\nReceived signal {:?}, shutting down daemon...", sig);
            keep_running_handler.store(false, Ordering::SeqCst);
        }
    });

    // Setup reaper thread.
    let (tx, rx) = channel::<Child>();

    thread::spawn(move || {
        for mut child in rx {
            let _ = child.wait();
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
    )
    .ok_or_else(|| anyhow!("Failed to create XKB keymap."))?;

    // Create an XKB state object from the keymap.
    let xkb_state = xkb::State::new(&keymap);

    // Parse config from XDG_CONFIG.
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("Could not determine user config directory."))?;
    let config_path = config_dir.join("clefd").join("clefdrc");

    let chord_state = ChordState::new();

    let keybindings: Keybindings = Arc::new(RwLock::new(HashMap::new()));

    // Start user config file watcher.
    let _watcher = UserConfig::start_watcher(config_path, keybindings.clone())
        .expect("Failed to start watcher thread.");

    let mut kb_client = KeyboardClient::new(
        keybindings.clone(),
        chord_state,
        tx,
    );

    // Notify tests that setup is complete via handshake.
    if let Some(tx) = ready_tx {
        let _ = tx.send(()); // Ignore if receiver already dropped.
    }

    // Run the main event loop.
    if let Err(e) = kb_client.keyboard_event_listener(xkb_state, keep_running) {
        eprintln!("An error occurred: {:?}", e);
    }

    info!("Daemon stopped.");

    Ok(())
}

/// Main entry point for the application.
fn main() -> Result<()> {
    Args::parse();
    let keep_running = Arc::new(AtomicBool::new(true));
    run(keep_running, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, sync::atomic::AtomicBool, sync::atomic::Ordering, thread, time::Duration};
    use std::sync::mpsc;

    #[test]
    fn run_should_start_and_stop() {
        let keep_running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();
        let kr_clone = keep_running.clone();

        let handle = thread::spawn(move || {
            run(kr_clone, Some(tx))
                .expect("Daemon should run without setup errors.");
        });

        // Wait until run() signals it is ready.
        rx.recv_timeout(Duration::from_secs(5))
            .expect("Did not receive ready signal within 5s.");

        keep_running.store(false, Ordering::SeqCst);

        handle.join().expect("Thread should join.");

        assert!(!keep_running.load(Ordering::SeqCst));
    }

    #[test]
    fn main_should_start_and_stop_on_sigint() {
        let handle = thread::spawn(|| super::main());

        // Sleep until signal thread spawns.
        thread::sleep(Duration::from_millis(100));

        // Send SIGINT to this process via signal-hook's helper.
        signal_hook::low_level::raise(SIGINT).expect("Failed to raise SIGINT.");

        // Wait for the completion of main().
        let result = handle.join().expect("Main thread should exit cleanly.");

        assert!(result.is_ok(),
                "Main should return Ok(()) after receiving SIGINT.");
    }
}
