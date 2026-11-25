use super::actions::{
    format_bytes, format_unix_timestamp, open_folder_in_host, path_mtime,
    populate_data_path_sizes, stream_command_output,
};
use super::model::{AppDataPath, AppEntry, AppSource, AppsEvent, ClearDataMode, RepoEntry};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn cmd_exists(bin: &str) -> bool {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(bin).is_file() {
                return true;
            }
        }
    }
    false
}

/// Strip XDG `.desktop` Exec field codes (`%u`, `%U`, `%f`, `%F`, `%i`, `%c`, `%k`).
fn clean_desktop_exec(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(&next) = chars.peek() {
                if matches!(next, 'u' | 'U' | 'f' | 'F' | 'i' | 'c' | 'k' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') {
                    chars.next();
                    continue;
                }
                if next == '%' {
                    chars.next();
                    out.push('%');
                    continue;
                }
            }
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Spawn a command completely detached from the launcher process.
fn spawn_detached_shell(cmd_str: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(cmd_str)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.spawn()
            .map_err(|e| format!("Failed to launch '{}': {}", cmd_str, e))?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("Unsupported platform".to_string())
}

fn find_verified_native_data_paths(app_id: &str, name: &str) -> Vec<AppDataPath> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let mut candidates: Vec<String> = Vec::new();
    let clean_id = app_id.trim_end_matches(".desktop").trim();
    if !clean_id.is_empty() {
        candidates.push(clean_id.to_string());
        candidates.push(clean_id.to_lowercase());
    }
    let clean_name = name.trim().to_lowercase().replace(' ', "-");
    if !clean_name.is_empty() && !candidates.contains(&clean_name) {
        candidates.push(clean_name);
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let roots = [
        ("Cache", home.join(".cache"), true),
        ("Config", home.join(".config"), false),
        ("Local Data", home.join(".local/share"), false),
    ];

    for cand in &candidates {
        // Never allow empty or path-traversal candidates
        if cand.is_empty() || cand == "." || cand == ".." || cand.contains('/') {
            continue;
        }
        for (label, root, is_cache) in &roots {
            let p = root.join(cand);
            if p.exists() && seen.insert(p.clone()) {
                out.push(AppDataPath {
                    label: (*label).to_string(),
                    path: p,
                    is_cache: *is_cache,
                    size_bytes: None,
                });
            }
        }
    }
    out
}

fn extract_flag_value(exec: &str, flag: &str) -> Option<String> {
    let idx = exec.find(flag)?;
    let rest = &exec[idx + flag.len()..];
    let val = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next().unwrap_or("")
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        stripped.split('\'').next().unwrap_or("")
    } else {
        rest.split_whitespace().next().unwrap_or("")
    };
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

fn find_pwa_data_paths(exec: &str, desktop_name: &str) -> Vec<AppDataPath> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let app_id = extract_flag_value(exec, "--app-id=");
    let profile = extract_flag_value(exec, "--profile-directory=").unwrap_or_else(|| "Default".to_string());

    let browser_dir = if exec.contains("chromium") || desktop_name.starts_with("chromium-") {
        home.join(".config/chromium")
    } else if exec.contains("brave") || desktop_name.starts_with("brave-") {
        home.join(".config/BraveSoftware/Brave-Browser")
    } else if exec.contains("msedge") || exec.contains("edge") || desktop_name.starts_with("msedge-") {
        home.join(".config/microsoft-edge")
    } else {
        home.join(".config/google-chrome")
    };

    let mut out = Vec::new();
    if let Some(id) = app_id {
        let profile_dir = browser_dir.join(&profile);
        let manifest_dir = profile_dir.join("Web Applications/Manifest Resources").join(&id);
        if manifest_dir.exists() {
            out.push(AppDataPath {
                label: "PWA Manifest Resources".to_string(),
                path: manifest_dir,
                is_cache: false,
                size_bytes: None,
            });
        }
        let app_cache = home
            .join(".cache")
            .join(browser_dir.file_name().unwrap_or_default())
            .join(&profile)
            .join("Cache");
        if app_cache.exists() {
            out.push(AppDataPath {
                label: "Browser Profile Cache".to_string(),
                path: app_cache,
                is_cache: true,
                size_bytes: None,
            });
        }
    }
    out
}

