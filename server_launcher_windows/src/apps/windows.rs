use super::actions::{format_bytes, open_folder_in_host, populate_data_path_sizes, stream_command_output};
use super::model::{AppDataPath, AppEntry, AppSource, AppsEvent, ClearDataMode, RepoEntry};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn cmd_exists(bin: &str) -> bool {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(bin).is_file() || dir.join(format!("{}.exe", bin)).is_file() {
                return true;
            }
        }
    }
    false
}

fn run_powershell_json(script: &str) -> Option<serde_json::Value> {
    let out = hidden_cmd("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str(&text).ok()
}

fn json_items(val: serde_json::Value) -> Vec<serde_json::Value> {
    match val {
        serde_json::Value::Array(arr) => arr,
        obj @ serde_json::Value::Object(_) => vec![obj],
        _ => Vec::new(),
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn find_win32_data_paths(name: &str, publisher: &str) -> Vec<AppDataPath> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let candidates: Vec<&str> = [name.trim(), publisher.trim()]
        .into_iter()
        .filter(|s| !s.is_empty() && !s.contains('\\') && !s.contains('/') && *s != "." && *s != "..")
        .collect();

    for (env_key, label, is_cache) in [
        ("LOCALAPPDATA", "Local AppData", false),
        ("APPDATA", "Roaming AppData", false),
    ] {
        if let Some(base) = std::env::var_os(env_key).map(PathBuf::from) {
            for cand in &candidates {
                let p = base.join(cand);
                if p.exists() && seen.insert(p.clone()) {
                    out.push(AppDataPath {
                        label: label.to_string(),
                        path: p,
                        is_cache,
                        size_bytes: None,
                    });
                }
            }
        }
    }
    out
}

fn discover_registry_win32() -> Vec<AppEntry> {
    let ps = r#"
$paths = @(
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
Get-ItemProperty $paths -ErrorAction SilentlyContinue |
  Where-Object { $_.DisplayName -and -not $_.SystemComponent } |
  Select-Object DisplayName, DisplayVersion, Publisher, InstallLocation, UninstallString, EstimatedSize |
  ConvertTo-Json -Compress
"#;
    let Some(val) = run_powershell_json(ps) else {
        return Vec::new();
    };
    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    for item in json_items(val) {
        let name = json_str(&item, "DisplayName");
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let version = {
            let v = json_str(&item, "DisplayVersion");
            if v.is_empty() { "—".to_string() } else { v }
        };
        let publisher = json_str(&item, "Publisher");
        let install_loc = {
            let l = json_str(&item, "InstallLocation");
            if l.is_empty() { None } else { Some(l) }
        };
        let uninstall_str = {
            let u = json_str(&item, "UninstallString");
            if u.is_empty() { None } else { Some(u) }
        };
        let size_bytes = item
            .get("EstimatedSize")
            .and_then(|x| x.as_u64())
            .map(|kb| kb.saturating_mul(1024));

        let data_paths = find_win32_data_paths(&name, &publisher);
        let can_uninstall = uninstall_str.is_some();

        apps.push(AppEntry {
            id: name.clone(),
            name: name.clone(),
            source: AppSource::Win32,
            version,
            size_bytes,
            size_display: size_bytes.map(format_bytes).unwrap_or_else(|| "—".to_string()),
            publisher_or_origin: if publisher.is_empty() {
                "Windows Registry".to_string()
            } else {
                publisher
            },
            description: "Installed Windows desktop application".to_string(),
            categories: "Win32".to_string(),
            install_location: install_loc,
            data_paths,
            exec_cmd: None,
            uninstall_cmd: uninstall_str,
            desktop_file: None,
            update_available: None,
            user_installed: true,
            can_launch: false,
            can_update: false,
            can_uninstall,
        });
    }
    apps
}

fn discover_appx() -> Vec<AppEntry> {
    let ps = r#"
Get-AppxPackage |
  Where-Object { -not $_.IsFramework -and -not $_.NonRemovable } |
  Select-Object Name, PackageFullName, PackageFamilyName, Version, Publisher, InstallLocation |
  ConvertTo-Json -Compress
"#;
    let Some(val) = run_powershell_json(ps) else {
        return Vec::new();
    };
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let mut apps = Vec::new();

    for item in json_items(val) {
        let name = json_str(&item, "Name");
        let full_name = json_str(&item, "PackageFullName");
        let family_name = json_str(&item, "PackageFamilyName");
        if name.is_empty() || full_name.is_empty() {
            continue;
        }
        let version = json_str(&item, "Version");
        let publisher = json_str(&item, "Publisher");
        let install_loc = {
            let l = json_str(&item, "InstallLocation");
            if l.is_empty() { None } else { Some(l) }
        };

        let mut data_paths = Vec::new();
        if let (Some(ref base), false) = (&local_app_data, family_name.is_empty()) {
            let pkg_root = base.join("Packages").join(&family_name);
            for (lbl, sub, is_cache) in [
                ("LocalCache", "LocalCache", true),
                ("LocalState", "LocalState", false),
            ] {
                let p = pkg_root.join(sub);
                if p.exists() {
                    data_paths.push(AppDataPath {
                        label: lbl.to_string(),
                        path: p,
                        is_cache,
                        size_bytes: None,
                    });
                }
            }
        }

        let exec_cmd = if !family_name.is_empty() {
            Some(format!("shell:AppsFolder\\{}!App", family_name))
        } else {
            None
        };

        apps.push(AppEntry {
            id: full_name.clone(),
            name,
            source: AppSource::Appx,
            version: if version.is_empty() { "—".to_string() } else { version },
            size_bytes: None,
            size_display: "—".to_string(),
            publisher_or_origin: if publisher.is_empty() {
                "Microsoft Store".to_string()
            } else {
                publisher
            },
            description: format!("MSIX / UWP Package ({})", full_name),
            categories: "Microsoft Store".to_string(),
            install_location: install_loc,
            data_paths,
            exec_cmd: exec_cmd.clone(),
            uninstall_cmd: Some(full_name),
            desktop_file: None,
            update_available: None,
            user_installed: true,
            can_launch: exec_cmd.is_some(),
            can_update: false,
            can_uninstall: true,
        });
    }
    apps
}

fn discover_winget() -> Vec<AppEntry> {
    if !cmd_exists("winget") {
        return Vec::new();
    }
    let Ok(out) = hidden_cmd("winget")
        .args(["list", "--accept-source-agreements"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut apps = Vec::new();
    let mut past_sep = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("---") {
            past_sep = true;
            continue;
        }
        if !past_sep {
            continue;
        }
        // Winget columns are separated by 2+ spaces
        let cols: Vec<&str> = trimmed
            .split("  ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].to_string();
        let id = cols[1].to_string();
        let version = cols.get(2).copied().unwrap_or("—").to_string();
        let source_col = cols.last().copied().unwrap_or("winget");

        apps.push(AppEntry {
            id: id.clone(),
            name,
            source: AppSource::Winget,
            version,
            size_bytes: None,
            size_display: "—".to_string(),
            publisher_or_origin: format!("Winget ({})", source_col),
            description: format!("Winget package ({})", id),
            categories: "Winget".to_string(),
            install_location: None,
            data_paths: Vec::new(),
            exec_cmd: None,
            uninstall_cmd: None,
            desktop_file: None,
            update_available: None,
            user_installed: true,
            can_launch: false,
            can_update: true,
            can_uninstall: true,
        });
    }
    apps
}

fn discover_scoop_and_choco() -> Vec<AppEntry> {
    let mut apps = Vec::new();

    if cmd_exists("scoop") {
        if let Ok(out) = hidden_cmd("scoop").args(["list"]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let cols: Vec<&str> = line.split_whitespace().collect();
                    if cols.len() >= 2 && cols[0] != "Name" && !cols[0].starts_with("---") {
                        let name = cols[0].to_string();
                        apps.push(AppEntry {
                            id: name.clone(),
                            name,
                            source: AppSource::Scoop,
                            version: cols[1].to_string(),
                            size_bytes: None,
                            size_display: "—".to_string(),
                            publisher_or_origin: "Scoop".to_string(),
                            description: "Scoop package".to_string(),
                            categories: "Scoop".to_string(),
                            install_location: None,
                            data_paths: Vec::new(),
                            exec_cmd: None,
                            uninstall_cmd: None,
                            desktop_file: None,
                            update_available: None,
                            user_installed: true,
                            can_launch: false,
                            can_update: true,
                            can_uninstall: true,
                        });
                    }
                }
            }
        }
    }

    if cmd_exists("choco") {
        if let Ok(out) = hidden_cmd("choco").args(["list", "--local-only"]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let cols: Vec<&str> = line.split_whitespace().collect();
                    if cols.len() == 2 && !cols[0].eq_ignore_ascii_case("Chocolatey") {
                        let name = cols[0].to_string();
                        apps.push(AppEntry {
                            id: name.clone(),
                            name,
                            source: AppSource::Choco,
                            version: cols[1].to_string(),
                            size_bytes: None,
                            size_display: "—".to_string(),
                            publisher_or_origin: "Chocolatey".to_string(),
                            description: "Chocolatey package".to_string(),
                            categories: "Chocolatey".to_string(),
                            install_location: None,
                            data_paths: Vec::new(),
                            exec_cmd: None,
                            uninstall_cmd: None,
                            desktop_file: None,
                            update_available: None,
                            user_installed: true,
                            can_launch: false,
                            can_update: true,
                            can_uninstall: true,
                        });
                    }
                }
            }
        }
    }

    apps
}

fn discover_windows_pwas() -> Vec<AppEntry> {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    let start_menu = appdata.join("Microsoft\\Windows\\Start Menu\\Programs");
    let dirs = [
        (start_menu.join("Chrome Apps"), "Chrome Web App"),
        (start_menu.join("Edge Apps"), "Edge Web App"),
    ];
    let mut apps = Vec::new();

    for (dir, origin) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("lnk") {
                continue;
            }
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Web App")
                .to_string();
            let path_str = p.display().to_string();
            apps.push(AppEntry {
                id: path_str.clone(),
                name,
                source: AppSource::ChromePwa,
                version: "—".to_string(),
                size_bytes: None,
                size_display: "—".to_string(),
                publisher_or_origin: origin.to_string(),
                description: format!("Browser shortcut ({})", path_str),
                categories: "Web App".to_string(),
                install_location: Some(dir.display().to_string()),
                data_paths: Vec::new(),
                exec_cmd: Some(path_str.clone()),
                uninstall_cmd: Some(path_str.clone()),
                desktop_file: Some(path_str),
                update_available: None,
                user_installed: true,
                can_launch: true,
                can_update: false,
                can_uninstall: true,
            });
        }
    }
    apps
}

