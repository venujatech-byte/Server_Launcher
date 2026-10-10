mod app;
mod config;
mod modals;
mod service;

use app::LauncherApp;
use eframe::egui::{self, Color32, Rounding, Visuals};

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Server Launcher (Rust)")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([750.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Server Launcher",
        native_options,
        Box::new(|cc| {
            // Apply sleek dark theme
            let mut visuals = Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(17, 19, 24);
            visuals.window_fill = Color32::from_rgb(26, 30, 43);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(26, 29, 39);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(37, 40, 51);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(47, 51, 64);
            visuals.widgets.active.bg_fill = Color32::from_rgb(108, 92, 231);
            visuals.widgets.noninteractive.rounding = Rounding::same(4.0);
            visuals.widgets.inactive.rounding = Rounding::same(4.0);
            visuals.widgets.hovered.rounding = Rounding::same(4.0);
            visuals.widgets.active.rounding = Rounding::same(4.0);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(LauncherApp::new(cc)))
        }),
    )
}
