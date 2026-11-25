use super::credentials::DesktopProtocol;
use crate::{config::ConfigFile, theme::ThemeMode};
use std::{
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub(crate) fn run(
    host_id: &str,
    protocol: DesktopProtocol,
    path: &Path,
    theme: &str,
) -> eframe::Result<()> {
    let config = ConfigFile::load_from_file(path);
    let profile = config.ssh_hosts.iter().find(|host| host.id == host_id);
    let name = profile.map_or("Remote Desktop", |host| host.name.as_str());
    let title = format!("{name} - {}", protocol.label());
    let message = if profile.is_none() {
        "The saved host could not be loaded. Open the viewer again from its host settings."
    } else {
        "The viewer window is ready. The protocol connection is still being implemented."
    };
    let palette = ThemeMode::from_id_str(theme).palette();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1024.0, 700.0])
            .with_min_inner_size([420.0, 260.0]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(palette.egui_visuals());
            let focus = Arc::new(AtomicBool::new(false));
            let closed = Arc::new(AtomicBool::new(false));
            let ctx = cc.egui_ctx.clone();
            let request_focus = focus.clone();
            let parent_closed = closed.clone();
            std::thread::spawn(move || {
                let mut input = std::io::stdin().lock();
                let mut command = [0];
                while input.read_exact(&mut command).is_ok() {
                    if command[0] == b'f' {
                        request_focus.store(true, Ordering::Release);
                        ctx.request_repaint();
                    }
                }
                parent_closed.store(true, Ordering::Release);
                ctx.request_repaint();
            });

            Ok(Box::new(Viewer {
                focus,
                closed,
                message,
            }))
        }),
    )
}

struct Viewer {
    focus: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    message: &'static str,
}

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.closed.load(Ordering::Acquire) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.focus.swap(false, Ordering::AcqRel) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| ui.label(self.message));
        });
    }
}
