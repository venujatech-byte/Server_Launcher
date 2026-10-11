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
    pub auto_restart: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub actions: Vec<CustomAction>,
    #[serde(default)]
    pub links: Vec<CustomLink>,
    #[serde(default)]
    pub server_type: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            key: String::new(),
            name: String::new(),
            cwd: default_cwd(),
            command: String::new(),
            stop_command: String::new(),
            port: 0,
            group: default_group(),
            own_console: false,
            auto_restart: false,
            autostart: false,
            env: HashMap::new(),
            actions: Vec::new(),
            links: Vec::new(),
            server_type: None,
            url: None,
            headers: HashMap::new(),
        }
    }
}

fn default_cwd() -> String {
    ".".to_string()
}

fn default_group() -> String {
    "General".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshRemoteHost {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default = "default_ssh_user")]
    pub user: String,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub remote_cwd: String,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_ssh_user() -> String {
    "ubuntu".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomScript {
    pub id: String,
    pub name: String,
    pub script: String,
    #[serde(default = "default_script_shell")]
    pub shell: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub host_id: Option<String>, // None for Local PC, Some(id) for SSH Host
}

fn default_script_shell() -> String {
    "bash".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedCommand {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_command_category")]
    pub category: String,
    #[serde(default)]
    pub host_id: Option<String>, // None for Local PC, Some(id) for SSH Host
}

fn default_command_category() -> String {
    "General".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub ssh_hosts: Vec<SshRemoteHost>,
    #[serde(default)]
    pub scripts: Vec<CustomScript>,
    #[serde(default)]
    pub saved_commands: Vec<SavedCommand>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub app_autostart: bool,
    #[serde(default)]
    pub journal_refresh_interval: Option<u32>,
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

pub fn parse_server_json_value(key: &str, val: &serde_json::Value) -> Option<ServerConfig> {
    // 1. If it directly deserializes as ServerConfig and has content
    if let Ok(mut cfg) = serde_json::from_value::<ServerConfig>(val.clone()) {
        if !cfg.name.is_empty() || !cfg.command.is_empty() || cfg.url.is_some() {
            if cfg.key.trim().is_empty() {
                cfg.key = key.to_string();
            }
            if cfg.name.trim().is_empty() {
                let mut chars = key.chars();
                cfg.name = match chars.next() {
                    None => "Server".to_string(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                };
            }
            if cfg.command.is_empty()
                && (cfg.server_type.as_deref() == Some("http") || cfg.url.is_some())
            {
                if let Some(u) = &cfg.url {
                    let mut h_args = String::new();
                    for (hk, hv) in &cfg.headers {
                        h_args.push_str(&format!(" -H \"{}: {}\"", hk, hv));
                    }
                    cfg.command = format!("curl -i{} {}", h_args, u);
                }
            }
            if cfg.links.is_empty() {
                if let Some(u) = &cfg.url {
                    cfg.links.push(CustomLink {
                        label: "MCP Endpoint".to_string(),
                        url: u.clone(),
                    });
                }
            }
            if cfg.actions.is_empty()
                && !cfg.command.is_empty()
                && (cfg.server_type.as_deref() == Some("http") || cfg.url.is_some())
            {
                cfg.actions.push(CustomAction {
                    label: "Test Ping".to_string(),
                    command: cfg.command.clone(),
                });
            }
            return Some(cfg);
        }
    }

    let srv_obj = val.as_object()?;

    let server_type = srv_obj
        .get("type")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    let url = srv_obj
        .get("url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());

    let mut headers = HashMap::new();
    if let Some(h_obj) = srv_obj.get("headers").and_then(|h| h.as_object()) {
        for (hk, hv) in h_obj {
            let hv_str = if let Some(s) = hv.as_str() {
                s.to_string()
            } else {
                hv.to_string()
            };
            headers.insert(hk.clone(), hv_str);
        }
    }

    let mut env = HashMap::new();
    if let Some(e_obj) = srv_obj.get("env").and_then(|e| e.as_object()) {
        for (ek, ev) in e_obj {
            let ev_str = if let Some(s) = ev.as_str() {
                s.to_string()
            } else {
                ev.to_string()
            };
            env.insert(ek.clone(), ev_str);
        }
    }
    for (hk, hv) in &headers {
        let env_key = hk.to_uppercase().replace('-', "_");
        env.entry(env_key).or_insert_with(|| hv.clone());
    }

    let mut command = srv_obj
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(args) = srv_obj.get("args").and_then(|a| a.as_array()) {
        for arg in args {
            if let Some(s) = arg.as_str() {
                if !command.is_empty() {
                    command.push(' ');
                }
                command.push_str(s);
            }
        }
    }

    let is_http = server_type.as_deref() == Some("http")
        || server_type.as_deref() == Some("sse")
        || url.is_some();

    if command.trim().is_empty() && is_http {
        if let Some(u) = &url {
            let mut h_args = String::new();
            for (hk, hv) in &headers {
                h_args.push_str(&format!(" -H \"{}: {}\"", hk, hv));
            }
            command = format!("curl -i{} {}", h_args, u);
        }
    }

    if command.trim().is_empty() && url.is_none() {
        return None;
    }

    let raw_name = srv_obj.get("name").and_then(|n| n.as_str()).unwrap_or(key);
    let mut name_chars = raw_name.chars();
    let name = match name_chars.next() {
        None => "Server".to_string(),
        Some(f) => f.to_uppercase().collect::<String>() + name_chars.as_str(),
    };

    let group = srv_obj
        .get("group")
        .and_then(|g| g.as_str())
        .unwrap_or(if is_http { "MCP" } else { "General" })
        .to_string();
    let port = srv_obj.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16;

    let mut links = Vec::new();
    if let Some(u) = &url {
        links.push(CustomLink {
            label: "MCP Endpoint".to_string(),
            url: u.clone(),
        });
    }

    let mut actions = Vec::new();
    if is_http && !command.is_empty() {
        actions.push(CustomAction {
            label: "Test Ping".to_string(),
            command: command.clone(),
        });
        if let Some(u) = &url {
            let mut h_args = String::new();
            for (hk, hv) in &headers {
                h_args.push_str(&format!(" -H \"{}: {}\"", hk, hv));
            }
            actions.push(CustomAction {
                label: "List Tools".to_string(),
                command: format!(
                    "curl -i -X POST -H \"Content-Type: application/json\"{} -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}' {}",
                    h_args, u
                ),
            });
        }
    }

    Some(ServerConfig {
        key: key.to_string(),
        name,
        cwd: srv_obj
            .get("cwd")
            .and_then(|c| c.as_str())
            .unwrap_or(".")
            .to_string(),
        command,
        stop_command: srv_obj
            .get("stop_command")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        port,
        group,
        own_console: false,
        auto_restart: false,
        autostart: false,
        env,
        actions,
        links,
        server_type: if is_http {
            Some("http".to_string())
        } else {
            server_type
        },
        url,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stitch_mcp_schema() {
        let json_snippet = r#"{
            "type": "http",
            "url": "https://stitch.googleapis.com/mcp",
            "headers": {
                "X-Goog-Api-Key": "YOUR_API_KEY"
            }
        }"#;
        let val: serde_json::Value = serde_json::from_str(json_snippet).unwrap();
        let cfg = parse_server_json_value("stitch", &val).expect("Should parse stitch config");

        assert_eq!(cfg.key, "stitch");
        assert_eq!(cfg.name, "Stitch");
        assert_eq!(cfg.group, "MCP");
        assert_eq!(cfg.server_type.as_deref(), Some("http"));
        assert_eq!(cfg.url.as_deref(), Some("https://stitch.googleapis.com/mcp"));
        assert_eq!(
            cfg.headers.get("X-Goog-Api-Key").map(|s| s.as_str()),
            Some("YOUR_API_KEY")
        );
        assert!(cfg.command.contains("curl -i"));
        assert!(cfg.command.contains("X-Goog-Api-Key"));
        assert_eq!(cfg.links.len(), 1);
        assert_eq!(cfg.links[0].label, "MCP Endpoint");
        assert_eq!(cfg.links[0].url, "https://stitch.googleapis.com/mcp");
        assert!(cfg.actions.iter().any(|a| a.label == "Test Ping"));
        assert!(cfg.actions.iter().any(|a| a.label == "List Tools"));
    }
}
