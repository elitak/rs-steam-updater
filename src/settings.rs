use indexmap::IndexMap;
use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Top-level settings loaded from `settings.yml`.
#[derive(Debug, Deserialize)]
pub struct Settings {
    /// Where to install all games. Defaults to `C:\Program Files (x86)\Steam`.
    pub library_root: Option<String>,
    /// Ordered map of Steam accounts. The map key is the Steam login name.
    pub accounts: IndexMap<String, Account>,
}

/// Per-account settings.
#[derive(Debug, Deserialize)]
pub struct Account {
    /// Steam account password.
    pub password: Option<String>,
    /// Explicit numeric Steam App IDs to update.
    #[serde(default, rename = "appIDs")]
    pub app_ids: Vec<u32>,
    /// Regex patterns matched against Steam app titles (resolved via Steam Web API).
    #[serde(default, rename = "appREs")]
    pub app_res: Vec<String>,
}

impl Settings {
    /// Load and parse `settings.yml` from the given path.
    pub fn load(settings_path: &Path) -> Result<Settings, Box<dyn std::error::Error>> {
        if !settings_path.exists() {
            return Err(format!(
                "settings.yml not found at: {}",
                settings_path.display()
            )
            .into());
        }
        let content = fs::read_to_string(settings_path)?;
        let settings: Settings = serde_yaml::from_str(&content)?;
        if settings.accounts.is_empty() {
            return Err("No 'accounts' key found in settings.yml".into());
        }
        Ok(settings)
    }

    /// Returns the library root, defaulting to `C:\Program Files (x86)\Steam`.
    pub fn library_root(&self) -> &str {
        self.library_root
            .as_deref()
            .unwrap_or(r"C:\Program Files (x86)\Steam")
    }
}
