use serde_json::{json, Value};

async fn on_worker<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("Worker failed: {error}"))?
}

// -------------------------------------------------------------
// Window Management Commands (Tauri 2 borderless window controls)
// -------------------------------------------------------------
#[tauri::command]
pub fn app_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn app_toggle_maximize(window: tauri::Window) {
    if let Ok(is_max) = window.is_maximized() {
        if is_max {
            let _ = window.unmaximize();
        } else {
            let _ = window.maximize();
        }
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn app_close(window: tauri::Window) {
    let _ = window.close();
}

// -------------------------------------------------------------
// Server Launcher Data & Process Lifecycle Commands
// -------------------------------------------------------------
#[tauri::command]
pub fn launcher_get_data() -> Result<Value, String> {
    let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
    let lan_ip = crate::scanner::get_lan_ip();
    Ok(json!({
        "lan_ip": lan_ip,
        "servers": config.servers,
        "ssh_hosts": config.ssh_hosts,
        "adb_devices": config.adb_devices,
        "scripts": config.scripts,
        "saved_commands": config.saved_commands,
        "theme": config.theme,
        "accent_color": config.accent_color,
        "app_autostart": config.app_autostart,
    }))
}

#[tauri::command]
pub async fn launcher_save_server(server: Value) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        let new_srv: crate::config::ServerConfig = serde_json::from_value(server)
            .map_err(|e| format!("Invalid server format: {e}"))?;
        if let Some(pos) = config.servers.iter().position(|s| s.key == new_srv.key) {
            config.servers[pos] = new_srv;
        } else {
            config.servers.push(new_srv);
        }
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Server configuration saved successfully".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_delete_server(key: String) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        config.servers.retain(|s| s.key != key);
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to delete server: {e}"))?;
        Ok("Server deleted".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_start_server(key: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(srv) = config.servers.iter().find(|s| s.key == key) {
            let cwd = if srv.cwd.is_empty() { "." } else { &srv.cwd };
            #[cfg(target_os = "linux")]
            {
                let mut cmd = std::process::Command::new("bash");
                cmd.args(["-c", &srv.command])
                    .current_dir(cwd)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                cmd.spawn().map_err(|e| format!("Failed to spawn process: {e}"))?;
            }
            Ok(format!("Started server: {}", srv.name))
        } else {
            Err("Server not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn launcher_stop_server(key: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(srv) = config.servers.iter().find(|s| s.key == key) {
            if !srv.stop_command.is_empty() {
                let _ = std::process::Command::new("bash").args(["-c", &srv.stop_command]).output();
            } else if srv.port > 0 {
                let _ = std::process::Command::new("fuser").args(["-k", &format!("{}/tcp", srv.port)]).output();
            }
            Ok(format!("Stopped server: {}", srv.name))
        } else {
            Err("Server not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn launcher_scan_listeners() -> Result<Value, String> {
    on_worker(move || {
        let listeners = crate::scanner::scan_system_listening_ports();
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        Ok(json!({
            "timestamp": now,
            "listeners": listeners.into_iter().map(|l| json!({
                "proto": l.proto,
                "port": l.port,
                "pid": l.pid,
                "name": l.name
            })).collect::<Vec<_>>()
        }))
    }).await
}

#[tauri::command]
pub async fn launcher_open_terminal(cwd: String, command: Option<String>) -> Result<String, String> {
    on_worker(move || {
        #[cfg(target_os = "linux")]
        {
            let mut cmd = std::process::Command::new("x-terminal-emulator");
            if !cwd.is_empty() {
                cmd.current_dir(&cwd);
            }
            if let Some(c) = command {
                cmd.args(["-e", "bash", "-c", &format!("{}; exec bash", c)]);
            }
            let _ = cmd.spawn();
            Ok("Terminal opened".to_string())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok("Terminal opened".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn launcher_open_folder(path: String) -> Result<String, String> {
    on_worker(move || {
        let p = if path.is_empty() { "." } else { &path };
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(p).spawn();
        }
        Ok("Folder opened".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_open_browser(url: String) -> Result<String, String> {
    on_worker(move || {
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
        Ok("Browser opened".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_get_sensors() -> Result<Value, String> {
    on_worker(move || {
        let mut sys = sysinfo::System::new_all();
        sys.refresh_all();
        let cpu_usage = sys.global_cpu_usage();
        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();
        let mem_pct = if total_mem > 0 { (used_mem as f32 / total_mem as f32) * 100.0 } else { 0.0 };
        let cpus: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

        Ok(json!({
            "cpu_global": cpu_usage,
            "cpu_cores": cpus,
            "memory_used_mb": used_mem / (1024 * 1024),
            "memory_total_mb": total_mem / (1024 * 1024),
            "memory_pct": mem_pct
        }))
    }).await
}

#[tauri::command]
pub async fn launcher_list_systemd() -> Result<Value, String> {
    on_worker(move || {
        let svcs = crate::systemd::list_local_services(false).unwrap_or_default();
        Ok(json!(svcs.into_iter().take(40).map(|s| json!({
            "name": s.name,
            "description": s.description,
            "load_state": s.load_state,
            "active_state": s.active_state,
            "sub_state": s.sub_state,
            "is_active": s.is_active(),
            "is_failed": s.is_failed()
        })).collect::<Vec<_>>()))
    }).await
}

#[tauri::command]
pub async fn launcher_control_systemd(service: String, action: String) -> Result<String, String> {
    on_worker(move || {
        crate::systemd::control_local_service(&service, &action, false, None)
    }).await
}

#[tauri::command]
pub async fn launcher_execute_script(id: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(script) = config.scripts.iter().find(|s| s.id == id) {
            let out = std::process::Command::new(&script.shell)
                .args(["-c", &script.script])
                .current_dir(if script.cwd.is_empty() { "." } else { &script.cwd })
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("Failed to run script: {e}"))?;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                Ok(if stdout.is_empty() { "Script executed successfully with code 0".to_string() } else { stdout })
            } else {
                Err(format!("Script failed (exit code {:?}): {}", out.status.code(), stderr))
            }
        } else {
            Err("Script not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn launcher_execute_command(command: String, cwd: Option<String>) -> Result<String, String> {
    on_worker(move || {
        let dir = cwd.unwrap_or_else(|| ".".to_string());
        let out = std::process::Command::new("bash")
            .args(["-c", &command])
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("Failed to execute command: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if out.status.success() {
            Ok(if stdout.is_empty() { "Command succeeded (exit 0)".to_string() } else { stdout })
        } else {
            Err(format!("Command failed (code {:?}): {}", out.status.code(), stderr))
        }
    }).await
}

#[tauri::command]
pub async fn launcher_get_detailed_sensors() -> Result<Value, String> {
    on_worker(move || {
        let mut collector = crate::sensors::SystemSensorsCollector::new();
        let snap = collector.collect();
        Ok(json!({
            "cpu_brand": snap.cpu_brand,
            "cpu_cores_physical": snap.cpu_cores_physical,
            "cpu_cores_logical": snap.cpu_cores_logical,
            "cpu_global": snap.cpu_global_usage,
            "cpu_cores": snap.cpu_core_usages,
            "cpu_freq_mhz": snap.cpu_frequency_mhz,
            "cpu_temp": snap.cpu_temp.unwrap_or(0.0),
            "mem_total_mb": snap.mem_total_bytes / (1024 * 1024),
            "mem_used_mb": snap.mem_used_bytes / (1024 * 1024),
            "mem_pct": snap.mem_usage_pct,
            "swap_total_mb": snap.swap_total_bytes / (1024 * 1024),
            "swap_used_mb": snap.swap_used_bytes / (1024 * 1024),
            "swap_pct": snap.swap_usage_pct,
            "disks": snap.disks.into_iter().map(|d| json!({
                "name": d.name,
                "mount_point": d.mount_point,
                "fs_type": d.fs_type,
                "total_gb": d.total_bytes / (1024 * 1024 * 1024),
                "used_gb": d.used_bytes / (1024 * 1024 * 1024),
                "pct": d.usage_pct
            })).collect::<Vec<_>>(),
            "networks": snap.networks.into_iter().map(|n| json!({
                "name": n.name,
                "rx_rate_bps": n.rx_rate_bps,
                "tx_rate_bps": n.tx_rate_bps,
                "is_wireless": n.is_wireless
            })).collect::<Vec<_>>(),
            "top_apps": snap.top_apps.into_iter().take(20).map(|p| json!({
                "pid": p.pid,
                "name": p.name,
                "cpu": p.cpu_usage,
                "memory_mb": p.memory_bytes / (1024 * 1024),
                "status": p.status
            })).collect::<Vec<_>>(),
            "gpus": snap.gpus.into_iter().map(|g| json!({
                "name": g.name,
                "load": g.load_pct,
                "temp": g.temp,
                "driver": g.driver
            })).collect::<Vec<_>>(),
            "uptime_secs": snap.uptime_secs
        }))
    }).await
}

#[tauri::command]
pub async fn launcher_kill_process(pid: u32, force: bool) -> Result<String, String> {
    on_worker(move || {
        let sig = if force { "-9" } else { "-15" };
        let out = std::process::Command::new("kill")
            .args([sig, &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to send signal: {e}"))?;
        if out.status.success() {
            Ok(format!("Terminated process {pid}"))
        } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string())
        }
    }).await
}

#[tauri::command]
pub async fn launcher_systemd_journal(service: String, lines: Option<u32>) -> Result<String, String> {
    on_worker(move || {
        let n = lines.unwrap_or(50);
        let out = std::process::Command::new("journalctl")
            .args(["-u", &service, "-n", &n.to_string(), "--no-pager"])
            .output()
            .map_err(|e| format!("Failed to read journalctl: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_save_script(script: Value) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        let new_s: crate::config::CustomScript = serde_json::from_value(script)
            .map_err(|e| format!("Invalid script format: {e}"))?;
        if let Some(pos) = config.scripts.iter().position(|s| s.id == new_s.id) {
            config.scripts[pos] = new_s;
        } else {
            config.scripts.push(new_s);
        }
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Script saved".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_delete_script(id: String) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        config.scripts.retain(|s| s.id != id);
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Script deleted".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_save_command(command: Value) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        let new_c: crate::config::SavedCommand = serde_json::from_value(command)
            .map_err(|e| format!("Invalid command format: {e}"))?;
        if let Some(pos) = config.saved_commands.iter().position(|c| c.id == new_c.id) {
            config.saved_commands[pos] = new_c;
        } else {
            config.saved_commands.push(new_c);
        }
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Command saved".to_string())
    }).await
}

#[tauri::command]
pub async fn launcher_delete_command(id: String) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        config.saved_commands.retain(|c| c.id != id);
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Command deleted".to_string())
    }).await
}

#[tauri::command]
pub fn launcher_get_autostart() -> bool {
    crate::autostart::is_app_autostart_enabled()
}

#[tauri::command]
pub fn launcher_set_autostart(enabled: bool) -> Result<String, String> {
    let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
    config.app_autostart = enabled;
    let _ = config.save_to_file(&crate::config::ConfigFile::default_path());
    crate::autostart::set_app_autostart(enabled)?;
    Ok(format!("Autostart set to {enabled}"))
}

// -------------------------------------------------------------
// SSH Remote Host Commands
// -------------------------------------------------------------
#[tauri::command]
pub async fn ssh_test_connection(host_id: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            crate::remote::test_ssh_connection(host)
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_fetch_listeners(host_id: String) -> Result<Value, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            let listeners = crate::remote::fetch_remote_listeners(host)?;
            Ok(json!(listeners))
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_open_terminal(host_id: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            crate::remote::launch_external_ssh_terminal(host)?;
            Ok("SSH terminal opened".to_string())
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_execute_command(host_id: String, command: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            let mut cmd = crate::remote::build_ssh_command(host, &command);
            let out = cmd.output().map_err(|e| format!("SSH execution error: {e}"))?;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                Ok(if stdout.is_empty() { "Command completed (exit 0)".to_string() } else { stdout })
            } else {
                Err(format!("Error (code {:?}): {}", out.status.code(), stderr))
            }
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_reboot(host_id: String, sudo_password: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            crate::remote::reboot_remote_device(host, &sudo_password)
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_shutdown(host_id: String, sudo_password: String) -> Result<String, String> {
    on_worker(move || {
        let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        if let Some(host) = config.ssh_hosts.iter().find(|h| h.id == host_id) {
            crate::remote::shutdown_remote_device(host, &sudo_password)
        } else {
            Err("Host not found".to_string())
        }
    }).await
}

#[tauri::command]
pub async fn ssh_save_host(host: Value) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        let new_h: crate::config::SshRemoteHost = serde_json::from_value(host)
            .map_err(|e| format!("Invalid host data: {e}"))?;
        if let Some(pos) = config.ssh_hosts.iter().position(|h| h.id == new_h.id) {
            config.ssh_hosts[pos] = new_h;
        } else {
            config.ssh_hosts.push(new_h);
        }
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Host saved".to_string())
    }).await
}

#[tauri::command]
pub async fn ssh_delete_host(host_id: String) -> Result<String, String> {
    on_worker(move || {
        let mut config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
        config.ssh_hosts.retain(|h| h.id != host_id);
        config.save_to_file(&crate::config::ConfigFile::default_path())
            .map_err(|e| format!("Failed to save config: {e}"))?;
        Ok("Host deleted".to_string())
    }).await
}

// -------------------------------------------------------------
// Existing ADB Cockpit Commands (Fully Preserved)
// -------------------------------------------------------------
#[tauri::command]
async fn adb_devices() -> Result<Vec<crate::adb::AdbDeviceInfo>, String> {
    on_worker(|| {
        crate::adb::adb_cmd(None, &["version"])?;
        Ok(crate::adb::list_devices())
    })
    .await
}

#[tauri::command]
fn adb_initial_serial() -> Option<String> {
    std::env::var("SERVER_LAUNCHER_ADB_SERIAL").ok()
}

#[derive(serde::Serialize)]
struct AdbThemeInfo {
    theme: String,
    is_dark: bool,
    bg: String,
    side: String,
    surface: String,
    surface2: String,
    well: String,
    line: String,
    text: String,
    muted: String,
    dim: String,
    accent: String,
    accent_strong: String,
    accent_soft: String,
    accent_text: String,
}

#[tauri::command]
fn adb_theme_info() -> AdbThemeInfo {
    let config = crate::config::ConfigFile::load_from_file(&crate::config::ConfigFile::default_path());
    let theme_mode = config.theme
        .as_deref()
        .map(crate::theme::ThemeMode::from_id_str)
        .unwrap_or(crate::theme::ThemeMode::NocturnalCommand);
    let mut palette = theme_mode.palette();

    if let Some(ref acc_str) = config.accent_color {
        if acc_str == "system" || acc_str == "gtk4" {
            palette.apply_accent(crate::theme::current_gtk4_accent_color());
        } else if let Some(c) = crate::theme::parse_hex_color(acc_str) {
            palette.apply_accent(c);
        }
    } else {
        palette.apply_accent(crate::theme::current_gtk4_accent_color());
    }

    let hex = |c: egui::Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    let rgba = |c: egui::Color32, a: f32| format!("rgba({}, {}, {}, {:.2})", c.r(), c.g(), c.b(), a);

    AdbThemeInfo {
        theme: theme_mode.id_str().to_string(),
        is_dark: palette.is_dark,
        bg: hex(palette.bg_app),
        side: hex(palette.bg_sidebar),
        surface: hex(palette.bg_card),
        surface2: hex(palette.bg_button),
        well: hex(palette.bg_subtle),
        line: "transparent".into(),
        text: hex(palette.text_primary),
        muted: hex(palette.text_secondary),
        dim: hex(palette.text_muted),
        accent: hex(palette.accent),
        accent_strong: hex(palette.accent_hover),
        accent_soft: rgba(palette.accent, 0.16),
        accent_text: hex(palette.accent_text),
    }
}

#[tauri::command]
fn adb_accent_color() -> Option<String> {
    std::env::var("SERVER_LAUNCHER_ADB_ACCENT").ok()
}

#[tauri::command]
async fn adb_telemetry(serial: String) -> Result<crate::adb::AdbDeviceTelemetry, String> {
    on_worker(move || Ok(crate::adb::query_telemetry(&serial))).await
}

#[tauri::command]
async fn adb_data(serial: String, section: String, path: Option<String>) -> Result<Value, String> {
    on_worker(move || {
        let serial = serial.as_str();
        let data = match section.as_str() {
            "messages" => json!(crate::adb::get_sms_messages(Some(serial)).into_iter().map(|m| json!({
                "id":m.id,"address":m.address,"contact":m.contact_name,"text":m.text,"time":m.time_str,"outgoing":m.is_outgoing
            })).collect::<Vec<_>>()),
            "calls" => json!(crate::adb::get_call_logs(Some(serial)).into_iter().map(|c| json!({
                "id":c.id,"number":c.number,"name":c.name,"kind":c.call_type.label(),"time":c.time_str,"duration":c.duration_str
            })).collect::<Vec<_>>()),
            "contacts" => json!(crate::adb::get_contacts(Some(serial))?.into_iter().map(|c| json!({
                "id":c.id,"name":c.name,"number":c.number
            })).collect::<Vec<_>>()),
            "photos" => json!(crate::adb::list_photos(Some(serial)).into_iter().map(|p| json!({
                "name":p.name,"path":p.path,"size":p.size_str,"date":p.date_str
            })).collect::<Vec<_>>()),
            "notifications" => json!(crate::adb::get_notifications(Some(serial)).into_iter().map(|n| json!({
                "id":n.id,"package":n.package,"app":n.app_name,"title":n.title,"text":n.text,"time":n.time_str
            })).collect::<Vec<_>>()),
            "apps" => json!(crate::adb::list_packages(serial, true).into_iter().map(|p| json!({"name":p.name,"system":p.is_system})).collect::<Vec<_>>()),
            "files" => {
                let path = path.unwrap_or_else(|| "/sdcard".to_string());
                json!(crate::adb::list_directory(serial, &path)?.into_iter().map(|f| json!({
                    "name":f.name,"directory":f.is_dir,"bytes":f.size_bytes,"permissions":f.permissions,"modified":f.modified
                })).collect::<Vec<_>>())
            }
            _ => return Err("Unsupported ADB data section".to_string()),
        };
        Ok(data)
    }).await
}

#[tauri::command]
async fn adb_screenshot(serial: String) -> Result<String, String> {
    on_worker(move || {
        use base64::Engine;
        let png = crate::adb::capture_screen_raw(&serial)?;
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ))
    })
    .await
}

#[tauri::command]
async fn adb_action(
    serial: String,
    action: String,
    value: Option<String>,
    extra: Option<String>,
) -> Result<String, String> {
    on_worker(move || {
        let _value = value.unwrap_or_default();
        let _extra = extra.unwrap_or_default();
        match action.as_str() {
            "key_back" => crate::adb::send_keyevent(&serial, 4).map(|_| "Back sent".into()),
            "key_home" => crate::adb::send_keyevent(&serial, 3).map(|_| "Home sent".into()),
            "key_recents" => crate::adb::send_keyevent(&serial, 187).map(|_| "Recents sent".into()),
            "key_power" => crate::adb::send_keyevent(&serial, 26).map(|_| "Power sent".into()),
            "scrcpy" => crate::adb::launch_scrcpy(&serial, true, false, false, Some(12), Some(8))
                .map(|_| "scrcpy started".into()),
            _ => Err("Unsupported ADB action".to_string()),
        }
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_minimize,
            app_toggle_maximize,
            app_close,
            launcher_get_data,
            launcher_save_server,
            launcher_delete_server,
            launcher_start_server,
            launcher_stop_server,
            launcher_scan_listeners,
            launcher_open_terminal,
            launcher_open_folder,
            launcher_open_browser,
            launcher_get_sensors,
            launcher_list_systemd,
            launcher_control_systemd,
            launcher_execute_script,
            launcher_execute_command,
            launcher_get_detailed_sensors,
            launcher_kill_process,
            launcher_systemd_journal,
            launcher_save_script,
            launcher_delete_script,
            launcher_save_command,
            launcher_delete_command,
            launcher_get_autostart,
            launcher_set_autostart,
            ssh_test_connection,
            ssh_fetch_listeners,
            ssh_open_terminal,
            ssh_execute_command,
            ssh_reboot,
            ssh_shutdown,
            ssh_save_host,
            ssh_delete_host,
            adb_devices,
            adb_initial_serial,
            adb_accent_color,
            adb_theme_info,
            adb_telemetry,
            adb_data,
            adb_screenshot,
            adb_action
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Server Launcher");
}
