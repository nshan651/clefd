//! Provides keyboard chord detection and state management for multi-key shortcuts.
//!
//! This module tracks the current state of pressed keys and constructs canonical
//! key chord representations for keybinding lookups. It distinguishes between
//! modifier keys (Shift, Control, Alt, etc.) and regular keys, ensuring that
//! valid chords consist of one or more modifiers plus exactly one non-modifier key.
//!
//! The [`ChordState`] struct maintains a set of currently pressed keycodes and
//! provides methods to add/remove keys and generate chord strings. Chord strings
//! are normalized to a canonical format where modifiers are sorted alphabetically
//! and separated by spaces (e.g. "Alt_L Control_L x" for Ctrl+Alt+X).
use std::collections::HashSet;
use xkbcommon::xkb;
use xkbcommon::xkb::{keysyms, Keysym};

const MAX_PRESSED_KEYS: usize = 16;

/// Manages the state of currently pressed keys for chord detection.
pub struct ChordState {
    pressed_keys: HashSet<xkb::Keycode>,
}

impl ChordState {
    /// Creates a new, empty ChordState.
    pub fn new() -> Self {
        Self {
            pressed_keys: HashSet::with_capacity(MAX_PRESSED_KEYS),
        }
    }

    /// Adds a keycode to the set of currently pressed keys.
    /// The HashSet automatically handles duplicates.
    pub fn add_key(&mut self, keycode: xkb::Keycode) {
        if self.pressed_keys.len() >= MAX_PRESSED_KEYS {
            eprintln!("Warning: Maximum number of pressed keys exceeded.");
            return;
        }
        self.pressed_keys.insert(keycode);
    }

    /// Removes a keycode from the set of currently pressed keys.
    pub fn remove_key(&mut self, keycode: xkb::Keycode) {
        self.pressed_keys.remove(&keycode);
    }

    /// Constructs a chord string and sends it if it's valid.
    ///
    /// A valid chord consists of one or more modifiers and EXACTLY ONE non-modifier
    /// key. The resulting string is canonical: modifiers are sorted alphabetically,
    /// followed by the single non-modifier key, all space-separated.
    pub fn get_keychord(&self, xkb_state: &xkb::State) -> Option<String> {
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
            return None;
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
    pub fn is_modifier_keysym(keysym: Keysym) -> bool {
        matches!(
            keysym.into(),
            keysyms::KEY_Shift_L
                | keysyms::KEY_Shift_R
                | keysyms::KEY_Control_L
                | keysyms::KEY_Control_R
                | keysyms::KEY_Alt_L
                | keysyms::KEY_Alt_R
                | keysyms::KEY_Meta_L
                | keysyms::KEY_Meta_R
                | keysyms::KEY_Super_L
                | keysyms::KEY_Super_R
                | keysyms::KEY_Hyper_L
                | keysyms::KEY_Hyper_R
                | keysyms::KEY_Caps_Lock
                | keysyms::KEY_Shift_Lock
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use xkbcommon::xkb::{self, Keycode};
    use xkbcommon::xkb;

    fn init_xkb_state() -> xkb::State {
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
        .expect("Failed to create XKB keymap");

        xkb::State::new(&keymap)
    }

    #[test]
    fn new_should_start_with_empty_pressed_keys() {
        let state = ChordState::new();
        assert!(state.pressed_keys.is_empty());
    }

    #[test]
    fn add_key_should_store_key_in_pressed_keys() {
        let xkb_state = init_xkb_state();
        let keycode = xkb_state.get_keymap().min_keycode();
        let mut state = ChordState::new();
        state.add_key(keycode);
        assert!(state.pressed_keys.contains(&keycode));
    }

    #[test]
    fn add_key_should_ignore_duplicates() {
        let xkb_state = init_xkb_state();
        let keycode = xkb_state.get_keymap().min_keycode();
        let mut state = ChordState::new();

        state.add_key(keycode);
        state.add_key(keycode);

        assert_eq!(state.pressed_keys.len(), 1);
    }

    #[test]
    fn add_key_should_warn_when_exceeding_capacity() {
        let xkb_state = init_xkb_state();
        let mut state = ChordState::new();

        // Fill the hashset to capacity
        for i in 0..MAX_PRESSED_KEYS {
            let keycode = xkb_state.get_keymap().min_keycode().raw() + i as u32;
            state.add_key(keycode.into());
        }

        // Verify we have exactly MAX_PRESSED_KEYS
        assert_eq!(state.pressed_keys.len(), MAX_PRESSED_KEYS);

        // Try to add one more key - this should trigger the warning and not add the key
        let extra_keycode = xkb_state.get_keymap().min_keycode().raw() + MAX_PRESSED_KEYS as u32;
        state.add_key(extra_keycode.into());

        // Verify the key was not added (still at capacity)
        assert_eq!(state.pressed_keys.len(), MAX_PRESSED_KEYS);
        assert!(!state.pressed_keys.contains(&extra_keycode.into()));
    }

    #[test]
    fn remove_key_should_remove_key_from_pressed_keys() {
        let xkb_state = init_xkb_state();
        let keycode = xkb_state.get_keymap().min_keycode();
        let mut state = ChordState::new();

        state.add_key(keycode);
        state.remove_key(keycode);

        assert!(!state.pressed_keys.contains(&keycode));
    }

    #[test]
    fn is_modifier_keysym_should_return_true_with_modifier() {
        let keysym = Keysym::new(keysyms::KEY_Super_L);
        let result = ChordState::is_modifier_keysym(keysym);
        assert!(result);
    }

    #[test]
    fn is_modifier_keysym_should_return_false_with_non_modifier() {
        let keysym = Keysym::new(keysyms::KEY_a);
        let result = ChordState::is_modifier_keysym(keysym);
        assert!(!result);
    }

    #[test]
    fn get_keychord_should_succeed_with_valid_content() {
        let xkb_state = init_xkb_state();
        let keymap = xkb_state.get_keymap();

        let keycodes = [
            keymap.key_by_name("LWIN").unwrap(),
            keymap.key_by_name("AD02").unwrap(),
        ];

        let mut state = ChordState::new();
        state.pressed_keys = keycodes.into_iter().collect();

        let keychord = state.get_keychord(&xkb_state);

        assert_eq!(state.pressed_keys.len(), 2);
        assert_eq!(keychord, Some(String::from("Super_L w")));
    }

    #[test]
    fn get_keychord_multi_nonmodifiers_should_return_none() {
        let xkb_state = init_xkb_state();
        let keymap = xkb_state.get_keymap();
        let keycodes = [
            keymap.key_by_name("LWIN").unwrap(),
            keymap.key_by_name("AE01").unwrap(),
            keymap.key_by_name("AE02").unwrap(),
        ];

        let xkb_state = init_xkb_state();
        let mut state = ChordState::new();
        state.pressed_keys = keycodes.into_iter().collect();

        let keychord = state.get_keychord(&xkb_state);

        assert!(keychord.is_none());
    }
}