fn desktop_icon(name: &str) -> Option<egui::ColorImage> {
    let path = Path::new(name);
    let mut candidates = Vec::new();
    if path.is_absolute() {
        candidates.push(path.to_owned());
    } else if !name.is_empty() && path.file_name().and_then(|part| part.to_str()) == Some(name) {
        let filename = if name.ends_with(".png") { name.to_owned() } else { format!("{name}.png") };
        let mut roots = vec![PathBuf::from("/usr/share"), PathBuf::from("/usr/local/share"),
            PathBuf::from("/var/lib/flatpak/exports/share")];
        if let Some(home) = home_dir() {
            roots.insert(0, home.join(".local/share/flatpak/exports/share"));
            roots.insert(0, home.join(".local/share"));
        }
        for root in roots {
            for size in ["48x48", "64x64", "32x32", "128x128", "256x256", "512x512"] {
                candidates.push(root.join("icons/hicolor").join(size).join("apps").join(&filename));
            }
            candidates.push(root.join("pixmaps").join(&filename));
        }
    }
    // shortcut: raster artwork only, add theme and SVG resolution when needed.
    candidates.into_iter().find_map(|path| {
        let metadata = std::fs::metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
            return None;
        }
        super::actions::decode_app_icon(&std::fs::read(path).ok()?)
    })
}

fn discover_desktop_entries() -> Vec<AppEntry> {
    let mut dirs: Vec<(PathBuf, bool)> = vec![
        (PathBuf::from("/usr/share/applications"), false),
        (PathBuf::from("/usr/local/share/applications"), false),
        (PathBuf::from("/var/lib/flatpak/exports/share/applications"), false),
    ];
    if let Some(home) = home_dir() {
        dirs.push((home.join(".local/share/applications"), true));
        dirs.push((home.join(".local/share/flatpak/exports/share/applications"), true));
    }

    let mut apps = Vec::new();
    let mut seen_ids = HashSet::new();

    for (dir, is_user_dir) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };

            let mut in_main_section = false;
            let mut name = None;
            let mut icon_name = None;
            let mut exec = None;
            let mut comment = None;
            let mut categories = None;
            let mut no_display = false;
            let mut hidden = false;
            let mut entry_type = None;

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') && line.ends_with(']') {
                    in_main_section = line == "[Desktop Entry]";
                    continue;
                }
                if !in_main_section {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim();
                    let val = v.trim();
                    match key {
                        "Icon" => icon_name = Some(val),
                        "Name" if name.is_none() => name = Some(val.to_string()),
                        "Exec" if exec.is_none() => exec = Some(val.to_string()),
                        "Comment" if comment.is_none() => comment = Some(val.to_string()),
                        "Categories" if categories.is_none() => categories = Some(val.to_string()),
                        "Type" => entry_type = Some(val.to_string()),
                        "NoDisplay" => no_display = val.eq_ignore_ascii_case("true"),
                        "Hidden" => hidden = val.eq_ignore_ascii_case("true"),
                        _ => {}
                    }
                }
            }

            if no_display || hidden {
                continue;
            }
            if entry_type.as_deref() != Some("Application") {
                continue;
            }
            let Some(app_name) = name else {
                continue;
            };
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.desktop")
                .to_string();

            let exec_str = exec.unwrap_or_default();
            let is_pwa = is_user_dir
                && (exec_str.contains("--app-id=")
                    || (file_name.starts_with("chrome-") && file_name.ends_with("-Default.desktop"))
                    || (file_name.starts_with("chromium-") && file_name.ends_with("-Default.desktop"))
                    || (file_name.starts_with("brave-") && file_name.ends_with("-Default.desktop"))
                    || (file_name.starts_with("msedge-") && file_name.ends_with("-Default.desktop")));

            let source = if is_pwa {
                AppSource::ChromePwa
            } else {
                AppSource::Desktop
            };

            let app_id = extract_flag_value(&exec_str, "--app-id=").unwrap_or_else(|| file_name.clone());
            let dedup_key = if is_pwa {
                format!("pwa:{}", app_id)
            } else {
                format!("desktop:{}", file_name)
            };
            if !seen_ids.insert(dedup_key) {
                continue;
            }

            let data_paths = if is_pwa {
                find_pwa_data_paths(&exec_str, &file_name)
            } else {
                find_verified_native_data_paths(&file_name, &app_name)
            };

            let cleaned_exec = clean_desktop_exec(&exec_str);
            let can_launch = !cleaned_exec.is_empty();
            let (installed_date, installed_timestamp) = path_mtime(&path)
                .map(|(d, t)| (Some(d), Some(t)))
                .unwrap_or((None, None));

            apps.push(AppEntry {
                id: app_id,
                name: app_name,
                source,
                version: "—".to_string(),
                size_bytes: None,
                size_display: "—".to_string(),
                publisher_or_origin: if is_pwa {
                    "Browser Web App".to_string()
                } else {
                    dir.display().to_string()
                },
                description: comment.unwrap_or_default(),
                categories: categories.unwrap_or_default(),
                install_location: Some(path.display().to_string()),
                data_paths,
                exec_cmd: if can_launch { Some(cleaned_exec) } else { None },
                uninstall_cmd: if is_pwa {
                    Some(path.display().to_string())
                } else {
                    None
                },
                desktop_file: Some(path.display().to_string()),
                icon: icon_name.and_then(desktop_icon),
                installed_date,
                installed_timestamp,
                update_available: None,
                user_installed: is_user_dir,
                can_launch,
                can_update: false,
                can_uninstall: is_pwa,
            });
        }
    }

    apps
}

