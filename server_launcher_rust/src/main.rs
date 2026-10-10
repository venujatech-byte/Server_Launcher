mod app;
mod config;
mod modals;
mod remote;
mod scanner;
mod service;

use app::LauncherApp;
use eframe::egui::{self, Color32, Rounding, Visuals};

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
fn ensure_linux_desktop_integration() {
    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return,
    };

    let icon_bytes = include_bytes!("../assets/icon.png");

    // 1. Install icon into standard XDG icon directories
    let icon_dir = home.join(".local/share/icons/hicolor/512x512/apps");
    if std::fs::create_dir_all(&icon_dir).is_ok() {
        let _ = std::fs::write(icon_dir.join("server_launcher.png"), icon_bytes);
    }

    let base_icon_dir = home.join(".local/share/icons");
    if std::fs::create_dir_all(&base_icon_dir).is_ok() {
        let _ = std::fs::write(base_icon_dir.join("server_launcher.png"), icon_bytes);
    }

    // 2. Install desktop entry matching app_id = "server_launcher"
    let apps_dir = home.join(".local/share/applications");
    if std::fs::create_dir_all(&apps_dir).is_ok() {
        let current_exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("server_launcher"));
        let desktop_content = format!(
            "[Desktop Entry]\nVersion=1.0\nType=Application\nName=Server Launcher\nComment=Development Server Launcher & Monitor\nExec=\"{}\"\nIcon=server_launcher\nTerminal=false\nCategories=Development;Utility;\nStartupWMClass=server_launcher\nStartupNotify=true\n",
            current_exe.display()
        );
        let _ = std::fs::write(apps_dir.join("server_launcher.desktop"), desktop_content);

        let _ = std::process::Command::new("update-desktop-database")
            .arg(&apps_dir)
            .output();
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_desktop_integration();

    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("server_launcher")
        .with_title("Server Launcher (Rust)")
        .with_inner_size([1200.0, 780.0])
        .with_min_inner_size([800.0, 500.0]);

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
            let mut visuals = Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(15, 17, 23);
            visuals.window_fill = Color32::from_rgb(22, 25, 34);
            visuals.faint_bg_color = Color32::from_rgb(18, 21, 29);
            visuals.extreme_bg_color = Color32::from_rgb(10, 12, 17);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 25, 34);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(37, 40, 51);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(47, 51, 64);
            visuals.widgets.active.bg_fill = Color32::from_rgb(108, 92, 231);
            visuals.widgets.noninteractive.rounding = Rounding::same(4.0);
            visuals.widgets.inactive.rounding = Rounding::same(4.0);
            visuals.widgets.hovered.rounding = Rounding::same(6.0);
            visuals.widgets.active.rounding = Rounding::same(6.0);
            cc.egui_ctx.set_visuals(visuals);

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
