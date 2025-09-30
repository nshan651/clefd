use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type Keybindings = Arc<RwLock<HashMap<String, String>>>;