fn parse_flatpak_size(s: &str) -> Option<u64> {
    let clean = s.replace('\u{a0}', " ");
    let mut parts = clean.split_whitespace();
    let num_str = parts.next()?.replace(',', ".");
    let unit = parts.next().unwrap_or("B");
    let val: f64 = num_str.parse().ok()?;
    let mult = match unit {
        "B" | "bytes" => 1.0,
        "kB" | "KB" | "KiB" => 1024.0,
        "MB" | "MiB" => 1024.0 * 1024.0,
        "GB" | "GiB" => 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    Some((val * mult) as u64)
}

fn discover_flatpaks() -> Vec<AppEntry> {
    if !cmd_exists("flatpak") {
        return Vec::new();
    }
    let Ok(out) = Command::new("flatpak")
        .args([
            "list",
            "--app",
            "--columns=name,application,version,branch,origin,installation,size",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let home = home_dir();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut apps = Vec::new();

    for line in stdout.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].trim().to_string();
        let app_id = cols[1].trim().to_string();
        let version = cols.get(2).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or("—").to_string();
        let branch = cols.get(3).map(|s| s.trim()).unwrap_or("");
        let origin = cols.get(4).map(|s| s.trim()).unwrap_or("flathub");
        let installation = cols.get(5).map(|s| s.trim()).unwrap_or("system");
        let size_raw = cols.get(6).map(|s| s.trim()).unwrap_or("—");
        let size_bytes = parse_flatpak_size(size_raw);

        let mut data_paths = Vec::new();
        if let Some(ref h) = home {
            let base = h.join(".var/app").join(&app_id);
            for (lbl, sub, is_cache) in [
                ("Flatpak Cache", "cache", true),
                ("Flatpak Config", "config", false),
                ("Flatpak Data", "data", false),
            ] {
                let p = base.join(sub);
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

        let (installed_date, installed_timestamp) = home
            .as_ref()
            .filter(|_| installation == "user")
            .map(|h| h.join(".local/share/flatpak/app").join(&app_id))
            .or_else(|| Some(PathBuf::from("/var/lib/flatpak/app").join(&app_id)))
            .and_then(|p| path_mtime(&p))
            .map(|(d, t)| (Some(d), Some(t)))
            .unwrap_or((None, None));

        apps.push(AppEntry {
            id: app_id.clone(),
            name,
            source: AppSource::Flatpak,
            version,
            size_bytes,
            size_display: size_bytes.map(format_bytes).unwrap_or_else(|| size_raw.to_string()),
            publisher_or_origin: format!("{} ({}, {})", origin, installation, branch),
            description: format!("Flatpak application ({})", app_id),
            categories: "Flatpak".to_string(),
            install_location: None,
            data_paths,
            exec_cmd: Some(format!("flatpak run {}", app_id)),
            uninstall_cmd: None,
            desktop_file: None,
            icon: None,
            installed_date,
            installed_timestamp,
            update_available: None,
            user_installed: installation == "user",
            can_launch: true,
            can_update: true,
            can_uninstall: true,
        });
    }
    apps
}

fn discover_rpm_dnf() -> Vec<AppEntry> {
    if !cmd_exists("rpm") {
        return Vec::new();
    }
    let mut user_installed = HashSet::new();
    if cmd_exists("dnf") {
        if let Ok(out) = Command::new("dnf")
            .args(["repoquery", "--userinstalled", "--qf", "%{NAME}"])
            .output()
        {
            if out.status.success() {
                for l in String::from_utf8_lossy(&out.stdout).lines() {
                    let t = l.trim();
                    if !t.is_empty() {
                        user_installed.insert(t.to_string());
                    }
                }
            }
        }
    }

    let Ok(out) = Command::new("rpm")
        .args([
            "-qa",
            "--qf",
            "%{NAME}\t%{VERSION}-%{RELEASE}\t%{SIZE}\t%{INSTALLTIME}\t%{SUMMARY}\n",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let mut apps = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].trim().to_string();
        let version = cols[1].trim().to_string();
        let size_bytes = cols.get(2).and_then(|s| s.trim().parse::<u64>().ok());
        let install_ts = cols.get(3).and_then(|s| s.trim().parse::<i64>().ok());
        let installed_date = install_ts.map(format_unix_timestamp);
        let summary = cols.get(4).map(|s| s.trim()).unwrap_or("").to_string();
        let is_user = user_installed.contains(&name);

        apps.push(AppEntry {
            id: name.clone(),
            name: name.clone(),
            source: AppSource::DnfRpm,
            version,
            size_bytes,
            size_display: size_bytes.map(format_bytes).unwrap_or_else(|| "—".to_string()),
            publisher_or_origin: if is_user {
                "DNF (User Installed)".to_string()
            } else {
                "RPM System Package".to_string()
            },
            description: summary,
            categories: "System Package".to_string(),
            install_location: None,
            data_paths: find_verified_native_data_paths(&name, &name),
            exec_cmd: None,
            uninstall_cmd: None,
            desktop_file: None,
            icon: None,
            installed_date,
            installed_timestamp: install_ts,
            update_available: None,
            user_installed: is_user,
            can_launch: false,
            can_update: cmd_exists("dnf"),
            can_uninstall: cmd_exists("dnf"),
        });
    }
    apps
}

fn discover_apt_dpkg() -> Vec<AppEntry> {
    if !cmd_exists("dpkg-query") {
        return Vec::new();
    }
    let Ok(out) = Command::new("dpkg-query")
        .args([
            "-W",
            "-f=${Package}\t${Version}\t${Installed-Size}\t${binary:Summary}\n",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let mut apps = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].trim().to_string();
        let version = cols[1].trim().to_string();
        let size_bytes = cols
            .get(2)
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|kb| kb.saturating_mul(1024));
        let summary = cols.get(3).map(|s| s.trim()).unwrap_or("").to_string();

        let (installed_date, installed_timestamp) =
            path_mtime(&PathBuf::from(format!("/var/lib/dpkg/info/{}.list", name)))
                .map(|(d, t)| (Some(d), Some(t)))
                .unwrap_or((None, None));

        apps.push(AppEntry {
            id: name.clone(),
            name: name.clone(),
            source: AppSource::AptDpkg,
            version,
            size_bytes,
            size_display: size_bytes.map(format_bytes).unwrap_or_else(|| "—".to_string()),
            publisher_or_origin: "APT / dpkg".to_string(),
            description: summary,
            categories: "System Package".to_string(),
            install_location: None,
            data_paths: find_verified_native_data_paths(&name, &name),
            exec_cmd: None,
            uninstall_cmd: None,
            desktop_file: None,
            icon: None,
            installed_date,
            installed_timestamp,
            update_available: None,
            user_installed: true,
            can_launch: false,
            can_update: cmd_exists("apt-get"),
            can_uninstall: cmd_exists("apt-get"),
        });
    }
    apps
}

fn discover_pacman() -> Vec<AppEntry> {
    if !cmd_exists("pacman") {
        return Vec::new();
    }
    let mut explicit = HashSet::new();
    if let Ok(out) = Command::new("pacman").args(["-Qqe"]).output() {
        if out.status.success() {
            for l in String::from_utf8_lossy(&out.stdout).lines() {
                explicit.insert(l.trim().to_string());
            }
        }
    }
    let Ok(out) = Command::new("pacman").args(["-Q"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let mut apps = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let version = parts.next().unwrap_or("—").to_string();
        let is_user = explicit.contains(name);

        apps.push(AppEntry {
            id: name.to_string(),
            name: name.to_string(),
            source: AppSource::Pacman,
            version,
            size_bytes: None,
            size_display: "—".to_string(),
            publisher_or_origin: if is_user {
                "Pacman (Explicit)".to_string()
            } else {
                "Pacman (Dependency)".to_string()
            },
            description: String::new(),
            categories: "System Package".to_string(),
            install_location: None,
            data_paths: find_verified_native_data_paths(name, name),
            exec_cmd: None,
            uninstall_cmd: None,
            desktop_file: None,
            icon: None,
            installed_date: None,
            installed_timestamp: None,
            update_available: None,
            user_installed: is_user,
            can_launch: false,
            can_update: true,
            can_uninstall: true,
        });
    }
    apps
}

fn discover_snaps() -> Vec<AppEntry> {
    if !cmd_exists("snap") {
        return Vec::new();
    }
    let Ok(out) = Command::new("snap").args(["list"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let mut apps = Vec::new();
    for (idx, line) in String::from_utf8_lossy(&out.stdout).lines().enumerate() {
        if idx == 0 {
            continue; // header
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 2 {
            continue;
        }
        let name = cols[0].to_string();
        let version = cols[1].to_string();
        let publisher = cols.get(4).copied().unwrap_or("Snap Store");

        apps.push(AppEntry {
            id: name.clone(),
            name: name.clone(),
            source: AppSource::Snap,
            version,
            size_bytes: None,
            size_display: "—".to_string(),
            publisher_or_origin: format!("Snap ({})", publisher),
            description: format!("Snap package {}", name),
            categories: "Snap".to_string(),
            install_location: Some(format!("/snap/{}", name)),
            data_paths: find_verified_native_data_paths(&name, &name),
            exec_cmd: Some(format!("snap run {}", name)),
            uninstall_cmd: None,
            desktop_file: None,
            icon: None,
            installed_date: None,
            installed_timestamp: None,
            update_available: None,
            user_installed: true,
            can_launch: true,
            can_update: true,
            can_uninstall: true,
        });
    }
    apps
}

fn discover_repos() -> Vec<RepoEntry> {
    let mut repos = Vec::new();

    // 1. DNF repos (Fedora / RHEL)
    if cmd_exists("dnf") {
        if let Ok(out) = Command::new("dnf").args(["repolist", "--all"]).output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty()
                        || trimmed.starts_with("repo id")
                        || trimmed.starts_with("Updating")
                    {
                        continue;
                    }
                    // Columns are whitespace-separated with status at the end (enabled/disabled)
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let id = parts[0].to_string();
                        let last = parts.last().copied().unwrap_or("");
                        let (enabled, name_end) = if last.eq_ignore_ascii_case("enabled") {
                            (true, parts.len() - 1)
                        } else if last.eq_ignore_ascii_case("disabled") {
                            (false, parts.len() - 1)
                        } else {
                            (true, parts.len())
                        };
                        let name = parts[1..name_end].join(" ");
                        repos.push(RepoEntry {
                            id,
                            name,
                            source: "DNF".to_string(),
                            url_or_details: "System DNF Repository".to_string(),
                            enabled,
                        });
                    }
                }
            }
        }
    }

    // 2. Flatpak remotes
    if cmd_exists("flatpak") {
        if let Ok(out) = Command::new("flatpak")
            .args(["remotes", "--columns=name,url,options"])
            .output()
        {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let cols: Vec<&str> = line.split('\t').collect();
                    if cols.is_empty() || cols[0].trim().is_empty() {
                        continue;
                    }
                    let id = cols[0].trim().to_string();
                    let url = cols.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                    let opts = cols.get(2).map(|s| s.trim()).unwrap_or("");
                    let enabled = !opts.contains("disabled");
                    repos.push(RepoEntry {
                        id: id.clone(),
                        name: id,
                        source: "Flatpak".to_string(),
                        url_or_details: if opts.is_empty() {
                            url
                        } else {
                            format!("{} ({})", url, opts)
                        },
                        enabled,
                    });
                }
            }
        }
    }

    // 3. Debian/Ubuntu APT sources
    let apt_main = Path::new("/etc/apt/sources.list");
    if apt_main.exists() {
        parse_apt_list(apt_main, &mut repos);
    }
    let apt_dir = Path::new("/etc/apt/sources.list.d");
    if let Ok(entries) = std::fs::read_dir(apt_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("list") {
                parse_apt_list(&p, &mut repos);
            }
        }
    }

    // 4. Arch pacman.conf repos
    let pacman_conf = Path::new("/etc/pacman.conf");
    if pacman_conf.exists() {
        if let Ok(content) = std::fs::read_to_string(pacman_conf) {
            for line in content.lines() {
                let t = line.trim();
                if t.starts_with('[') && t.ends_with(']') && t != "[options]" {
                    let name = t[1..t.len() - 1].to_string();
                    repos.push(RepoEntry {
                        id: name.clone(),
                        name,
                        source: "Pacman".to_string(),
                        url_or_details: "/etc/pacman.conf".to_string(),
                        enabled: true,
                    });
                }
            }
        }
    }

    repos
}

