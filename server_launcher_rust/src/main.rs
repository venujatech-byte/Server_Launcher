mod app;
mod config;
mod modals;
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

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
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
