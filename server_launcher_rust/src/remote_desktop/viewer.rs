use super::{
    credentials::DesktopProtocol,
    framebuffer::{Framebuffer, Region},
    profile::Scaling,
};
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
    test_pattern: bool,
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
    let scaling = profile
        .and_then(|host| host.vnc.as_ref())
        .filter(|_| protocol == DesktopProtocol::Vnc)
        .map_or(Scaling::Fit, |profile| profile.scaling);
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

            let mut framebuffer = Framebuffer::default();
            if test_pattern {
                write_test_pattern(&mut framebuffer).map_err(std::io::Error::other)?;
            }

            Ok(Box::new(Viewer {
                focus,
                closed,
                message,
                framebuffer,
                texture: None,
                scaling,
                fullscreen: false,
                test_pattern,
            }))
        }),
    )
}

struct Viewer {
    focus: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    message: &'static str,
    framebuffer: Framebuffer,
    texture: Option<egui::TextureHandle>,
    scaling: Scaling,
    fullscreen: bool,
    test_pattern: bool,
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
        if let Some(update) = self.framebuffer.take_update() {
            match self.texture.as_mut() {
                Some(texture) if !update.resized => {
                    texture.set_partial(update.origin, update.image, egui::TextureOptions::LINEAR);
                }
                Some(texture) => texture.set(update.image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "remote-frame",
                        update.image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
        }
        let fullscreen = ctx
            .input(|input| input.viewport().fullscreen)
            .unwrap_or(self.fullscreen);
        self.fullscreen = fullscreen;
        egui::TopBottomPanel::top("viewer_tools").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(if self.test_pattern {
                    "Test pattern"
                } else {
                    "Disconnected"
                });
                egui::ComboBox::from_id_salt("remote_scaling")
                    .selected_text(scale_label(self.scaling))
                    .show_ui(ui, |ui| {
                        for scale in [Scaling::Fit, Scaling::Native, Scaling::Fill] {
                            ui.selectable_value(&mut self.scaling, scale, scale_label(scale));
                        }
                    });
                if ui
                    .button(if fullscreen {
                        "Exit fullscreen"
                    } else {
                        "Fullscreen"
                    })
                    .clicked()
                {
                    self.fullscreen = !fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
                if ui.button("Disconnect").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(texture) = &self.texture else {
                ui.centered_and_justified(|ui| ui.label(self.message));
                return;
            };
            let native = texture.size_vec2() / ctx.pixels_per_point();
            if self.scaling == Scaling::Native {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.image((texture.id(), native));
                });
            } else {
                let canvas = ui.available_rect_before_wrap();
                let size = if self.scaling == Scaling::Fit {
                    native * (canvas.width() / native.x).min(canvas.height() / native.y)
                } else {
                    canvas.size()
                };
                let screen = egui::Rect::from_center_size(canvas.center(), size);
                ui.painter().image(
                    texture.id(),
                    screen,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        });
    }
}

fn scale_label(scale: Scaling) -> &'static str {
    match scale {
        Scaling::Fit => "Fit",
        Scaling::Native => "100%",
        Scaling::Fill => "Fill",
    }
}

fn write_test_pattern(framebuffer: &mut Framebuffer) -> Result<(), &'static str> {
    let size = [960, 540];
    framebuffer.resize(size)?;
    let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let checker = if (x / 60 + y / 60) % 2 == 0 { 120 } else { 75 };
            rgba.extend_from_slice(&[
                (x * 255 / size[0]) as u8,
                (y * 255 / size[1]) as u8,
                checker,
                255,
            ]);
        }
    }
    framebuffer.write_rgba(
        Region {
            origin: [0, 0],
            size,
        },
        &rgba,
    )
}