fn parse_apt_list(path: &Path, repos: &mut Vec<RepoEntry>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let file_lbl = path.file_name().and_then(|n| n.to_str()).unwrap_or("sources.list");
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with("deb ") || t.starts_with("deb-src ") {
            repos.push(RepoEntry {
                id: format!("{}:{}", file_lbl, idx + 1),
                name: file_lbl.to_string(),
                source: "APT".to_string(),
                url_or_details: t.to_string(),
                enabled: true,
            });
        }
    }
}

pub fn discover_all(tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        // Fast batch 1: GUI .desktop apps + PWAs + Flatpaks + Repos
        let mut batch = discover_desktop_entries();
        batch.extend(discover_flatpaks().into_iter().map(|mut app| {
            app.icon = desktop_icon(&app.id);
            app
        }));
        let mut desktop_icons = HashMap::new();
        for app in batch.iter().filter(|app| app.source == AppSource::Desktop) {
            let Some(icon) = &app.icon else { continue };
            if let Some(stem) = app.desktop_file.as_deref().and_then(|path| Path::new(path).file_stem()) {
                desktop_icons.insert(stem.to_string_lossy().into_owned(), icon.clone());
            }
            if let Some(executable) = app.exec_cmd.as_deref().and_then(|exec| exec.split_whitespace().next()) {
                if let Some(name) = Path::new(executable).file_name().and_then(|name| name.to_str()) {
                    desktop_icons.entry(name.to_owned()).or_insert_with(|| icon.clone());
                }
            }
        }
        let repos = discover_repos();
        let _ = tx.send(AppsEvent::DiscoveryBatch {
            apps: batch,
            repos,
            done: false,
            status: "Scanning system packages…".to_string(),
        });
        ctx.request_repaint();

        // Batch 2: Native system packages (RPM/DNF, APT, Pacman, Snap)
        let mut sys_pkgs = Vec::new();
        if cmd_exists("rpm") {
            sys_pkgs.extend(discover_rpm_dnf());
        } else if cmd_exists("dpkg-query") {
            sys_pkgs.extend(discover_apt_dpkg());
        } else if cmd_exists("pacman") {
            sys_pkgs.extend(discover_pacman());
        }
        if cmd_exists("snap") {
            sys_pkgs.extend(discover_snaps());
        }

        for app in &mut sys_pkgs {
            app.icon = desktop_icons.get(&app.id).cloned();
        }
        let _ = tx.send(AppsEvent::DiscoveryBatch {
            apps: sys_pkgs,
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

        // Flatpak updates
        if cmd_exists("flatpak") {
            if let Ok(out) = Command::new("flatpak")
                .args(["remote-ls", "--updates", "--columns=application,version"])
                .output()
            {
                if out.status.success() {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let cols: Vec<&str> = line.split('\t').collect();
                        if !cols.is_empty() && !cols[0].trim().is_empty() {
                            let id = cols[0].trim().to_string();
                            let ver = cols.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or("Update available");
                            updates.insert(id, ver.to_string());
                        }
                    }
                }
            }
        }

        // DNF updates (exit code 100 = updates available)
        if cmd_exists("dnf") {
            if let Ok(out) = Command::new("dnf").args(["check-update", "-q"]).output() {
                if out.status.code() == Some(100) || out.status.success() {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let cols: Vec<&str> = line.split_whitespace().collect();
                        if cols.len() >= 2 && !cols[0].starts_with("Obsoleting") {
                            let pkg = cols[0].split('.').next().unwrap_or(cols[0]).to_string();
                            updates.insert(pkg, cols[1].to_string());
                        }
                    }
                }
            }
        } else if cmd_exists("apt") {
            if let Ok(out) = Command::new("apt").args(["list", "--upgradable"]).output() {
                if out.status.success() {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        if let Some((pkg_part, rest)) = line.split_once('/') {
                            let ver = rest.split_whitespace().nth(1).unwrap_or("Update available");
                            updates.insert(pkg_part.trim().to_string(), ver.to_string());
                        }
                    }
                }
            }
        } else if cmd_exists("pacman") {
            if let Ok(out) = Command::new("pacman").args(["-Qu"]).output() {
                if out.status.success() {
                    for line in String::from_utf8_lossy(&out.stdout).lines() {
                        let cols: Vec<&str> = line.split_whitespace().collect();
                        if cols.len() >= 4 {
                            updates.insert(cols[0].to_string(), cols[3].to_string());
                        }
                    }
                }
            }
        }

        let count = updates.len();
        let status = if count == 0 {
            "All packages are up to date.".to_string()
        } else {
            format!("{} update(s) available.", count)
        };
        let _ = tx.send(AppsEvent::UpdatesChecked { updates, status });
        ctx.request_repaint();
    });
}

