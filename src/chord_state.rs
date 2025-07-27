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
