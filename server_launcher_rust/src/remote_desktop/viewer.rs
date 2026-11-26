use super::{
    credentials::DesktopProtocol,
    framebuffer::{Framebuffer, Region},
    input::Capture,
    profile::{Clipboard, Resolution, Scaling},
    vnc::{Connection, Input},
};
use crate::{config::ConfigFile, theme::ThemeMode};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
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
    let host_name = name.to_owned();
    let message = if profile.is_none() {
        "The saved host could not be loaded. Open the viewer again from its host settings."
    } else {
        "The viewer window is ready. The protocol connection is still being implemented."
    };
    let scaling = profile
        .and_then(|host| host.vnc.as_ref())
        .filter(|_| protocol == DesktopProtocol::Vnc)
        .map_or(Scaling::Fit, |profile| profile.scaling);
    let viewer_host = profile.filter(|_| !test_pattern).cloned();
    let view_only = viewer_host
        .as_ref()
        .and_then(|host| host.vnc.as_ref())
        .is_some_and(|profile| protocol == DesktopProtocol::Vnc && profile.view_only);
    let dynamic_resize = protocol == DesktopProtocol::Rdp
        && viewer_host
            .as_ref()
            .and_then(|host| host.rdp.as_ref())
            .is_some_and(|profile| profile.resolution == Resolution::FitWindow);
    let send_clipboard = protocol == DesktopProtocol::Vnc
        && viewer_host
            .as_ref()
            .and_then(|host| host.vnc.as_ref())
            .is_some_and(|profile| {
                matches!(
                    profile.clipboard,
                    Clipboard::Both | Clipboard::LocalToRemote
                )
            });
    let config_path = path.to_owned();
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
                connection: viewer_host.as_ref().map(|host| {
                    start_connection(host.clone(), &config_path, protocol, &cc.egui_ctx)
                }),
                session_host: viewer_host,
                config_path,
                texture: None,
                scaling,
                fullscreen: false,
                toolbar_pinned: true,
                toolbar_until: None,
                toolbar_rect: egui::Rect::NOTHING,
                dynamic_resize,
                last_window_size: None,
                test_pattern,
                view_only,
                send_clipboard,
                capture: Capture::default(),
                host_name,
                protocol,
                screenshot: None,
                screenshot_result: None,
                show_fps: false,
                fps_sample: None,
                fps: 0.0,
            }))
        }),
    )
}

fn start_connection(
    host: crate::config::SshRemoteHost,
    config_path: &Path,
    protocol: DesktopProtocol,
    ctx: &egui::Context,
) -> Arc<Mutex<Connection>> {
    match protocol {
        DesktopProtocol::Rdp => super::rdp::start(host, config_path.to_owned(), ctx.clone()),
        DesktopProtocol::Vnc => Connection::start(host, ctx.clone()),
    }
}

struct Viewer {
    focus: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    message: &'static str,
    framebuffer: Framebuffer,
    connection: Option<Arc<Mutex<Connection>>>,
    session_host: Option<crate::config::SshRemoteHost>,
    config_path: PathBuf,
    texture: Option<egui::TextureHandle>,
    scaling: Scaling,
    fullscreen: bool,
    toolbar_pinned: bool,
    toolbar_until: Option<Instant>,
    toolbar_rect: egui::Rect,
    dynamic_resize: bool,
    last_window_size: Option<[u32; 2]>,
    view_only: bool,
    send_clipboard: bool,
    capture: Capture,
    host_name: String,
    protocol: DesktopProtocol,
    screenshot: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    screenshot_result: Option<Result<PathBuf, String>>,
    test_pattern: bool,
    show_fps: bool,
    fps_sample: Option<(Instant, u32)>,
    fps: f64,
}