pub fn resolve_details(app: AppEntry, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut install_location = app.install_location.clone();
        if install_location.is_none() {
            install_location = match app.source {
                AppSource::Flatpak => Command::new("flatpak")
                    .args(["info", "--show-location", &app.id])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|s| !s.is_empty()),
                AppSource::DnfRpm => Command::new("rpm")
                    .args(["-ql", &app.id])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| pick_primary_install_path(&String::from_utf8_lossy(&o.stdout))),
                AppSource::AptDpkg => Command::new("dpkg")
                    .args(["-L", &app.id])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| pick_primary_install_path(&String::from_utf8_lossy(&o.stdout))),
                AppSource::Pacman => Command::new("pacman")
                    .args(["-Ql", &app.id])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| {
                        let text = String::from_utf8_lossy(&o.stdout);
                        let lines: Vec<&str> = text
                            .lines()
                            .filter_map(|l| l.split_once(' ').map(|(_, p)| p))
                            .collect();
                        pick_primary_install_path(&lines.join("\n"))
                    }),
                _ => None,
            };
        }

        let description = match app.source {
            AppSource::DnfRpm => Command::new("rpm")
                .args(["-qi", &app.id])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| parse_rpm_description(&String::from_utf8_lossy(&o.stdout))),
            AppSource::AptDpkg => Command::new("dpkg-query")
                .args(["-s", &app.id])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| parse_dpkg_description(&String::from_utf8_lossy(&o.stdout))),
            AppSource::Pacman => Command::new("pacman")
                .args(["-Qi", &app.id])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout);
                    s.lines()
                        .find(|l| l.starts_with("Description"))
                        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
                }),
            AppSource::Flatpak => Command::new("flatpak")
                .args(["info", &app.id])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout);
                    s.lines()
                        .find(|l| l.contains(" - "))
                        .map(|l| l.trim().to_string())
                }),
            _ => None,
        };

        let mut data_paths = app.data_paths.clone();
        populate_data_path_sizes(&mut data_paths);

        let _ = tx.send(AppsEvent::DetailsResolved {
            id: app.id,
            source: app.source,
            install_location,
            description,
            installed_date: app.installed_date,
            installed_timestamp: app.installed_timestamp,
            data_paths,
        });
        ctx.request_repaint();
    });
}

