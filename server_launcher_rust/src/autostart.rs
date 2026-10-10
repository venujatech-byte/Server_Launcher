use std::path::PathBuf;

/// Check if Server Launcher is configured to run at system startup (login/boot)
pub fn is_app_autostart_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let autostart_file = PathBuf::from(home).join(".config/autostart/server_launcher.desktop");
            return autostart_file.exists();
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let autostart_file = PathBuf::from(appdata)
                .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup\\ServerLauncher.bat");
            return autostart_file.exists();
        }
    }

    false
}

/// Enable or disable Server Launcher running at system startup
pub fn set_app_autostart(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
        let autostart_dir = PathBuf::from(&home).join(".config/autostart");
        let autostart_file = autostart_dir.join("server_launcher.desktop");

        if enable {
            std::fs::create_dir_all(&autostart_dir)
                .map_err(|e| format!("Failed to create autostart directory: {}", e))?;

            let current_exe = std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from("server_launcher"));

            let content = format!(
                "[Desktop Entry]\nVersion=1.0\nType=Application\nName=Server Launcher\nComment=Development Server Launcher & Monitor\nExec=\"{}\"\nIcon=server_launcher\nTerminal=false\nCategories=Development;Utility;\nStartupWMClass=server_launcher\nX-GNOME-Autostart-enabled=true\n",
                current_exe.display()
            );

            std::fs::write(&autostart_file, content)
                .map_err(|e| format!("Failed to write autostart desktop file: {}", e))?;
        } else if autostart_file.exists() {
            let _ = std::fs::remove_file(&autostart_file);
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")
            .map_err(|_| "APPDATA environment variable not set".to_string())?;
        let startup_dir = PathBuf::from(appdata)
            .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        let bat_file = startup_dir.join("ServerLauncher.bat");

        if enable {
            std::fs::create_dir_all(&startup_dir)
                .map_err(|e| format!("Failed to create startup directory: {}", e))?;

            let current_exe = std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from("server_launcher.exe"));

            let content = format!("start \"\" \"{}\"\r\n", current_exe.display());
            std::fs::write(&bat_file, content)
                .map_err(|e| format!("Failed to write startup batch file: {}", e))?;
        } else if bat_file.exists() {
            let _ = std::fs::remove_file(&bat_file);
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = enable;
        Err("Autostart is not supported on this platform".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_check_does_not_crash() {
        let _ = is_app_autostart_enabled();
    }
}