fn discover_repos() -> Vec<RepoEntry> {
    let mut repos = Vec::new();
    if cmd_exists("winget") {
        if let Ok(out) = hidden_cmd("winget").args(["source", "list"]).output() {
            if out.status.success() {
                let mut past_sep = false;
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let t = line.trim();
                    if t.starts_with("---") {
                        past_sep = true;
                        continue;
                    }
                    if !past_sep || t.is_empty() {
                        continue;
                    }
                    let cols: Vec<&str> = t.split_whitespace().collect();
                    if !cols.is_empty() {
                        let name = cols[0].to_string();
                        let url = cols.get(1).copied().unwrap_or("").to_string();
                        repos.push(RepoEntry {
                            id: name.clone(),
                            name,
                            source: "Winget".to_string(),
                            url_or_details: url,
                            enabled: true,
                        });
                    }
                }
            }
        }
    }
    repos
}

pub fn discover_all(tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut batch = discover_windows_pwas();
        batch.extend(discover_registry_win32());
        let repos = discover_repos();
        let _ = tx.send(AppsEvent::DiscoveryBatch {
            apps: batch,
            repos,
            done: false,
            status: "Scanning Store & Winget packages…".to_string(),
        });
        ctx.request_repaint();

        let mut batch2 = discover_appx();
        batch2.extend(discover_winget());
        batch2.extend(discover_scoop_and_choco());
        let _ = tx.send(AppsEvent::DiscoveryBatch {
            apps: batch2,
            repos: Vec::new(),
            done: true,
            status: "Ready".to_string(),
        });
        ctx.request_repaint();
    });
}

