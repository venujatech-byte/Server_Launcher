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
            adb_telemetry,
            adb_data,
            adb_screenshot,
            adb_photo_preview,
            adb_action
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Android Cockpit");
}
