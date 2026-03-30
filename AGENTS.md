# Agent Guidelines for Clefd

## Project Overview

Clefd is a universal keybindings manager daemon written in Rust (edition 2021). It reads keyboard events via libinput and executes commands based on user-defined keybindings.

## Build/Test/Lint Commands

### Standard Cargo Commands
```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run tests
cargo test

# Run a specific test
cargo test test_name

# Run tests with output visible
cargo test -- --nocapture

# Check code (fast compile)
cargo check

# Run clippy lints
cargo clippy

# Format code
cargo fmt

# Generate documentation
cargo doc
```

### Makefile Targets
```bash
make all      # build + test + lint + doc
make build    # cargo build --release
make test     # cargo test
make lint     # cargo check + cargo clippy
make doc      # cargo doc
make cov      # cargo tarpaulin (requires cargo-tarpaulin)
make bench    # cargo bench
make format   # cargo fmt
make clean    # rm -rf ./target
make install  # install binary and systemd service
```

### Development Environment
Use `guix shell -m guix.scm` for an isolated development environment with all dependencies (rust, cargo-tarpaulin, pkg-config, libinput, eudev, libxkbcommon).

## Code Style Guidelines

### Formatting
- Use `cargo fmt` to format code (enforced by CI)
- 4-space indentation (Rust standard)
- No trailing whitespace
- One blank line between function definitions

### Naming Conventions
- **Modules:** `snake_case` (e.g., `chord_state.rs`, `keyboard_client.rs`)
- **Functions:** `snake_case` (e.g., `keyboard_event_handler`, `reload_config`)
- **Structs/Types:** `PascalCase` (e.g., `ChordState`, `KeyboardClient`, `UserConfig`)
- **Constants:** `SCREAMING_SNAKE_CASE` (e.g., `MAX_PRESSED_KEYS`)
- **Type aliases:** `PascalCase` (e.g., `pub type Keybindings = Arc<RwLock<HashMap<String, String>>>`)

### Imports
- Group imports: std library → external crates → local modules
- Use absolute paths for local modules: `crate::module_name`
- One import per line
- Avoid `use *` imports except for test modules

### Error Handling
- Use `anyhow::Result<T>` for application-level error handling
- Use `anyhow::Context` for adding context to errors: `.context("message")`
- Use `.map_err(|e| anyhow!("formatted {}", e))` for custom error messages
- Return `Ok(())` for operations that should not fail silently (e.g., keychord not found)

### Thread Safety
- Use `Arc<RwLock<T>>` for shared mutable state across threads
- Use `Arc<AtomicBool>` for atomic flags
- Use `mpsc::channel` for thread communication
- Always clone Arcs when sharing: `keybindings.clone()`

### Documentation
- Module-level documentation with `//!` doc comments
- Function documentation with `///` for public APIs
- Document all `pub fn` functions with: Purpose, Arguments, Returns
- Example format:
```rust
/// Handles a single keyboard event.
///
/// Converts the libinput keycode to XKB format, tracks pressed/released
/// keys, and triggers actions for completed key chords.
///
/// # Arguments
/// * `state` - A mutable reference to the XKB state.
/// * `event` - The keyboard event to process.
fn keyboard_event_handler(&mut self, state: &mut xkb::State, event: &KeyboardEvent) -> Result<()> {
```

### Testing Conventions
- Tests live in `#[cfg(test)]` modules within each source file
- Use helper functions for common test setup (e.g., `create_temp_config`)
- Use `tempfile::NamedTempFile` for test file creation
- Tests should be deterministic and self-contained
- Use `#[ignore]` for tests that require specific environment (document why)
- Use descriptive test names: `fn test_name_should_expected_behavior()`

### Logging
- Use `log` crate with appropriate levels: `debug!`, `info!`, `warn!`, `error!`
- Log at startup, shutdown, configuration changes, and errors
- Use `env_logger::Builder` to configure logging filter
- Disable logs during testing with `.is_test(cfg!(test))`

### Module Structure
```
src/
├── main.rs      # Entry point, signal handling, CLI args
├── lib.rs       # Module declarations
├── chord_state.rs      # Key chord detection and state
├── keybindings.rs      # Type alias for keybindings map
├── keyboard_client.rs  # Main event loop, command execution
└── user_config.rs      # Config file parsing and hot-reloading
```

### Dependencies (External Crates)
- `input` (0.8.3): libinput wrapper for keyboard events
- `udev` (0.7.0): libudev wrapper for device enumeration
- `xkbcommon` (0.7.0): XKB keycode/sym handling
- `anyhow` (1.0.75): Flexible error handling
- `clap` (4.5.41): CLI argument parsing
- `log`/`env_logger`: Logging
- `notify` (8.1.0): File watching for config hot-reload
- `signal-hook` (0.3.17): Signal handling
- `nix` (0.30.1): Unix system calls (poll)
- `tempfile` (3.20.0): Temporary files for tests

## Common Patterns

### Creating Keybindings
```rust
let keybindings: Keybindings = Arc::new(RwLock::new(HashMap::new()));
```

### Spawning a Thread with Shared State
```rust
std::thread::spawn(move || {
    // state is moved into the closure
});
```

### Lock Acquisition
```rust
let guard = self.keybindings.read().expect("Failed to acquire read lock on keybindings map.");
// use guard...
drop(guard); // explicit drop or scope end
```

### Config File Watching
```rust
let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())
    .context("Failed to create file watcher")?;
watcher.watch(config_dir, RecursiveMode::NonRecursive)?;
```
