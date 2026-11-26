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
    #[serde(default, skip_serializing)]
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub remote_cwd: String,
    #[serde(default = "ssh_agent_default")]
    pub use_agent: bool,
    #[serde(default)]
    pub strict_host_key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) rdp: Option<crate::remote_desktop::profile::RdpProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vnc: Option<crate::remote_desktop::profile::VncProfile>,
}

fn ssh_agent_default() -> bool {
    true
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
    #[serde(default)]
    pub run_in_terminal: bool,
}

fn default_command_category() -> String {
    "General".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdbDeviceConfig {
    pub id: String,
    pub name: String,
    pub serial: String,
    #[serde(default)]
    pub is_wireless: bool,
    #[serde(default)]
    pub ip_port: Option<String>,
    #[serde(default)]
    pub scrcpy_max_fps: Option<u32>,
    #[serde(default)]
    pub scrcpy_bitrate_mbps: Option<u32>,
    #[serde(default)]
    pub scrcpy_stay_awake: bool,
    #[serde(default)]
    pub scrcpy_turn_screen_off: bool,
    #[serde(default)]
    pub scrcpy_no_audio: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshIdleMode {
    #[default]
    PageOrWindowInactive,
    WindowInactive,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshIdleSettings {
    pub mode: SshIdleMode,
    pub minutes: u32,
}

impl Default for SshIdleSettings {
    fn default() -> Self {
        Self { mode: SshIdleMode::default(), minutes: 5 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRecordingFormat {
    #[default]
    Mp4,
    PngSequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteDesktopSettings {
    pub default_quality: crate::remote_desktop::profile::Quality,
    pub default_clipboard: crate::remote_desktop::profile::Clipboard,
    pub show_live_fps: bool,
    pub warn_unencrypted_vnc: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h264_library_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_folder: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_folder: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<PathBuf>,
    pub recording_fps: u32,
    pub recording_format: RemoteRecordingFormat,
    pub copy_screenshots_to_clipboard: bool,
}

impl Default for RemoteDesktopSettings {
    fn default() -> Self {
        Self {
            default_quality: crate::remote_desktop::profile::Quality::Auto,
            default_clipboard: crate::remote_desktop::profile::Clipboard::Both,
            show_live_fps: false,
            warn_unencrypted_vnc: true,
            h264_library_path: None,
            screenshot_folder: None,
            recording_folder: None,
            ffmpeg_path: None,
            recording_fps: 30,
            recording_format: RemoteRecordingFormat::Mp4,
            copy_screenshots_to_clipboard: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SshProbeTimeouts {
    pub connection_test_seconds: u32,
    pub port_scan_seconds: u32,
}

impl Default for SshProbeTimeouts {
    fn default() -> Self {
        Self { connection_test_seconds: 10, port_scan_seconds: 20 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub server_tab_order: Vec<String>,
    #[serde(default)]
    pub ssh_hosts: Vec<SshRemoteHost>,
    #[serde(default)]
    pub adb_devices: Vec<AdbDeviceConfig>,
    #[serde(default)]
    pub scripts: Vec<CustomScript>,
    #[serde(default)]
    pub saved_commands: Vec<SavedCommand>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub app_autostart: bool,
    #[serde(default)]
    pub journal_refresh_interval: Option<u32>,
    #[serde(default)]
    pub ssh_idle: SshIdleSettings,
    #[serde(default)]
    pub ssh_probe_timeouts: SshProbeTimeouts,
    #[serde(default)]
    pub remote_desktop: RemoteDesktopSettings,
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
        if self.ssh_hosts.iter().any(|host| !host.password.is_empty()) {
            return Err("Cannot save an unprotected SSH password.".into());
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialization error: {}", e))?;
        use std::io::Write;
        // Resolve an existing symlink before replacing the file it points to.
        let target = if fs::symlink_metadata(path).is_ok() {
            path.canonicalize().map_err(|e| format!("Config path error: {e}"))?
        } else {
            path.to_path_buf()
        };
        let directory = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(directory)
            .map_err(|e| format!("Could not stage configuration: {e}"))?;
        temp.write_all(content.as_bytes()).and_then(|()| temp.as_file().sync_all())
            .map_err(|e| format!("Write error: {e}"))?;
        temp.persist(&target).map_err(|e| format!("Could not replace configuration: {e}"))?;
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
    fn old_config_defaults_remote_desktop_safety_settings() {
        let config: ConfigFile = serde_json::from_str(r#"{"servers":[]}"#).unwrap();
        assert_eq!(
            config.remote_desktop.default_quality,
            crate::remote_desktop::profile::Quality::Auto
        );
        assert_eq!(
            config.remote_desktop.default_clipboard,
            crate::remote_desktop::profile::Clipboard::Both
        );
        assert!(!config.remote_desktop.show_live_fps);
        assert!(config.remote_desktop.warn_unencrypted_vnc);
        assert!(config.remote_desktop.screenshot_folder.is_none());
        assert!(config.remote_desktop.recording_folder.is_none());
        assert!(config.remote_desktop.ffmpeg_path.is_none());
        assert_eq!(config.remote_desktop.recording_fps, 30);
        assert_eq!(config.remote_desktop.recording_format, RemoteRecordingFormat::Mp4);
        assert!(!config.remote_desktop.copy_screenshots_to_clipboard);
    }

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
