use serde_json::{json, Value};

async fn on_worker<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("ADB worker failed: {error}"))?
}

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
    }

    if let Ok(theme_id) = std::env::var("SERVER_LAUNCHER_ADB_THEME") {
        let tm = crate::theme::ThemeMode::from_id_str(&theme_id);
        palette = tm.palette();
    }
    if let Ok(env_acc) = std::env::var("SERVER_LAUNCHER_ADB_ACCENT") {
        if let Some(c) = crate::theme::parse_hex_color(&env_acc) {
            palette.apply_accent(c);
        }
    }
    if let Ok(env_bg) = std::env::var("SERVER_LAUNCHER_ADB_BG") {
        if let Some(c) = crate::theme::parse_hex_color(&env_bg) {
            palette.bg_app = c;
            palette.bg_main = c;
        }
    }
    if let Ok(env_sb) = std::env::var("SERVER_LAUNCHER_ADB_SIDEBAR") {
        if let Some(c) = crate::theme::parse_hex_color(&env_sb) {
            palette.bg_sidebar = c;
        }
    }
    if let Ok(env_card) = std::env::var("SERVER_LAUNCHER_ADB_CARD") {
        if let Some(c) = crate::theme::parse_hex_color(&env_card) {
            palette.bg_card = c;
        }
    }
    if let Ok(env_btn) = std::env::var("SERVER_LAUNCHER_ADB_BUTTON") {
        if let Some(c) = crate::theme::parse_hex_color(&env_btn) {
            palette.bg_button = c;
        }
    }
    if let Ok(env_well) = std::env::var("SERVER_LAUNCHER_ADB_WELL") {
        if let Some(c) = crate::theme::parse_hex_color(&env_well) {
            palette.bg_subtle = c;
        }
    }
    if let Ok(env_at) = std::env::var("SERVER_LAUNCHER_ADB_ACCENT_TEXT") {
        if let Some(c) = crate::theme::parse_hex_color(&env_at) {
            palette.accent_text = c;
        }
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
            "contacts" => json!(crate::adb::get_contacts(Some(serial)).into_iter().map(|c| json!({
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
            "endpoints" => json!(crate::adb::discover_device_endpoints(serial).into_iter().map(|e| json!({
                "interface":e.interface,"ip":e.ip,"wifi":e.is_wifi,"label":e.label
            })).collect::<Vec<_>>()),
            "sensors" => json!(crate::adb::query_device_sensors(serial)?),
            "sensor_inventory" => json!(crate::adb::query_device_sensor_inventory(serial)?),
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
async fn adb_photo_preview(serial: String, path: String) -> Result<String, String> {
    on_worker(move || {
        use base64::Engine;
        let bytes = crate::adb::read_remote_file(&serial, &path)?;
        let mime = match std::path::Path::new(&path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => "image/jpeg",
        };
        Ok(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
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
        let value = value.unwrap_or_default();
        let extra = extra.unwrap_or_default();
        match action.as_str() {
            "key_back" => crate::adb::send_keyevent(&serial, 4).map(|_| "Back sent".into()),
            "key_home" => crate::adb::send_keyevent(&serial, 3).map(|_| "Home sent".into()),
            "key_recents" => crate::adb::send_keyevent(&serial, 187).map(|_| "Recents sent".into()),
            "key_power" => crate::adb::send_keyevent(&serial, 26).map(|_| "Power sent".into()),
            "key_volume_up" => crate::adb::volume_up(Some(&serial)),
            "key_volume_down" => crate::adb::volume_down(Some(&serial)),
            "reboot" => crate::adb::reboot_device(&serial, &value),
            "shell" => crate::adb::adb_cmd(Some(&serial), &["shell", &value]),
            "send_text" => crate::adb::send_text(&serial, &value).map(|_| "Text sent".into()),
            "call" => crate::adb::start_phone_call(Some(&serial), &value),
            "hangup" => crate::adb::end_phone_call(Some(&serial)),
            "send_sms" => crate::adb::send_sms(Some(&serial), &value, &extra),
            "clipboard_get" => crate::adb::get_clipboard(&serial),
            "clipboard_set" => {
                crate::adb::set_clipboard(&serial, &value).map(|_| "Clipboard sent".into())
            }
            "launch_app" => crate::adb::launch_app(&serial, &value).map(|_| "App launched".into()),
            "stop_app" => crate::adb::force_stop_app(&serial, &value).map(|_| "App stopped".into()),
            "delete_file" => {
                crate::adb::delete_file(&serial, &value).map(|_| "File deleted".into())
            }
            "download_photo" => crate::adb::pull_photo_to_downloads(Some(&serial), &value),
            "delete_photo" => crate::adb::delete_device_photo(Some(&serial), &value),
            "scrcpy" => crate::adb::launch_scrcpy(&serial, true, false, false, Some(12), Some(8))
                .map(|_| "scrcpy started".into()),
            "wireless_on" => crate::adb::enable_tcpip(&serial, 5555),
            "wireless_off" => crate::adb::reset_to_usb(&serial),
            "dnd_on" => crate::adb::toggle_dnd(Some(&serial), true),
            "dnd_off" => crate::adb::toggle_dnd(Some(&serial), false),
            "pick_file" => {
                let mut dialog = rfd::FileDialog::new();
                if !value.is_empty() {
                    let exts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
                    dialog = dialog.add_filter("Files", &exts);
                }
                match dialog.pick_file() {
                    Some(p) => Ok(p.to_string_lossy().to_string()),
                    None => Err("File selection cancelled".to_string()),
                }
            },
            "sideload_ota" => crate::adb::sideload_ota(&serial, std::path::Path::new(&value)),
            "fastboot_flash" => crate::adb::fastboot_flash(&value, std::path::Path::new(&extra)),
            "fastboot_devices" => {
                let devs = crate::adb::list_fastboot_devices();
                if devs.is_empty() {
                    Ok("No fastboot devices detected. (Device must be in bootloader/fastboot mode)".into())
                } else {
                    Ok(format!("Connected fastboot devices: {}", devs.join(", ")))
                }
            },
            "bugreport" => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dest = std::path::PathBuf::from(home).join("Downloads").join(format!("bugreport_{}.zip", serial.replace([':', '.'], "_")));
                crate::adb::generate_bugreport(&serial, &dest).map(|_| format!("Bugreport saved to {}", dest.display()))
            },
            "root_check" => {
                let id_out = crate::adb::adb_cmd(Some(&serial), &["shell", "su", "-c", "id"]).unwrap_or_default();
                let magisk = crate::adb::adb_cmd(Some(&serial), &["shell", "which", "magisk"]).unwrap_or_default();
                let ksu = crate::adb::adb_cmd(Some(&serial), &["shell", "ls", "/data/adb/ksu"]).unwrap_or_default();
                let apatch = crate::adb::adb_cmd(Some(&serial), &["shell", "ls", "/data/adb/ap"]).unwrap_or_default();
                let selinux = crate::adb::adb_cmd(Some(&serial), &["shell", "getenforce"]).unwrap_or_else(|_| "Unknown".into());
                let is_root = id_out.contains("uid=0");
                let mgr = if !magisk.is_empty() && !magisk.contains("not found") {
                    "Magisk"
                } else if !ksu.is_empty() && !ksu.contains("No such") {
                    "KernelSU"
                } else if !apatch.is_empty() && !apatch.contains("No such") {
                    "APatch"
                } else if is_root {
                    "su binary"
                } else {
                    "None (Stock)"
                };
                Ok(format!("Status: {} | Superuser: {} | SELinux: {}", if is_root { "ROOTED (UID 0)" } else { "UNROOTED" }, mgr, selinux.trim()))
            },
            "remount_system" => {
                crate::adb::adb_cmd(Some(&serial), &["remount"])
                    .or_else(|_| crate::adb::adb_cmd(Some(&serial), &["shell", "su", "-c", "mount -o rw,remount /"]))
                    .map(|out| if out.trim().is_empty() { "Remounted /system as Read-Write".into() } else { out })
            },
            "set_selinux" => {
                let mode = if value == "0" || value.to_lowercase().contains("permissive") { "0" } else { "1" };
                crate::adb::adb_cmd(Some(&serial), &["shell", "su", "-c", &format!("setenforce {}", mode)])
                    .map(|_| format!("SELinux set to {}", if mode == "0" { "Permissive (0)" } else { "Enforcing (1)" }))
            },
            "backup_full" => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dest = std::path::PathBuf::from(home).join("Downloads").join(format!("backup_{}.ab", serial.replace([':', '.'], "_")));
                let dest_str = dest.to_string_lossy().to_string();
                crate::adb::adb_cmd(Some(&serial), &["backup", "-apk", "-shared", "-all", "-f", &dest_str])
                    .map(|_| format!("Full backup initiated. Saved to {}. (Confirm on device)", dest.display()))
            },
            "backup_app" => {
                let pkg = value.trim();
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dest = std::path::PathBuf::from(home).join("Downloads").join(format!("backup_{}_{}.ab", pkg, serial.replace([':', '.'], "_")));
                let dest_str = dest.to_string_lossy().to_string();
                crate::adb::adb_cmd(Some(&serial), &["backup", "-apk", pkg, "-f", &dest_str])
                    .map(|_| format!("App backup initiated for {}. Saved to {}", pkg, dest.display()))
            },
            "restore_backup" => {
                crate::adb::adb_cmd(Some(&serial), &["restore", &value])
                    .map(|_| "Backup restore command sent. Confirm restore prompt on phone screen.".into())
            },
            "dump_partition" => {
                let part = value.trim();
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dest = std::path::PathBuf::from(home).join("Downloads").join(format!("{}_{}.img", part, serial.replace([':', '.'], "_")));
                let dest_str = dest.to_string_lossy().to_string();
                let block = format!("/dev/block/by-name/{}", part);
                crate::adb::adb_cmd(Some(&serial), &["shell", "su", "-c", &format!("dd if={} of=/sdcard/dump_{}.img bs=4M", block, part)])?;
                crate::adb::adb_cmd(Some(&serial), &["pull", &format!("/sdcard/dump_{}.img", part), &dest_str])?;
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "rm", "-f", &format!("/sdcard/dump_{}.img", part)]);
                Ok(format!("Partition '{}' dumped successfully to {}", part, dest.display()))
            },
            "set_density" => {
                let dpi = value.trim();
                if dpi == "reset" || dpi.is_empty() {
                    crate::adb::adb_cmd(Some(&serial), &["shell", "wm", "density", "reset"])
                        .map(|_| "Display density reset to default".into())
                } else {
                    crate::adb::adb_cmd(Some(&serial), &["shell", "wm", "density", dpi])
                        .map(|_| format!("Display density set to {} DPI", dpi))
                }
            },
            "set_animations" => {
                let scale = if value.trim().is_empty() { "0.5" } else { value.trim() };
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "settings", "put", "global", "window_animation_scale", scale]);
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "settings", "put", "global", "transition_animation_scale", scale]);
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "settings", "put", "global", "animator_duration_scale", scale]);
                Ok(format!("Animation scales set to {}x", scale))
            },
            "push_photo" => {
                let host_path = std::path::Path::new(&value);
                let fname = if extra.is_empty() {
                    host_path.file_name().and_then(|n| n.to_str()).unwrap_or("photo.jpg")
                } else {
                    &extra
                };
                let dest_device = format!("/sdcard/DCIM/Camera/{}", fname);
                crate::adb::adb_cmd(Some(&serial), &["push", &value, &dest_device])?;
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "am", "broadcast", "-a", "android.intent.action.MEDIA_SCANNER_SCAN_FILE", "-d", &format!("file://{}", dest_device)]);
                Ok(format!("Uploaded photo to {}", dest_device))
            },
            "bluetooth_toggle" => {
                let current = crate::adb::adb_cmd(Some(&serial), &["shell", "settings", "get", "global", "bluetooth_on"]).unwrap_or_default();
                let next = if current.trim() == "1" { "disable" } else { "enable" };
                crate::adb::adb_cmd(Some(&serial), &["shell", "svc", "bluetooth", next])
                    .map(|_| format!("Bluetooth radio {next}d"))
            },
            "bluetooth_enable" => {
                crate::adb::adb_cmd(Some(&serial), &["shell", "svc", "bluetooth", "enable"])
                    .map(|_| "Bluetooth radio enabled".into())
            },
            "bluetooth_disable" => {
                crate::adb::adb_cmd(Some(&serial), &["shell", "svc", "bluetooth", "disable"])
                    .map(|_| "Bluetooth radio disabled".into())
            },
            "bluetooth_auto_connect" => {
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "svc", "bluetooth", "enable"]);
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "am", "broadcast", "-a", "android.bluetooth.adapter.action.REQUEST_DISCOVERABLE"]);
                let _ = crate::adb::adb_cmd(Some(&serial), &["shell", "am", "broadcast", "-a", "android.bluetooth.device.action.FOUND"]);
                Ok("Bluetooth auto-connect triggered: Radio active, peripheral reconnect sent".into())
            },
            "bluetooth_settings" => {
                crate::adb::adb_cmd(Some(&serial), &["shell", "am", "start", "-a", "android.settings.BLUETOOTH_SETTINGS"])
                    .map(|_| "Bluetooth settings launched on device".into())
            },
            _ => Err("Unsupported ADB action".to_string()),
        }
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            adb_devices,
            adb_initial_serial,
            adb_accent_color,
            adb_theme_info,
            adb_telemetry,
            adb_data,
            adb_screenshot,
            adb_photo_preview,
            adb_action
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Android Cockpit");
}