fn parse_rpm_description(text: &str) -> Option<String> {
    let mut in_desc = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.starts_with("Description") {
            in_desc = true;
            continue;
        }
        if in_desc {
            lines.push(line.trim());
        }
    }
    let joined = lines.join(" ").trim().to_string();
    if joined.is_empty() { None } else { Some(joined) }
}

fn parse_dpkg_description(text: &str) -> Option<String> {
    let mut in_desc = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Description:") {
            in_desc = true;
            lines.push(rest.trim());
            continue;
        }
        if in_desc {
            if line.starts_with(' ') || line.starts_with('\t') {
                let t = line.trim();
                if t != "." {
                    lines.push(t);
                }
            } else {
                break;
            }
        }
    }
    let joined = lines.join(" ").trim().to_string();
    if joined.is_empty() { None } else { Some(joined) }
}

fn pick_primary_install_path(file_list: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    for line in file_list.lines() {
        let p = line.trim();
        if p.is_empty() || p.starts_with("package ") || p == "/." {
            continue;
        }
        if p.starts_with("/usr/bin/") || p.starts_with("/opt/") || p.starts_with("/usr/lib/") {
            return Some(p.to_string());
        }
        if fallback.is_none() {
            fallback = Some(p.to_string());
        }
    }
    fallback
}