pub fn check_updates(tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut updates: HashMap<String, String> = HashMap::new();
        if cmd_exists("winget") {
            if let Ok(out) = hidden_cmd("winget")
                .args(["upgrade", "--accept-source-agreements"])
                .output()
            {
                if out.status.success() {
                    let mut past_sep = false;
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let t = line.trim();
                        if t.starts_with("---") {
                            past_sep = true;
                            continue;
                        }
                        if !past_sep || t.is_empty() {
                            continue;
                        }
                        let cols: Vec<&str> = t
                            .split("  ")
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .collect();
                        if cols.len() >= 4 {
                            updates.insert(cols[1].to_string(), cols[3].to_string());
                        }
                    }
                }
            }
        }
        let count = updates.len();
        let status = if count == 0 {
            "All Winget packages are up to date.".to_string()
        } else {
            format!("{} update(s) available.", count)
        };
        let _ = tx.send(AppsEvent::UpdatesChecked { updates, status });
        ctx.request_repaint();
    });
}

pub fn resolve_details(app: AppEntry, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut data_paths = app.data_paths.clone();
        populate_data_path_sizes(&mut data_paths);
        let _ = tx.send(AppsEvent::DetailsResolved {
            id: app.id,
            source: app.source,
            install_location: app.install_location,
            data_paths,
        });
        ctx.request_repaint();
    });
}

