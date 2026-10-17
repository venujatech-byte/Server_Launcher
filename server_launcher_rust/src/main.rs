mod app;
mod config;
mod modals;
mod remote;
mod scanner;
mod service;
pub mod adb;
pub mod autostart;
pub mod bluetooth;
pub mod commands;
pub mod lucide;
pub mod power;
pub mod procmgr;
pub mod scripts;
pub mod sensors;
pub mod systemd;
pub mod theme;
pub mod wifi;

use app::LauncherApp;
use config::ConfigFile;
use eframe::egui;
use theme::ThemeMode;

pub fn load_app_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../assets/icon.png");
    if let Ok(img) = image::load_from_memory(png_bytes) {
        let rgba = img.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        Some(egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        })
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub fn ensure_linux_desktop_integration() {
    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return,
    };

    let icon_512_bytes = include_bytes!("../assets/icon.png");
    let icon_96_bytes = include_bytes!("../assets/icon_96.png");

    // 1. Install icons into all standard XDG icon theme and pixmap locations.
    // This ensures GNOME, KDE, XFCE, Cinnamon, and custom themes (such as MacTahoe)
    // find the icon regardless of lookup method or icon size requested.
    let icon_sizes = ["512x512", "256x256", "128x128", "64x64", "48x48", "32x32"];
    for size in icon_sizes {
        let size_dir = home.join(format!(".local/share/icons/hicolor/{}/apps", size));
        if std::fs::create_dir_all(&size_dir).is_ok() {
            let _ = std::fs::write(size_dir.join("server_launcher.png"), icon_512_bytes);
        }
    }

    // 96x96
    let dir_96 = home.join(".local/share/icons/hicolor/96x96/apps");
    if std::fs::create_dir_all(&dir_96).is_ok() {
        let _ = std::fs::write(dir_96.join("server_launcher.png"), icon_96_bytes);
    }

    // Fallbacks: direct ~/.local/share/icons/ and ~/.local/share/pixmaps/
    let base_icon_dir = home.join(".local/share/icons");
    if std::fs::create_dir_all(&base_icon_dir).is_ok() {
        let _ = std::fs::write(base_icon_dir.join("server_launcher.png"), icon_512_bytes);
    }

    let pixmaps_dir = home.join(".local/share/pixmaps");
    if std::fs::create_dir_all(&pixmaps_dir).is_ok() {
        let _ = std::fs::write(pixmaps_dir.join("server_launcher.png"), icon_512_bytes);
    }

    // Primary absolute icon path for guaranteed loading across all desktop environments
    let primary_icon_path = home.join(".local/share/icons/hicolor/512x512/apps/server_launcher.png");

    // Refresh icon cache
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t", &home.join(".local/share/icons/hicolor").to_string_lossy()])
        .output();

    // 2. Determine target binary path.
    // When the user runs the downloaded GitHub release binary from Downloads or a temporary folder,
    // copy it cleanly to ~/.local/bin/server_launcher so the desktop entry and desktop shortcut
    // remain permanently valid even if the user deletes or moves their Downloads folder.
    let current_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("server_launcher"));
    let local_bin_dir = home.join(".local/bin");
    let target_bin = local_bin_dir.join("server_launcher");

    let is_system_install = current_exe.starts_with("/usr") || current_exe.starts_with("/opt");
    let is_local_bin = current_exe == target_bin;

    let exec_path = if !is_system_install && !is_local_bin {
        let _ = std::fs::create_dir_all(&local_bin_dir);
        if std::fs::copy(&current_exe, &target_bin).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&target_bin, std::fs::Permissions::from_mode(0o755));
            }
            target_bin
        } else {
            current_exe.clone()
        }
    } else {
        current_exe.clone()
    };

    // 3. Desktop entry content
    // Specifying the absolute icon path guarantees that GNOME Shell (including custom themes like MacTahoe),
    // KDE, XFCE, and application launchers immediately render the icon.
    let desktop_content = format!(
        "[Desktop Entry]\n\
Version=1.0\n\
Type=Application\n\
Name=Server Launcher\n\
GenericName=Server Launcher & Android Cockpit\n\
Comment=Development Server Launcher, ADB Android Cockpit & System Monitor\n\
Exec=\"{}\" %u\n\
Icon={}\n\
Terminal=false\n\
Categories=Development;Utility;System;\n\
StartupWMClass=server_launcher\n\
StartupNotify=true\n\
Keywords=server;launcher;adb;android;scrcpy;terminal;ssh;docker;monitor;\n\
X-GNOME-UsesNotifications=true\n",
        exec_path.display(),
        primary_icon_path.display()
    );

    // 4. Install desktop entry in ~/.local/share/applications/ (shows in "All Apps")
    let apps_dir = home.join(".local/share/applications");
    if std::fs::create_dir_all(&apps_dir).is_ok() {
        let app_desktop_file = apps_dir.join("server_launcher.desktop");
        let _ = std::fs::write(&app_desktop_file, &desktop_content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&app_desktop_file, std::fs::Permissions::from_mode(0o755));
        }

        let _ = std::process::Command::new("update-desktop-database")
            .arg(&apps_dir)
            .output();
    }

    // 5. Create Desktop shortcut on ~/Desktop (if Desktop directory exists)
    let desktop_dir = home.join("Desktop");
    if desktop_dir.is_dir() {
        let desktop_file = desktop_dir.join("server_launcher.desktop");
        let _ = std::fs::write(&desktop_file, &desktop_content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&desktop_file, std::fs::Permissions::from_mode(0o755));
        }

        // On GNOME Shell, desktop entries require gio set metadata::trusted true
        // so the launcher icon activates and can be opened without warning prompts.
        let _ = std::process::Command::new("gio")
            .args(["set", &desktop_file.to_string_lossy(), "metadata::trusted", "true"])
            .output();
    }
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_linux_desktop_integration() {}

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_desktop_integration();

    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("server_launcher")
        .with_title("Server Launcher (Rust)")
        .with_inner_size([1200.0, 780.0])
        .with_min_inner_size([800.0, 500.0])
        .with_transparent(false)
        // Custom titlebar: native decorations on eframe are the legacy server-side
        // style, so we draw our own modern GTK4/Adwaita-style headerbar instead.
        .with_decorations(false);

    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Server Launcher",
        native_options,
        Box::new(|cc| {
            let config_path = ConfigFile::default_path();
            let config_file = ConfigFile::load_from_file(&config_path);
            let initial_theme = config_file
                .theme
                .as_deref()
                .map(ThemeMode::from_id_str)
                .unwrap_or_default();
            cc.egui_ctx.set_visuals(initial_theme.palette().egui_visuals());

            // Initialize Lucide icon pack font for crisp, reliable vector icon rendering
            lucide::setup_lucide_font(&cc.egui_ctx);

            // Scale UI for comfortable modern HiDPI reading
            cc.egui_ctx.set_pixels_per_point(1.15);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.text_styles = [
                (egui::TextStyle::Heading, egui::FontId::new(20.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Name("Subheading".into()), egui::FontId::new(16.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Body, egui::FontId::new(14.0, egui::FontFamily::Proportional)),
                (egui::TextStyle::Button, egui::FontId::new(13.5, egui::FontFamily::Proportional)),
                (egui::TextStyle::Monospace, egui::FontId::new(13.0, egui::FontFamily::Monospace)),
                (egui::TextStyle::Small, egui::FontId::new(11.5, egui::FontFamily::Proportional)),
            ].into();
            style.spacing.item_spacing = egui::vec2(8.0, 7.0);
            style.spacing.button_padding = egui::vec2(9.0, 5.0);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(LauncherApp::new(cc)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_app_icon() {
        let icon = load_app_icon();
        assert!(icon.is_some(), "Icon failed to load from memory!");
        let icon = icon.unwrap();
        println!("Loaded icon: {}x{}, bytes: {}", icon.width, icon.height, icon.rgba.len());
        assert_eq!(icon.width, 512);
        assert_eq!(icon.height, 512);
    }

    #[test]
    fn test_color_image_creation() {
        let png_bytes = include_bytes!("../assets/icon.png");
        let img = image::load_from_memory(png_bytes).expect("image load failed");
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba.into_raw());
        assert_eq!(color_image.size, [512, 512]);
        assert_eq!(color_image.pixels.len(), 512 * 512);
    }
}
