use crate::config::SshRemoteHost;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_RECORDS: usize = 100;
static HISTORY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct HistoryFile {
    version: u8,
    records: Vec<HistoryRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct HistoryRecord {
    pub at: String,
    pub host_id: String,
    pub host_name: String,
    pub host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
    pub protocol: String,
    pub action: String,
    pub prior_unit_active: Option<bool>,
    pub prior_unit_enabled: Option<bool>,
    pub completed_steps: Vec<String>,
    pub completed: bool,
}

pub(crate) fn record_action(
    host: &SshRemoteHost,
    protocol: &str,
    action: &str,
    distro: Option<&str>,
    prior_unit_active: Option<bool>,
    prior_unit_enabled: Option<bool>,
    completed_steps: Vec<String>,
    completed: bool,
) -> Result<(), String> {
    if completed_steps.is_empty() {
        return Ok(());
    }
    let path = history_path()?;
    let record = HistoryRecord {
        at: chrono::Utc::now().to_rfc3339(),
        host_id: limited(&host.id, 256),
        host_name: limited(&host.name, 256),
        host: limited(&host.host, 512),
        ssh_port: host.port,
        ssh_user: limited(&host.user, 128),
        distro: distro.map(|name| limited(name, 256)),
        protocol: limited(protocol, 8),
        action: limited(action, 128),
        prior_unit_active,
        prior_unit_enabled,
        completed_steps,
        completed,
    };
    let _guard = HISTORY_LOCK
        .lock()
        .map_err(|_| "The local service history is unavailable.".to_string())?;
    append_to(&path, record)
}

fn limited(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn recent_for_host(host: &SshRemoteHost) -> Result<Vec<HistoryRecord>, String> {
    let path = history_path()?;
    let _guard = HISTORY_LOCK
        .lock()
        .map_err(|_| "The local service history is unavailable.".to_string())?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Could not read local service history: {error}")),
    };
    if metadata.len() > MAX_FILE_BYTES {
        return Err("The local service history is larger than its safety limit.".into());
    }
    let history: HistoryFile = serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("Could not read local service history: {error}"))?,
    )
    .map_err(|error| format!("Could not read local service history: {error}"))?;
    if history.version != 1 {
        return Err("The local service history uses an unsupported version.".into());
    }
    Ok(history
        .records
        .into_iter()
        .filter(|record| {
            record.host_id == host.id
                && record.host == host.host
                && record.ssh_port == host.port
                && record.ssh_user == host.user
        })
        .rev()
        .take(10)
        .collect())
}

fn history_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|p| p.join("AppData/Local"))
        })
        .ok_or_else(|| "The local state folder is unavailable.".to_string())?;

    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join("Library/Application Support"))
        .ok_or_else(|| "The local state folder is unavailable.".to_string())?;

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".local/state"))
        })
        .ok_or_else(|| "The local state folder is unavailable.".to_string())?;

    #[cfg(target_os = "windows")]
    let dir = root.join("Server Launcher").join("state");
    #[cfg(target_os = "macos")]
    let dir = root.join("Server Launcher").join("state");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let dir = root.join("server-launcher");
    Ok(dir.join("remote-service-history.json"))
}

fn append_to(path: &Path, record: HistoryRecord) -> Result<(), String> {
    let mut history = match fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_FILE_BYTES => {
            return Err("The local service history is larger than its safety limit.".into());
        }
        Ok(_) => serde_json::from_slice::<HistoryFile>(
            &fs::read(path)
                .map_err(|error| format!("Could not read local service history: {error}"))?,
        )
        .map_err(|error| format!("Could not read local service history: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HistoryFile {
            version: 1,
            records: Vec::new(),
        },
        Err(error) => return Err(format!("Could not read local service history: {error}")),
    };
    if history.version != 1 {
        return Err("The local service history uses an unsupported version.".into());
    }
    history.records.push(record);
    let excess = history.records.len().saturating_sub(MAX_RECORDS);
    if excess > 0 {
        history.records.drain(..excess);
    }

    let directory = path
        .parent()
        .ok_or_else(|| "The local service history path has no parent folder.".to_string())?;
    fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create local service history folder: {error}"))?;
    #[cfg(unix)]
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not protect local service history folder: {error}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("Could not stage local service history: {error}"))?;
    #[cfg(unix)]
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not protect local service history: {error}"))?;
    let contents = serde_json::to_vec(&history)
        .map_err(|error| format!("Could not write local service history: {error}"))?;
    if contents.len() as u64 > MAX_FILE_BYTES {
        return Err("The local service history reached its safety limit.".into());
    }
    temporary
        .write_all(&contents)
        .map_err(|error| format!("Could not write local service history: {error}"))?;
    temporary
        .write_all(b"\n")
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("Could not write local service history: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("Could not replace local service history: {}", error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_contains_no_credential_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        for index in 0..=MAX_RECORDS {
            append_to(
                &path,
                HistoryRecord {
                    at: format!("2026-01-01T00:00:{index:02}Z"),
                    host_id: "host-id".into(),
                    host_name: "Host".into(),
                    host: "192.0.2.10".into(),
                    ssh_port: 22,
                    ssh_user: "user".into(),
                    distro: Some("Ubuntu 24.04".into()),
                    protocol: "RDP".into(),
                    action: "Start xrdp".into(),
                    prior_unit_active: Some(false),
                    prior_unit_enabled: Some(false),
                    completed_steps: vec!["start".into()],
                    completed: true,
                },
            )
            .unwrap();
        }
        let contents = fs::read_to_string(path).unwrap();
        let history: HistoryFile = serde_json::from_str(&contents).unwrap();
        assert_eq!(history.records.len(), MAX_RECORDS);
        assert!(!contents.contains("password"));
    }

    #[test]
    fn legacy_history_without_distro_still_loads() {
        let old = r#"{"version":1,"records":[{"at":"2026-01-01T00:00:00Z","host_id":"host","host_name":"Host","host":"192.0.2.10","ssh_port":22,"ssh_user":"user","protocol":"RDP","action":"Start xrdp","prior_unit_active":false,"prior_unit_enabled":false,"completed_steps":["start"],"completed":true}]}"#;
        let history: HistoryFile = serde_json::from_str(old).unwrap();
        assert_eq!(history.records[0].distro, None);
    }
}