impl Viewer {
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let frame = if let Some(connection) = &self.connection {
            connection.lock().unwrap().framebuffer.snapshot()
        } else {
            self.framebuffer.snapshot()
        };
        let Some(frame) = frame else { return };
        let (sender, receiver) = mpsc::channel();
        self.screenshot = Some(receiver);
        self.screenshot_result = None;
        let host = self.host_name.clone();
        let protocol = self.protocol.label();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(super::screenshot::save(frame, &host, protocol));
            ctx.request_repaint();
        });
    }

    fn screenshot_notification(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.screenshot {
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Screenshot worker stopped unexpectedly.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.screenshot = None;
                self.screenshot_result = Some(result);
            }
        }
        let Some(result) = &self.screenshot_result else {
            return;
        };
        let mut close = false;
        let mut failure = None;
        egui::Area::new(egui::Id::new("remote_screenshot_result"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -14.0))
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.ctx().screen_rect().width().min(330.0) - 32.0);
                    match result {
                        Ok(path) => {
                            ui.strong("Screenshot saved");
                            ui.label(path.display().to_string());
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Open file").clicked() {
                                    failure = super::screenshot::open(path)
                                        .err()
                                        .map(|error| error.to_string());
                                }
                                if ui.button("Show in folder").clicked() {
                                    if let Some(folder) = path.parent() {
                                        failure = super::screenshot::open(folder)
                                            .err()
                                            .map(|error| error.to_string());
                                    }
                                }
                            });
                        }
                        Err(error) => {
                            ui.label(error);
                        }
                    }
                    close = ui.button("Dismiss").clicked();
                });
            });
        if close {
            self.screenshot_result = None;
        } else if let Some(error) = failure {
            self.screenshot_result = Some(Err(format!("Could not open the screenshot: {error}")));
        }
    }

    fn send_keys(&mut self, keys: &[u32]) {
        let Some(connection) = &self.connection else {
            return;
        };
        let connection = connection.lock().unwrap();
        if !connection.connected || self.view_only {
            return;
        }
        self.capture.release(|event| connection.send_input(event));
        for (keysym, pressed) in key_sequence(keys) {
            connection.send_input(Input::Key { keysym, pressed });
        }
    }

    fn send_input(
        &mut self,
        ctx: &egui::Context,
        canvas: egui::Rect,
        clip: egui::Rect,
        size: [usize; 2],
        layer: egui::LayerId,
    ) {
        let Some(connection) = &self.connection else {
            return;
        };
        let connection = connection.lock().unwrap();
        if !connection.connected || self.view_only {
            return;
        }
        let unobstructed = ctx
            .pointer_hover_pos()
            .and_then(|pos| ctx.layer_id_at(pos))
            .is_none_or(|top| top == layer);
        ctx.input(|input| {
            self.capture.events(
                &input.events,
                input.modifiers,
                input.focused && unobstructed,
                canvas,
                clip,
                size,
                input.pointer.hover_pos(),
                |event| {
                    if self.send_clipboard || !matches!(event, Input::ClipboardText(_)) {
                        connection.send_input(event);
                    }
                },
            );
        });
    }
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
        let prompt = self.connection.as_ref().and_then(|connection| {
            let state = connection.lock().unwrap();
            state.trust_prompt.as_ref().map(|prompt| {
                (
                    prompt.subject.clone(),
                    prompt.fingerprint.clone(),
                    prompt.answer.clone(),
                )
            })
        });
        if let Some((subject, fingerprint, answer)) = prompt {
            use super::trust::Choice;
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Verify the RDP certificate");
                ui.label(
                    "Compare the fingerprint with the server administrator before trusting it.",
                );
                ui.label(format!("Subject: {subject}"));
                ui.label("SHA-256");
                ui.monospace(fingerprint);
                ui.horizontal_wrapped(|ui| {
                    for (label, choice) in [
                        ("Cancel", Choice::Cancel),
                        ("Trust once", Choice::Once),
                        ("Always trust", Choice::Always),
                    ] {
                        if ui.button(label).clicked() {
                            let _ = answer.send(choice);
                        }
                    }
                });
            });
            return;
        }
        let (update, status, connected, reconnect_at, finished, connected_since, received_bytes) =
            if let Some(connection) = &self.connection {
                let mut connection = connection.lock().unwrap();
                if let Some(text) = connection.clipboard_text.take() {
                    ctx.copy_text(text);
                }
                (
                    if connection.receiving_frame {
                        None
                    } else {
                        connection.framebuffer.take_update()
                    },
                    connection.status.clone(),
                    connection.connected,
                    connection.reconnect_at,
                    connection.finished,
                    connection.connected_since,
                    connection.received_bytes.load(Ordering::Relaxed),
                )
            } else {
                (
                    self.framebuffer.take_update(),
                    self.message.into(),
                    false,
                    None,
                    false,
                    None,
                    0,
                )
            };
        if !connected {
            self.capture = Capture::default();
            self.last_window_size = None;
        }
        if connected && self.show_fps {
            let (since, frames) = self.fps_sample.get_or_insert_with(|| (Instant::now(), 0));
            *frames += u32::from(update.is_some());
            let elapsed = since.elapsed();
            let interval = Duration::from_secs(1);
            if elapsed >= interval {
                self.fps = f64::from(*frames) / elapsed.as_secs_f64();
                self.fps_sample = Some((Instant::now(), 0));
                ctx.request_repaint_after(interval);
            } else {
                ctx.request_repaint_after(interval - elapsed);
            }
        } else {
            self.fps_sample = None;
            self.fps = 0.0;
        }
        if let Some(update) = update {
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
        let pinned = self.toolbar_pinned || !connected;
        let now = Instant::now();
        let screen = ctx.screen_rect();
        let visible = self.toolbar_until.is_some_and(|until| until > now);
        let hovered = ctx.pointer_hover_pos().is_some_and(|pos| {
            pos.y <= screen.top() + 5.0 || (visible && self.toolbar_rect.contains(pos))
        });
        if hovered || (visible && ctx.memory(|memory| memory.any_popup_open())) {
            self.toolbar_until = Some(now + Duration::from_secs(2));
        }
        let overlay_until = self.toolbar_until.filter(|until| *until > now);
        let toolbar = |ui: &mut egui::Ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(if self.test_pattern {
                    "Test pattern"
                } else if connected {
                    &status
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
                if let Some(deadline) = reconnect_at {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    ui.label(format!(
                        "Reconnecting in {}s",
                        remaining.as_secs_f64().ceil() as u64
                    ));
                    ctx.request_repaint_after(
                        remaining.clamp(Duration::from_millis(50), Duration::from_secs(1)),
                    );
                    if ui.button("Cancel retry").clicked() {
                        if let Some(connection) = &self.connection {
                            let mut connection = connection.lock().unwrap();
                            if connection.reconnect_at.take().is_some() {
                                connection.finished = true;
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                if finished {
                    if let Some(host) = &self.session_host {
                        if ui.button("Retry").clicked() {
                            self.connection = Some(start_connection(
                                host.clone(),
                                &self.config_path,
                                self.protocol,
                                ctx,
                            ));
                            self.texture = None;
                            ctx.request_repaint();
                        }
                    }
                }
                if ui.checkbox(&mut self.show_fps, "Show live FPS").changed() {
                    ctx.request_repaint();
                }
                if self.show_fps && connected {
                    ui.label(format!("{:.1} FPS", self.fps));
                }
                ui.add_enabled_ui(connected, |ui| {
                    ui.menu_button("Stats", |ui| {
                        ui.label(self.protocol.label());
                        ui.label(&status);
                        if let Some(texture) = &self.texture {
                            let [width, height] = texture.size();
                            ui.label(format!("Remote resolution  {width} × {height}"));
                        }
                        if let Some(since) = connected_since {
                            let seconds = since.elapsed().as_secs();
                            ui.label(format!(
                                "Session time  {}:{:02}",
                                seconds / 60,
                                seconds % 60
                            ));
                            let elapsed = since.elapsed().as_secs_f64();
                            if elapsed >= 1.0 {
                                let kbps = received_bytes as f64 * 8.0 / elapsed / 1000.0;
                                ui.label(format!("Average protocol receive rate  {kbps:.1} kbps"));
                            }
                            ctx.request_repaint_after(Duration::from_secs(1));
                        }
                        if self.show_fps {
                            ui.label(format!("Displayed updates  {:.1} FPS", self.fps));
                        }
                    });
                });
                if self.protocol == DesktopProtocol::Vnc {
                    if let Some(connection) = &self.connection {
                        let connection = connection.lock().unwrap();
                        let menu = ui
                            .add_enabled_ui(
                                connected && !self.view_only && connection.vnc_screen.is_some(),
                                |ui| {
                                    ui.menu_button("Resolution", |ui| {
                                        for size in [
                                            [1024, 768],
                                            [1280, 720],
                                            [1600, 900],
                                            [1920, 1080],
                                            [2560, 1440],
                                        ] {
                                            if ui
                                                .button(format!("{} × {}", size[0], size[1]))
                                                .clicked()
                                            {
                                                if let Some(screen) = connection.vnc_screen {
                                                    connection.send_input(Input::VncResize {
                                                        size,
                                                        screen,
                                                    });
                                                }
                                                ui.close_menu();
                                            }
                                        }
                                    })
                                    .response
                                },
                            )
                            .inner;
                        if connection.vnc_screen.is_none() {
                            menu.on_disabled_hover_text(
                                "The VNC server must support single-screen remote resizing.",
                            );
                        }
                    }
                }
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
                if !connected && self.texture.is_some() && !self.test_pattern {
                    ui.label(&status);
                }
                let screenshot_label = if self.screenshot.is_some() {
                    "Saving…"
                } else {
                    "Screenshot"
                };
                if ui
                    .add_enabled(
                        self.texture.is_some() && self.screenshot.is_none(),
                        egui::Button::new(screenshot_label),
                    )
                    .clicked()
                {
                    self.take_screenshot(ctx);
                }
                ui.add_enabled_ui(connected && !self.view_only, |ui| {
                    ui.menu_button("Send keys", |ui| {
                        let shortcuts: &[(&str, &[u32])] = &[
                            ("Ctrl+Alt+Del", &[0xffe3, 0xffe9, 0xffff]),
                            ("Alt+Tab", &[0xffe9, 0xff09]),
                            ("Alt+F4", &[0xffe9, 0xffc1]),
                            ("Windows key", &[0xffeb]),
                            ("Win+L", &[0xffeb, b'l' as u32]),
                            ("Win+D", &[0xffeb, b'd' as u32]),
                            ("Ctrl+Shift+Esc", &[0xffe3, 0xffe1, 0xff1b]),
                            ("Print Screen", &[0xff61]),
                            ("Escape", &[0xff1b]),
                            ("Ctrl+C", &[0xffe3, b'c' as u32]),
                            ("Ctrl+V", &[0xffe3, b'v' as u32]),
                        ];
                        for &(label, keys) in shortcuts {
                            if ui.button(label).clicked() {
                                self.send_keys(keys);
                                ui.close_menu();
                            }
                        }
                        ui.menu_button("Function keys", |ui| {
                            for number in 1..=12 {
                                if ui.button(format!("F{number}")).clicked() {
                                    self.send_keys(&[0xffbd + number]);
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                });
                if self.view_only && connected {
                    ui.label("View only");
                }
                if connected {
                    let label = if self.toolbar_pinned {
                        "Unpin toolbar"
                    } else {
                        "Pin toolbar"
                    };
                    if ui.button(label).clicked() {
                        self.toolbar_pinned = !self.toolbar_pinned;
                        self.toolbar_until = Some(now + Duration::from_secs(2));
                        ctx.request_repaint();
                    }
                }
                if ui.button("Disconnect").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        };
        if pinned {
            egui::TopBottomPanel::top("viewer_tools").show(ctx, toolbar);
        } else if let Some(until) = overlay_until {
            ctx.request_repaint_after(until - now);
            self.toolbar_rect = egui::Area::new(egui::Id::new("floating_viewer_tools"))
                .fixed_pos(screen.left_top())
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.set_max_width(screen.width());
                    egui::Frame::popup(ui.style()).show(ui, toolbar);
                })
                .response
                .rect;
        }
        self.screenshot_notification(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dynamic_resize && connected {
                let size = ui.available_size() * ctx.pixels_per_point();
                let dimensions = super::rdp::display_dimensions(size.x as u32, size.y as u32);
                if self.last_window_size != Some(dimensions) {
                    self.last_window_size = Some(dimensions);
                    if let Some(connection) = &self.connection {
                        connection.lock().unwrap().resize_request = Some(dimensions);
                    }
                }
            }
            let Some(texture) = &self.texture else {
                ui.centered_and_justified(|ui| ui.label(&status));
                return;
            };
            let native = texture.size_vec2() / ctx.pixels_per_point();
            let pixels = texture.size();
            let mut clip = ui.clip_rect();
            let screen = if self.scaling == Scaling::Native {
                egui::ScrollArea::both()
                    .show(ui, |ui| {
                        clip = ui.clip_rect();
                        ui.image((texture.id(), native)).rect
                    })
                    .inner
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
                screen
            };
            self.send_input(ctx, screen, clip, pixels, ui.layer_id());
        });
    }
}

fn key_sequence(keys: &[u32]) -> impl Iterator<Item = (u32, bool)> + '_ {
    keys.iter()
        .map(|&key| (key, true))
        .chain(keys.iter().rev().map(|&key| (key, false)))
}

#[cfg(test)]
#[test]
fn shortcut_releases_keys_in_reverse_order() {
    assert_eq!(
        key_sequence(&[0xffe3, 0xffff]).collect::<Vec<_>>(),
        [
            (0xffe3, true),
            (0xffff, true),
            (0xffff, false),
            (0xffe3, false)
        ]
    );
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
