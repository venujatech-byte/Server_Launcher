use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAction {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub key: String,
    pub name: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    pub command: String,
    #[serde(default)]
    pub stop_command: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default = "default_group")]
    pub group: String,
    #[serde(default)]
    pub own_console: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub actions: Vec<CustomAction>,
    #[serde(default)]
    pub links: Vec<CustomLink>,
}

fn default_cwd() -> String {
    ".".to_string()
}

fn default_group() -> String {
    "General".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    pub servers: Vec<ServerConfig>,
}

impl ConfigFile {
    pub fn default_path() -> PathBuf {
        // Look in current working directory or next to binary
        let local_path = PathBuf::from("servers.json");
        if local_path.exists() {
            return local_path;
        }

        // Try parent directory (e.g. if running inside server_launcher_rust/)
        let parent_path = PathBuf::from("../servers.json");
        if parent_path.exists() {
            return parent_path;
        }

        local_path
    }

    pub fn load_from_file(path: &Path) -> Self {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<ConfigFile>(&content) {
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialization error: {}", e))?;
        fs::write(path, content).map_err(|e| format!("Write error: {}", e))?;
        Ok(())
    }
}