pub fn launch_app(app: &AppEntry) -> Result<(), String> {
    match app.source {
        AppSource::Appx => {
            if let Some(ref shell_uri) = app.exec_cmd {
                hidden_cmd("explorer.exe")
                    .arg(shell_uri)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to launch Appx: {}", e))?;
                Ok(())
            } else {
                Err("No AppsFolder URI available.".to_string())
            }
        }
        AppSource::ChromePwa => {
            if let Some(ref lnk) = app.exec_cmd {
                hidden_cmd("cmd")
                    .args(["/C", "start", "", lnk])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to launch Web App: {}", e))?;
                Ok(())
            } else {
                Err("No shortcut path available.".to_string())
            }
        }
        _ => Err("Launch is not supported for this package type.".to_string()),
    }
}

pub fn open_app_location(app: &AppEntry) -> Result<(), String> {
    if let Some(ref loc) = app.install_location {
        return open_folder_in_host(Path::new(loc));
    }
    if let Some(ref lnk) = app.desktop_file {
        return open_folder_in_host(Path::new(lnk));
    }
    Err("Install location not available.".to_string())
}

pub fn run_update(app: AppEntry, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(AppsEvent::TaskStarted {
            title: format!("Updating {}", app.name),
        });
        ctx.request_repaint();

        let res = match app.source {
            AppSource::Winget => {
                let mut cmd = hidden_cmd("winget");
                cmd.args([
                    "upgrade",
                    "--id",
                    &app.id,
                    "--exact",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Scoop => {
                let mut cmd = hidden_cmd("scoop");
                cmd.args(["update", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Choco => {
                let mut cmd = hidden_cmd("powershell");
                cmd.args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Start-Process choco -ArgumentList 'upgrade','{}','-y' -Verb RunAs -Wait",
                        app.id.replace('\'', "''")
                    ),
                ]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            _ => Err("Update not supported for this item.".to_string()),
        };

        let (success, summary) = match res {
            Ok(true) => (true, format!("Updated {} successfully.", app.name)),
            Ok(false) => (false, format!("Update failed for {}.", app.name)),
            Err(e) => (false, e),
        };
        let _ = tx.send(AppsEvent::TaskFinished {
            success,
            summary,
            refresh_needed: success,
        });
        ctx.request_repaint();
    });
}

pub fn run_update_all(apps: Vec<AppEntry>, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(AppsEvent::TaskStarted {
            title: format!("Updating {} Winget package(s)", apps.len()),
        });
        ctx.request_repaint();

        let mut cmd = hidden_cmd("winget");
        cmd.args([
            "upgrade",
            "--all",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ]);
        let ok = matches!(stream_command_output(&mut cmd, &tx, &ctx), Ok(true));
        let _ = tx.send(AppsEvent::TaskFinished {
            success: ok,
            summary: if ok {
                "All Winget updates completed.".to_string()
            } else {
                "Winget update completed with errors.".to_string()
            },
            refresh_needed: true,
        });
        ctx.request_repaint();
    });
}

pub fn run_uninstall(app: AppEntry, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(AppsEvent::TaskStarted {
            title: format!("Uninstalling {}", app.name),
        });
        ctx.request_repaint();

        if app.source == AppSource::ChromePwa {
            let mut ok = true;
            if let Some(ref p) = app.desktop_file {
                if let Err(e) = std::fs::remove_file(p) {
                    let _ = tx.send(AppsEvent::TaskOutput(format!("[error] {}", e)));
                    ok = false;
                }
            }
            let _ = tx.send(AppsEvent::TaskFinished {
                success: ok,
                summary: if ok {
                    format!("Removed Web App shortcut {}.", app.name)
                } else {
                    format!("Failed to remove Web App {}.", app.name)
                },
                refresh_needed: ok,
            });
            ctx.request_repaint();
            return;
        }

        let res = match app.source {
            AppSource::Winget => {
                let mut cmd = hidden_cmd("winget");
                cmd.args(["uninstall", "--id", &app.id, "--exact"]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Appx => {
                let mut cmd = hidden_cmd("powershell");
                cmd.args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Remove-AppxPackage -Package '{}'",
                        app.id.replace('\'', "''")
                    ),
                ]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Scoop => {
                let mut cmd = hidden_cmd("scoop");
                cmd.args(["uninstall", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Choco => {
                let mut cmd = hidden_cmd("powershell");
                cmd.args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Start-Process choco -ArgumentList 'uninstall','{}','-y' -Verb RunAs -Wait",
                        app.id.replace('\'', "''")
                    ),
                ]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Win32 => {
                if let Some(ref u) = app.uninstall_cmd {
                    let mut cmd = hidden_cmd("powershell");
                    cmd.args([
                        "-NoProfile",
                        "-Command",
                        &format!(
                            "Start-Process cmd -ArgumentList '/C', '{}' -Verb RunAs -Wait",
                            u.replace('\'', "''")
                        ),
                    ]);
                    stream_command_output(&mut cmd, &tx, &ctx)
                } else {
                    Err("No UninstallString available.".to_string())
                }
            }
            _ => Err("Uninstall not supported for this item.".to_string()),
        };

        let (success, summary) = match res {
            Ok(true) => (true, format!("Uninstalled {} successfully.", app.name)),
            Ok(false) => (false, format!("Uninstall failed for {}.", app.name)),
            Err(e) => (false, e),
        };
        let _ = tx.send(AppsEvent::TaskFinished {
            success,
            summary,
            refresh_needed: success,
        });
        ctx.request_repaint();
    });
}

pub fn run_clear_data(app: AppEntry, mode: ClearDataMode, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(AppsEvent::TaskStarted {
            title: format!("Clearing data for {}", app.name),
        });
        ctx.request_repaint();
        super::actions::clear_app_data(&app, mode, &tx, &ctx);
    });
}
