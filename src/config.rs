//! Settings on disk (server, theme, last place) and the session token in
//! the OS keychain. The token never goes into a plain file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: String,
    pub theme: ThemePref,
    pub last_team: String,
    /// Last channel per team.
    pub last_channel: HashMap<String, String>,
    /// Closing the window keeps the app in the menu bar (notifications go on).
    #[serde(default = "yes")]
    pub keep_running: bool,
    /// The account signed in on each server: its local cache is that account's.
    pub accounts: HashMap<String, String>,
}

fn yes() -> bool {
    true
}

pub struct Dirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    pub fn new() -> Self {
        if let Ok(root) = std::env::var("MMFAST_HOME") {
            let root = PathBuf::from(root);
            return Self { config: root.join("config"), data: root.join("data"), cache: root.join("cache") };
        }
        let dirs = directories::ProjectDirs::from("dev", "mmfast", "mmfast").expect("home directory");
        Self {
            config: dirs.config_dir().to_path_buf(),
            data: dirs.data_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
        }
    }
}

impl Config {
    fn path(dirs: &Dirs) -> PathBuf {
        dirs.config.join("config.json")
    }

    pub fn load(dirs: &Dirs) -> Self {
        std::fs::read_to_string(Self::path(dirs)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Written to a temporary file and renamed, so a crash never leaves half a file.
    pub fn save(&self, dirs: &Dirs) {
        let path = Self::path(dirs);
        let _ = std::fs::create_dir_all(&dirs.config);
        let tmp = path.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string_pretty(self)
            && std::fs::write(&tmp, json).is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: String::new(),
            theme: ThemePref::System,
            last_team: String::new(),
            last_channel: HashMap::new(),
            keep_running: true,
            accounts: HashMap::new(),
        }
    }
}

const SERVICE: &str = "mmfast";

pub fn load_token(server: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, server).ok()?.get_password().ok()
}

pub fn save_token(server: &str, token: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, server)
        && let Err(err) = entry.set_password(token)
    {
        log::warn!("could not store the session in the keychain: {err}");
    }
}

pub fn forget_token(server: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, server) {
        let _ = entry.delete_credential();
    }
}