pub fn launch_app(app: &AppEntry) -> Result<(), String> {
    match app.source {
        AppSource::Flatpak => spawn_detached_shell(&format!("flatpak run {}", app.id)),
        AppSource::Snap => spawn_detached_shell(&format!("snap run {}", app.id)),
        AppSource::Desktop | AppSource::ChromePwa => {
            if let Some(ref cmd) = app.exec_cmd {
                spawn_detached_shell(cmd)
            } else {
                Err("No launch command available for this application.".to_string())
            }
        }
        _ => Err("Launch is not supported for this package type.".to_string()),
    }
}

pub fn open_app_location(app: &AppEntry) -> Result<(), String> {
    if let Some(ref loc) = app.install_location {
        return open_folder_in_host(Path::new(loc));
    }
    if let Some(ref desk) = app.desktop_file {
        return open_folder_in_host(Path::new(desk));
    }
    Err("Install location has not been resolved yet.".to_string())
}

pub fn run_update(app: AppEntry, tx: Sender<AppsEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let _ = tx.send(AppsEvent::TaskStarted {
            title: format!("Updating {}", app.name),
        });
        ctx.request_repaint();

        let res = match app.source {
            AppSource::Flatpak => {
                let mut cmd = Command::new("flatpak");
                cmd.args(["update", "-y", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::DnfRpm => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["dnf", "upgrade", "-y", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::AptDpkg => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["apt-get", "install", "-y", "--only-upgrade", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Pacman => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["pacman", "-S", "--noconfirm", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Snap => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["snap", "refresh", &app.id]);
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
            title: format!("Updating {} item(s)", apps.len()),
        });
        ctx.request_repaint();

        let has_flatpak = apps.iter().any(|a| a.source == AppSource::Flatpak);
        let dnf_pkgs: Vec<String> = apps
            .iter()
            .filter(|a| a.source == AppSource::DnfRpm)
            .map(|a| a.id.clone())
            .collect();
        let apt_pkgs: Vec<String> = apps
            .iter()
            .filter(|a| a.source == AppSource::AptDpkg)
            .map(|a| a.id.clone())
            .collect();
        let pacman_pkgs: Vec<String> = apps
            .iter()
            .filter(|a| a.source == AppSource::Pacman)
            .map(|a| a.id.clone())
            .collect();

        let mut all_ok = true;

        if has_flatpak {
            let _ = tx.send(AppsEvent::TaskOutput("==> Running flatpak update -y".to_string()));
            let mut cmd = Command::new("flatpak");
            cmd.args(["update", "-y"]);
            if !matches!(stream_command_output(&mut cmd, &tx, &ctx), Ok(true)) {
                all_ok = false;
            }
        }
        if !dnf_pkgs.is_empty() {
            let _ = tx.send(AppsEvent::TaskOutput("==> Running pkexec dnf upgrade -y".to_string()));
            let mut cmd = Command::new("pkexec");
            cmd.arg("dnf").arg("upgrade").arg("-y").args(&dnf_pkgs);
            if !matches!(stream_command_output(&mut cmd, &tx, &ctx), Ok(true)) {
                all_ok = false;
            }
        }
        if !apt_pkgs.is_empty() {
            let _ = tx.send(AppsEvent::TaskOutput("==> Running pkexec apt-get install -y --only-upgrade".to_string()));
            let mut cmd = Command::new("pkexec");
            cmd.args(["apt-get", "install", "-y", "--only-upgrade"]).args(&apt_pkgs);
            if !matches!(stream_command_output(&mut cmd, &tx, &ctx), Ok(true)) {
                all_ok = false;
            }
        }
        if !pacman_pkgs.is_empty() {
            let _ = tx.send(AppsEvent::TaskOutput("==> Running pkexec pacman -S --noconfirm".to_string()));
            let mut cmd = Command::new("pkexec");
            cmd.args(["pacman", "-S", "--noconfirm"]).args(&pacman_pkgs);
            if !matches!(stream_command_output(&mut cmd, &tx, &ctx), Ok(true)) {
                all_ok = false;
            }
        }

        let _ = tx.send(AppsEvent::TaskFinished {
            success: all_ok,
            summary: if all_ok {
                "All updates completed.".to_string()
            } else {
                "Some updates failed; see task log.".to_string()
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
            if let Some(ref desktop_path) = app.desktop_file {
                let p = Path::new(desktop_path);
                if p.exists() {
                    let _ = tx.send(AppsEvent::TaskOutput(format!("Removing {}", p.display())));
                    if let Err(e) = std::fs::remove_file(p) {
                        let _ = tx.send(AppsEvent::TaskOutput(format!("[error] {}", e)));
                        ok = false;
                    }
                }
            }
            for dp in &app.data_paths {
                if dp.path.exists() {
                    let _ = tx.send(AppsEvent::TaskOutput(format!("Removing {}", dp.path.display())));
                    if let Err(e) = std::fs::remove_dir_all(&dp.path) {
                        let _ = tx.send(AppsEvent::TaskOutput(format!("[error] {}", e)));
                        ok = false;
                    }
                }
            }
            let _ = tx.send(AppsEvent::TaskFinished {
                success: ok,
                summary: if ok {
                    format!("Uninstalled Web App {}.", app.name)
                } else {
                    format!("Failed to completely uninstall Web App {}.", app.name)
                },
                refresh_needed: true,
            });
            ctx.request_repaint();
            return;
        }

        let res = match app.source {
            AppSource::Flatpak => {
                let mut cmd = Command::new("flatpak");
                cmd.args(["uninstall", "-y", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::DnfRpm => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["dnf", "remove", "-y", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::AptDpkg => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["apt-get", "remove", "-y", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Pacman => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["pacman", "-R", "--noconfirm", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
            }
            AppSource::Snap => {
                let mut cmd = Command::new("pkexec");
                cmd.args(["snap", "remove", &app.id]);
                stream_command_output(&mut cmd, &tx, &ctx)
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
