use super::{
    credentials::DesktopProtocol,
    framebuffer::{Framebuffer, Region, Update},
    input::Capture,
    profile::{Clipboard, RdpFramePainting, RdpPainting, Resolution, Scaling},
    vnc::{Connection, Input},
};
use crate::{config::ConfigFile, theme::ThemeMode};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

fn toolbar_icon_button(
    ui: &mut egui::Ui,
    icon: &str,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(toolbar_icon_text(icon))
            .min_size(egui::vec2(30.0, 28.0))
            .rounding(egui::Rounding::same(8.0)),
    )
    .on_hover_text(tooltip)
}

fn toolbar_toggle_button(
    ui: &mut egui::Ui,
    icon: &str,
    tooltip: &str,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(toolbar_icon_text(icon))
            .selected(selected)
            .min_size(egui::vec2(30.0, 28.0))
            .rounding(egui::Rounding::same(8.0)),
    )
    .on_hover_text(tooltip)
}

fn toolbar_icon_text(icon: &str) -> egui::RichText {
    egui::RichText::new(icon)
        .family(egui::FontFamily::Proportional)
        .size(13.0)
}

pub(crate) fn run(
    host_id: &str,
    protocol: DesktopProtocol,
    path: &Path,
    theme: &str,
    test_pattern: bool,
) -> eframe::Result<()> {
    let config = ConfigFile::load_from_file(path);
    let show_update_rate = config.remote_desktop.show_live_fps;
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let host_key_enabled = config.remote_desktop.host_key_enabled;
    let screenshot_folder = config.remote_desktop.screenshot_folder.clone();
    let recording_folder = config.remote_desktop.recording_folder.clone();
    let ffmpeg_path = config.remote_desktop.ffmpeg_path.clone();
    let recording_fps = config.remote_desktop.recording_fps;
    let recording_format = config.remote_desktop.recording_format;
    let copy_screenshots_to_clipboard = config.remote_desktop.copy_screenshots_to_clipboard;
    let clipboard_image_limit_bytes =
        usize::from(config.remote_desktop.clipboard_image_limit_mb.clamp(1, 64)) * 1024 * 1024;
    let warn_unencrypted_vnc = config.remote_desktop.warn_unencrypted_vnc;
    let default_scaling = config.remote_desktop.default_scaling;
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
        .and_then(|host| match protocol {
            DesktopProtocol::Rdp => host.rdp.as_ref().map(|profile| profile.scaling),
            DesktopProtocol::Vnc => host.vnc.as_ref().map(|profile| profile.scaling),
        })
        .unwrap_or(default_scaling);
    let show_vnc_cursor = profile
        .and_then(|host| host.vnc.as_ref())
        .filter(|_| protocol == DesktopProtocol::Vnc)
        .is_none_or(|profile| profile.show_remote_cursor);
    let rdp_painting = profile
        .and_then(|host| host.rdp.as_ref())
        .filter(|_| protocol == DesktopProtocol::Rdp)
        .map_or(RdpPainting::Smooth, |profile| profile.painting);
    let rdp_frame_painting = profile
        .and_then(|host| host.rdp.as_ref())
        .filter(|_| protocol == DesktopProtocol::Rdp)
        .map_or(RdpFramePainting::Incremental, |profile| {
            profile.frame_painting
        });
    let (left_handed_mouse, disable_smooth_scrolling) = match protocol {
        DesktopProtocol::Rdp => profile
            .and_then(|host| host.rdp.as_ref())
            .map_or((false, false), |profile| {
                (profile.left_handed_mouse, profile.disable_smooth_scrolling)
            }),
        DesktopProtocol::Vnc => profile
            .and_then(|host| host.vnc.as_ref())
            .map_or((false, false), |profile| {
                (false, profile.disable_smooth_scrolling)
            }),
    };
    let viewer_host = profile.filter(|_| !test_pattern).cloned();
    let confirm_unencrypted_vnc = viewer_host
        .as_ref()
        .filter(|_| protocol == DesktopProtocol::Vnc)
        .is_some_and(|host| {
            warn_unencrypted_vnc
                && host.vnc.as_ref().is_some_and(|profile| {
                    !profile.tunnel_via_ssh && super::vnc::needs_unencrypted_warning(&host.host)
                })
        });
    let view_only = viewer_host.as_ref().is_some_and(|host| match protocol {
        DesktopProtocol::Rdp => host.rdp.as_ref().is_some_and(|profile| profile.view_only),
        DesktopProtocol::Vnc => host.vnc.as_ref().is_some_and(|profile| profile.view_only),
    });
    let close_on_failure = viewer_host
        .as_ref()
        .and_then(|host| host.vnc.as_ref())
        .is_some_and(|profile| protocol == DesktopProtocol::Vnc && profile.close_on_failure);
    let dynamic_resize = protocol == DesktopProtocol::Rdp
        && viewer_host
            .as_ref()
            .and_then(|host| host.rdp.as_ref())
            .is_some_and(|profile| {
                profile.resolution == Resolution::FitWindow && !profile.multi_monitor
            });
    let send_clipboard = viewer_host
        .as_ref()
        .and_then(|host| match protocol {
            DesktopProtocol::Vnc => host.vnc.as_ref().map(|profile| profile.clipboard),
            DesktopProtocol::Rdp => host.rdp.as_ref().map(|profile| profile.clipboard),
        })
        .is_some_and(|direction| matches!(direction, Clipboard::Both | Clipboard::LocalToRemote));
    let receive_clipboard = viewer_host
        .as_ref()
        .and_then(|host| match protocol {
            DesktopProtocol::Vnc => host.vnc.as_ref().map(|profile| profile.clipboard),
            DesktopProtocol::Rdp => host.rdp.as_ref().map(|profile| profile.clipboard),
        })
        .is_some_and(|direction| matches!(direction, Clipboard::Both | Clipboard::RemoteToLocal));
    let clipboard_enabled = viewer_host
        .as_ref()
        .and_then(|host| match protocol {
            DesktopProtocol::Vnc => host.vnc.as_ref().map(|profile| profile.clipboard),
            DesktopProtocol::Rdp => host.rdp.as_ref().map(|profile| profile.clipboard),
        })
        .is_some_and(|direction| direction != Clipboard::Off);
    let config_path = path.to_owned();
    let palette = ThemeMode::from_id_str(theme).palette();
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let (native_shortcut_tx, native_shortcut_rx) = mpsc::channel();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1024.0, 700.0])
            .with_active(true)
            .with_min_inner_size([420.0, 260.0]),
        persist_window: true,
        persistence_path: Some(session_persistence_path(path, host_id, protocol)),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(palette.egui_visuals());
            crate::lucide::setup_lucide_font(&cc.egui_ctx);
            let focus = Arc::new(AtomicBool::new(false));
            let closed = Arc::new(AtomicBool::new(false));
            let ctx = cc.egui_ctx.clone();
            let parent_ctx = ctx.clone();
            let request_focus = focus.clone();
            let parent_closed = closed.clone();
            std::thread::spawn(move || {
                let mut input = std::io::stdin().lock();
                let mut command = [0];
                while input.read_exact(&mut command).is_ok() {
                    if command[0] == b'f' {
                        request_focus.store(true, Ordering::Release);
                        parent_ctx.request_repaint();
                    }
                }
                parent_closed.store(true, Ordering::Release);
                parent_ctx.request_repaint();
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
                connection: None,
                clipboard_sync: clipboard_enabled.then(|| {
                    super::clipboard_sync::ClipboardSync::start(
                        ctx.clone(),
                        send_clipboard,
                        clipboard_image_limit_bytes,
                    )
                }),
                session_host: viewer_host,
                upload: None,
                upload_result: None,
                config_path,
                screenshot_folder,
                recording_folder,
                ffmpeg_path,
                recording_fps,
                recording_format,
                texture: None,
                vnc_cursor_texture: None,
                vnc_cursor_revision: 0,
                show_vnc_cursor,
                scaling,
                rdp_painting,
                rdp_frame_painting,
                fullscreen: false,
                toolbar_pinned: true,
                toolbar_until: None,
                toolbar_rect: egui::Rect::NOTHING,
                dynamic_resize,
                last_window_size: None,
                test_pattern,
                view_only,
                close_on_failure,
                send_clipboard,
                receive_clipboard,
                capture: Capture::new(left_handed_mouse, disable_smooth_scrolling),
                #[cfg(target_os = "linux")]
                native_keyboard: None,
                #[cfg(target_os = "linux")]
                native_keyboard_attempted: false,
                #[cfg(target_os = "windows")]
                native_keyboard: None,
                #[cfg(target_os = "windows")]
                native_keyboard_attempted: false,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                native_shortcut_tx,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                native_shortcut_rx,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                host_key_enabled,
                show_send_keys: false,
                keyboard_capture_feedback: None,
                left_handed_mouse,
                disable_smooth_scrolling,
                host_name,
                protocol,
                confirm_unencrypted_vnc,
                clipboard_notice: None,
                connection_error_notice: None,
                last_connection_error: None,
                copy_screenshot: copy_screenshots_to_clipboard,
                screenshot_clipboard: None,
                screenshot_queued: false,
                screenshot: None,
                recording: None,
                recording_result: None,
                screenshot_result: None,
                show_update_rate,
                texture_uploads: 0,
                texture_uploads_per_second: 0.0,
                receive_sample: None,
                receive_kbps: 0.0,
                traffic_sample: None,
                download_bytes_per_second: 0.0,
                upload_bytes_per_second: 0.0,
                live_fps: 0.0,
                password_entry: Zeroizing::new(String::new()),
            }))
        }),
    )
}

fn session_persistence_path(
    config_path: &Path,
    host_id: &str,
    protocol: DesktopProtocol,
) -> PathBuf {
    let mut hash = Sha256::new();
    hash.update(host_id.as_bytes());
    hash.update([0]);
    hash.update(protocol.label().as_bytes());
    let digest = hash.finalize();
    let filename = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    config_path.with_file_name(format!("remote-session-{filename}.ron"))
}

fn start_connection(
    host: crate::config::SshRemoteHost,
    config_path: &Path,
    protocol: DesktopProtocol,
    fit_window_size: Option<[u32; 2]>,
    session_password: Option<Zeroizing<String>>,
    ctx: &egui::Context,
) -> Arc<Mutex<Connection>> {
    match protocol {
        DesktopProtocol::Rdp => {
            let monitor_pixels = ctx.input(|input| {
                let viewport = input.viewport();
                let scale = viewport.native_pixels_per_point.unwrap_or(1.0);
                viewport.monitor_size.map(|size| size * scale)
            });
            let monitor_size = monitor_pixels.map(|size| {
                super::rdp::display_dimensions(size.x.round() as u32, size.y.round() as u32)
            });
            super::rdp::start(
                host,
                config_path.to_owned(),
                monitor_size,
                fit_window_size,
                session_password,
                ctx.clone(),
            )
        }
        DesktopProtocol::Vnc => {
            Connection::start(host, config_path.to_owned(), session_password, ctx.clone())
        }
    }
}

struct Viewer {
    focus: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    message: &'static str,
    framebuffer: Framebuffer,
    connection: Option<Arc<Mutex<Connection>>>,
    clipboard_sync: Option<super::clipboard_sync::ClipboardSync>,
    session_host: Option<crate::config::SshRemoteHost>,
    upload: Option<crate::app::DesktopUpload>,
    upload_result: Option<Result<String, String>>,
    config_path: PathBuf,
    screenshot_folder: Option<PathBuf>,
    recording_folder: Option<PathBuf>,
    ffmpeg_path: Option<PathBuf>,
    recording_fps: u32,
    recording_format: crate::config::RemoteRecordingFormat,
    texture: Option<egui::TextureHandle>,
    vnc_cursor_texture: Option<egui::TextureHandle>,
    vnc_cursor_revision: u64,
    show_vnc_cursor: bool,
    scaling: Scaling,
    rdp_painting: RdpPainting,
    rdp_frame_painting: RdpFramePainting,
    fullscreen: bool,
    toolbar_pinned: bool,
    toolbar_until: Option<Instant>,
    toolbar_rect: egui::Rect,
    dynamic_resize: bool,
    last_window_size: Option<[u32; 2]>,
    view_only: bool,
    close_on_failure: bool,
    send_clipboard: bool,
    receive_clipboard: bool,
    capture: Capture,
    #[cfg(target_os = "linux")]
    native_keyboard: Option<super::x11_keyboard::NativeKeyboardCapture>,
    #[cfg(target_os = "linux")]
    native_keyboard_attempted: bool,
    #[cfg(target_os = "windows")]
    native_keyboard: Option<super::windows_keyboard::NativeKeyboardCapture>,
    #[cfg(target_os = "windows")]
    native_keyboard_attempted: bool,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    native_shortcut_tx: mpsc::Sender<super::input::HostShortcut>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    native_shortcut_rx: mpsc::Receiver<super::input::HostShortcut>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    host_key_enabled: bool,
    show_send_keys: bool,
    keyboard_capture_feedback: Option<String>,
    left_handed_mouse: bool,
    disable_smooth_scrolling: bool,
    host_name: String,
    protocol: DesktopProtocol,
    confirm_unencrypted_vnc: bool,
    clipboard_notice: Option<(Instant, String)>,
    connection_error_notice: Option<(Instant, String)>,
    last_connection_error: Option<String>,
    copy_screenshot: bool,
    screenshot_clipboard: Option<arboard::Clipboard>,
    screenshot_queued: bool,
    recording: Option<super::recording::Recording>,
    recording_result: Option<Result<PathBuf, String>>,
    screenshot: Option<mpsc::Receiver<super::screenshot::SaveResult>>,
    screenshot_result: Option<Result<PathBuf, String>>,
    test_pattern: bool,
    show_update_rate: bool,
    texture_uploads: u64,
    texture_uploads_per_second: f64,
    receive_sample: Option<(Instant, u64)>,
    receive_kbps: f64,
    traffic_sample: Option<(Instant, u64, u64, u64, u64)>,
    download_bytes_per_second: f64,
    upload_bytes_per_second: f64,
    live_fps: f64,
    password_entry: Zeroizing<String>,
}

impl Viewer {
    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let frame = if let Some(connection) = &self.connection {
            let connection = connection.lock().unwrap();
            if connection.receiving_frame {
                self.screenshot_queued = true;
                return;
            }
            connection.framebuffer.snapshot()
        } else {
            self.framebuffer.snapshot()
        };
        self.screenshot_queued = false;
        let Some(frame) = frame else { return };
        let (sender, receiver) = mpsc::channel();
        self.screenshot = Some(receiver);
        self.screenshot_result = None;
        let host = self.host_name.clone();
        let protocol = self.protocol.label();
        let ctx = ctx.clone();
        let copy = self.copy_screenshot;
        let folder = self.screenshot_folder.clone();
        std::thread::spawn(move || {
            let _ = sender.send(super::screenshot::save_with_clipboard(
                frame, &host, protocol, copy, folder,
            ));
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
                self.screenshot_result = Some(result.map(|(path, clipboard)| {
                    if let Some(result) = clipboard {
                        match result {
                            Ok(clipboard) => {
                                self.screenshot_clipboard = Some(clipboard);
                                self.clipboard_notice =
                                    Some((Instant::now(), "Screenshot copied".into()));
                            }
                            Err(_) => {
                                self.clipboard_notice = Some((
                                    Instant::now(),
                                    "Screenshot clipboard unavailable".into(),
                                ));
                            }
                        }
                    }
                    path
                }));
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
        let keyboard_capture = self.capture.keyboard_capture();
        #[cfg(target_os = "linux")]
        self.native_keyboard.take();
        #[cfg(target_os = "windows")]
        self.native_keyboard.take();
        let connection = connection.lock().unwrap();
        if !connection.connected || self.view_only {
            return;
        }
        self.capture.release(|event| connection.send_input(event));
        for (keysym, pressed) in key_sequence(keys) {
            connection.send_input(Input::Key {
                keysym,
                physical_key: None,
                pressed,
            });
        }
        self.capture.set_keyboard_capture(keyboard_capture, |_| {});
        #[cfg(target_os = "linux")]
        {
            self.native_keyboard_attempted = false;
            self.keyboard_capture_feedback = None;
        }
        #[cfg(target_os = "windows")]
        {
            self.native_keyboard_attempted = false;
            self.keyboard_capture_feedback = None;
        }
    }

    fn toggle_recording(&mut self, ctx: &egui::Context) {
        if let Some(recording) = &self.recording {
            recording.stop();
            return;
        }
        if let Some(connection) = &self.connection {
            if connection.lock().unwrap().connected && self.texture.is_some() {
                self.recording_result = None;
                self.recording = Some(super::recording::Recording::start(
                    connection.clone(),
                    ctx.clone(),
                    self.recording_folder.clone(),
                    self.ffmpeg_path.clone(),
                    self.recording_fps,
                    self.recording_format,
                ));
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn handle_native_shortcuts(&mut self, ctx: &egui::Context) {
        while let Ok(shortcut) = self.native_shortcut_rx.try_recv() {
            match shortcut {
                super::input::HostShortcut::Fullscreen => {
                    self.fullscreen = !self.fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
                super::input::HostShortcut::Screenshot => self.take_screenshot(ctx),
                super::input::HostShortcut::Recording => self.toggle_recording(ctx),
                super::input::HostShortcut::Disconnect => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                super::input::HostShortcut::SendKeys => self.show_send_keys = true,
            }
        }
    }

    fn send_keys_window(&mut self, ctx: &egui::Context) {
        if !self.show_send_keys {
            return;
        }
        let mut open = self.show_send_keys;
        let mut selected: Option<Vec<u32>> = None;
        egui::Window::new("Send keys")
            .id(egui::Id::new("remote_send_keys_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for (label, keys) in [
                    ("Ctrl+Alt+Del", &[0xffe3, 0xffe9, 0xffff][..]),
                    ("Alt+Tab", &[0xffe9, 0xff09][..]),
                    ("Alt+F4", &[0xffe9, 0xffc1][..]),
                    ("Super / Windows key", &[0xffeb][..]),
                    ("Super+Tab", &[0xffeb, 0xff09][..]),
                    ("Win+L", &[0xffeb, b'l' as u32][..]),
                    ("Win+D", &[0xffeb, b'd' as u32][..]),
                    ("Ctrl+Shift+Esc", &[0xffe3, 0xffe1, 0xff1b][..]),
                    ("Print Screen", &[0xff61][..]),
                    ("Escape", &[0xff1b][..]),
                    ("Ctrl+C", &[0xffe3, b'c' as u32][..]),
                    ("Ctrl+V", &[0xffe3, b'v' as u32][..]),
                ] {
                    if ui.button(label).clicked() {
                        selected = Some(keys.to_vec());
                    }
                }
                ui.menu_button("Function keys", |ui| {
                    for number in 1..=12 {
                        if ui.button(format!("F{number}")).clicked() {
                            selected = Some(vec![0xffbd + number]);
                            ui.close_menu();
                        }
                    }
                });
            });
        self.show_send_keys = open;
        if let Some(keys) = selected {
            self.send_keys(&keys);
        }
    }

    fn send_input(
        &mut self,
        ctx: &egui::Context,
        canvas: egui::Rect,
        clip: egui::Rect,
        size: [usize; 2],
    ) {
        let Some(connection) = &self.connection else {
            #[cfg(target_os = "linux")]
            {
                self.native_keyboard.take();
                self.native_keyboard_attempted = false;
            }
            #[cfg(target_os = "windows")]
            {
                self.native_keyboard.take();
                self.native_keyboard_attempted = false;
            }
            return;
        };
        let connection = connection.lock().unwrap();
        if !connection.connected || self.view_only {
            self.capture.release(|event| connection.send_input(event));
            return;
        }
        ctx.input(|input| {
            self.capture.events(
                &input.events,
                input.modifiers,
                input.focused,
                canvas,
                clip,
                size,
                input.pointer.hover_pos(),
                |event| {
                    if self.send_clipboard
                        || !matches!(
                            event,
                            Input::ClipboardText(_)
                                | Input::ClipboardImage(_)
                                | Input::ClipboardFiles(_)
                        )
                    {
                        connection.send_input(event);
                    }
                },
            );
        });
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn sync_native_keyboard_capture(
        &mut self,
        ctx: &egui::Context,
        focused: bool,
        connected: bool,
    ) {
        if !connected || self.view_only || !self.capture.keyboard_capture() {
            self.native_keyboard.take();
            self.native_keyboard_attempted = false;
            self.keyboard_capture_feedback = None;
        } else if focused && self.native_keyboard.is_none() && !self.native_keyboard_attempted {
            self.native_keyboard_attempted = true;
            if let Some(connection) = &self.connection {
                #[cfg(target_os = "linux")]
                {
                    self.native_keyboard = Some(super::x11_keyboard::NativeKeyboardCapture::start(
                        connection.clone(),
                        ctx.clone(),
                        format!("{} - {}", self.host_name, self.protocol.label()),
                        self.native_shortcut_tx.clone(),
                        self.host_key_enabled,
                    ));
                }
                #[cfg(target_os = "windows")]
                {
                    self.native_keyboard =
                        Some(super::windows_keyboard::NativeKeyboardCapture::start(
                            connection.clone(),
                            self.protocol,
                            self.host_key_enabled,
                            self.native_shortcut_tx.clone(),
                            ctx.clone(),
                        ));
                }
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(capture) = &self.native_keyboard {
            capture.set_focused(focused);
        }
        if let Some(Err(error)) = self
            .native_keyboard
            .as_mut()
            .and_then(|capture| capture.poll_ready())
        {
            self.native_keyboard.take();
            self.keyboard_capture_feedback = Some(format!(
                "System shortcuts may stay local; use Send keys if needed. {error}"
            ));
        }
    }

    fn send_loading_keyboard(&mut self, ctx: &egui::Context) {
        let Some(connection) = &self.connection else {
            return;
        };
        let connection = connection.lock().unwrap();
        if !connection.connected || self.view_only {
            return;
        }
        ctx.input(|input| {
            self.capture.events(
                &input.events,
                input.modifiers,
                input.focused,
                egui::Rect::NOTHING,
                egui::Rect::NOTHING,
                [0, 0],
                None,
                |event| {
                    if self.send_clipboard
                        || !matches!(
                            event,
                            Input::ClipboardText(_)
                                | Input::ClipboardImage(_)
                                | Input::ClipboardFiles(_)
                        )
                    {
                        connection.send_input(event);
                    }
                },
            );
        });
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        self.native_keyboard.take();
        #[cfg(target_os = "windows")]
        self.native_keyboard.take();
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
        if self.dynamic_resize && self.last_window_size.is_none() {
            ctx.request_repaint();
        } else if self.connection.is_none() && !self.confirm_unencrypted_vnc {
            if let Some(host) = &self.session_host {
                self.connection = Some(start_connection(
                    host.clone(),
                    &self.config_path,
                    self.protocol,
                    if self.dynamic_resize {
                        self.last_window_size
                    } else {
                        None
                    },
                    None,
                    ctx,
                ));
            }
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
        if self.confirm_unencrypted_vnc {
            let mut choice = None;
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        egui::Frame::none()
                            .fill(ui.visuals().widgets.noninteractive.bg_fill)
                            .rounding(egui::Rounding::same(12.0))
                            .inner_margin(egui::Margin::same(24.0))
                            .show(ui, |ui| {
                                ui.set_max_width(520.0);
                                ui.heading("Direct VNC is not encrypted");
                                ui.add_space(8.0);
                                ui.label(format!(
                                    "The connection to {} will send screen data and input without transport encryption. Use an SSH tunnel when possible.",
                                    self.host_name
                                ));
                                ui.add_space(16.0);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("Use SSH tunnel").clicked() {
                                        choice = Some(0);
                                    }
                                    if ui.button("Continue directly").clicked() {
                                        choice = Some(1);
                                    }
                                    if ui.button("Cancel").clicked() {
                                        choice = Some(2);
                                    }
                                });
                            });
                    },
                );
            });
            match choice {
                Some(0) => {
                    if let Some(host) = &mut self.session_host {
                        if let Some(profile) = &mut host.vnc {
                            profile.tunnel_via_ssh = true;
                        }
                    }
                    self.confirm_unencrypted_vnc = false;
                    ctx.request_repaint();
                }
                Some(1) => {
                    self.confirm_unencrypted_vnc = false;
                    ctx.request_repaint();
                }
                Some(2) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
            return;
        }
        let (
            mut updates,
            status,
            connected,
            reconnect_at,
            finished,
            password_retry,
            connected_since,
            received_bytes,
            sent_bytes,
            display_updates,
            remote_cursor,
            remote_clipboard_file_count,
            clipboard_file_download,
        ) = if let Some(connection) = &self.connection {
            let mut connection = connection.lock().unwrap();
            if connection.clipboard_sent.swap(false, Ordering::Relaxed) {
                self.clipboard_notice = Some((Instant::now(), "Clipboard sent".into()));
            }
            if connection
                .clipboard_unsupported
                .swap(false, Ordering::Relaxed)
            {
                self.clipboard_notice = Some((
                    Instant::now(),
                    "Classic VNC supports Latin-1 text only; images are unsupported".into(),
                ));
            }
            if std::mem::take(&mut connection.clipboard_too_large) {
                self.clipboard_notice =
                    Some((Instant::now(), "Clipboard exceeds the 1 MiB limit".into()));
            }
            if let Some(text) = connection.clipboard_text.take() {
                if let Some(clipboard) = &self.clipboard_sync {
                    clipboard.set_remote_text(text);
                } else {
                    self.clipboard_notice = Some((Instant::now(), "Clipboard received".into()));
                    ctx.copy_text(text);
                }
            }
            if let Some(image) = connection.clipboard_image.take() {
                if let Some(clipboard) = &self.clipboard_sync {
                    clipboard.set_remote_image(image);
                }
            }
            (
                if connection.receiving_frame {
                    Vec::new()
                } else {
                    connection.framebuffer.take_updates()
                },
                connection.status.clone(),
                connection.connected,
                connection.reconnect_at,
                connection.finished,
                connection.password_retry,
                connection.connected_since,
                connection.received_bytes.load(Ordering::Relaxed),
                connection.sent_bytes.load(Ordering::Relaxed),
                connection.display_updates,
                connection.vnc_cursor.as_ref().map(|image| {
                    (
                        image.clone(),
                        connection.vnc_cursor_hotspot,
                        connection.vnc_cursor_position,
                        connection.vnc_cursor_revision,
                    )
                }),
                connection.remote_clipboard_file_count,
                connection.clipboard_file_download.take(),
            )
        } else {
            (
                self.framebuffer.take_updates(),
                self.message.into(),
                false,
                None,
                false,
                false,
                None,
                0,
                0,
                0,
                None,
                0,
                None,
            )
        };
        if finished
            && !connected
            && status != "Session ended"
            && status != "Disconnected"
            && self.last_connection_error.as_deref() != Some(&status)
        {
            self.last_connection_error = Some(status.clone());
            self.connection_error_notice = Some((Instant::now(), status.clone()));
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.sync_native_keyboard_capture(ctx, ctx.input(|input| input.focused), connected);
        if let Some(result) = clipboard_file_download {
            self.clipboard_notice = Some((Instant::now(), result));
        }
        if should_close_after_failure(self.close_on_failure, finished, connected) && !password_retry
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if password_retry {
            self.texture = None;
            self.vnc_cursor_texture = None;
        }
        if self.show_vnc_cursor {
            if let Some((image, _, _, revision)) = &remote_cursor {
                if self.vnc_cursor_revision != *revision {
                    self.vnc_cursor_revision = *revision;
                    self.vnc_cursor_texture = Some(ctx.load_texture(
                        "remote-vnc-cursor",
                        (**image).clone(),
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
        } else if self.protocol == DesktopProtocol::Vnc {
            self.vnc_cursor_texture = None;
        }
        if self.protocol == DesktopProtocol::Rdp
            && self.rdp_frame_painting == RdpFramePainting::FullFrame
        {
            if let Some(connection) = &self.connection {
                paint_full_frame(&connection.lock().unwrap().framebuffer, &mut updates);
            }
        }
        if let Some(clipboard) = &self.clipboard_sync {
            clipboard.set_focused(connected && ctx.input(|input| input.focused));
            if let Some(notice) = clipboard.take_notice() {
                self.clipboard_notice = Some((Instant::now(), notice.into()));
            }
            if connected && self.send_clipboard && !self.view_only {
                if let Some(connection) = &self.connection {
                    if let Some(pending) = clipboard.take_pending() {
                        let event = match pending {
                            super::clipboard_sync::PendingClipboard::Text(text) => {
                                Input::ClipboardText(text)
                            }
                            super::clipboard_sync::PendingClipboard::Image(image) => {
                                Input::ClipboardImage(image)
                            }
                        };
                        connection.lock().unwrap().send_input(event);
                    }
                }
            }
        }
        if self.screenshot_queued {
            if connected {
                self.take_screenshot(ctx);
            } else {
                self.screenshot_queued = false;
                self.screenshot_result = Some(Err(
                    "Session ended before the screenshot was captured.".into(),
                ));
            }
        }
        if !connected {
            self.capture = Capture::new(self.left_handed_mouse, self.disable_smooth_scrolling);
        }
        if connected && self.show_update_rate {
            let (since, previous_rx, previous_tx, frames, uploads) =
                self.traffic_sample.get_or_insert_with(|| {
                    (
                        Instant::now(),
                        received_bytes,
                        sent_bytes,
                        display_updates,
                        self.texture_uploads,
                    )
                });
            let elapsed = since.elapsed();
            let interval = Duration::from_secs(1);
            if elapsed >= interval {
                self.live_fps = updates_per_second(*frames, display_updates, elapsed);
                self.download_bytes_per_second =
                    received_bytes.saturating_sub(*previous_rx) as f64 / elapsed.as_secs_f64();
                self.upload_bytes_per_second =
                    sent_bytes.saturating_sub(*previous_tx) as f64 / elapsed.as_secs_f64();
                self.texture_uploads_per_second =
                    updates_per_second(*uploads, self.texture_uploads, elapsed);
                self.traffic_sample = Some((
                    Instant::now(),
                    received_bytes,
                    sent_bytes,
                    display_updates,
                    self.texture_uploads,
                ));
                ctx.request_repaint_after(interval);
            } else {
                ctx.request_repaint_after(interval - elapsed);
            }
        } else {
            self.traffic_sample = None;
            self.texture_uploads_per_second = 0.0;
            self.download_bytes_per_second = 0.0;
            self.upload_bytes_per_second = 0.0;
            self.live_fps = 0.0;
        }
        let texture_options =
            if self.protocol == DesktopProtocol::Rdp && self.rdp_painting == RdpPainting::Fast {
                egui::TextureOptions::NEAREST
            } else {
                egui::TextureOptions::LINEAR
            };
        self.texture_uploads = self.texture_uploads.wrapping_add(updates.len() as u64);
        for update in updates {
            match self.texture.as_mut() {
                Some(texture) if !update.resized => {
                    texture.set_partial(update.origin, update.image, texture_options);
                }
                Some(texture) => texture.set(update.image, texture_options),
                None => {
                    self.texture =
                        Some(ctx.load_texture("remote-frame", update.image, texture_options));
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
        if let Some(recorder) = &self.recording {
            let result = match recorder.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Recording worker stopped.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                self.recording = None;
                self.recording_result = Some(result);
            }
        }
        let toolbar = |ui: &mut egui::Ui| {
            ui.set_min_height(36.0);
            let show_rates = self.show_update_rate && connected;
            let rates_width = if show_rates { 270.0 } else { 0.0 };
            let controls_width = (ui.available_width() - rates_width - 8.0).max(0.0);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(controls_width, 30.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("remote_desktop_toolbar_controls")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                let status_text = if self.test_pattern {
                    "Test pattern"
                } else if connected {
                    &status
                } else {
                    "Disconnected"
                };
                let status_icon = if self.test_pattern {
                    crate::lucide::IMAGE
                } else if connected {
                    crate::lucide::CIRCLE_DOT
                } else {
                    crate::lucide::ALERT_TRIANGLE
                };
                let status_color = if self.test_pattern {
                    ui.visuals().text_color()
                } else if connected {
                    egui::Color32::from_rgb(52, 211, 153)
                } else {
                    ui.visuals().warn_fg_color
                };
                egui::Frame::none()
                    .fill(ui.visuals().widgets.inactive.bg_fill)
                    .rounding(egui::Rounding::same(999.0))
                    .inner_margin(egui::Margin::symmetric(9.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("{status_icon} {status_text}"))
                                .size(11.5)
                                .color(status_color),
                        );
                    });
                ui.separator();
                egui::ComboBox::from_id_salt("remote_scaling")
                    .selected_text(format!("{} {}", crate::lucide::MAXIMIZE, scale_label(self.scaling)))
                    .width(88.0)
                    .show_ui(ui, |ui| {
                        for scale in [Scaling::Fit, Scaling::Native, Scaling::Fill] {
                            ui.selectable_value(&mut self.scaling, scale, scale_label(scale));
                        }
                    });
                ui.add_enabled_ui(connected && !self.view_only, |ui| {
                    let keyboard_capture = self.capture.keyboard_capture();
                    if toolbar_toggle_button(
                        ui,
                        crate::lucide::KEYBOARD,
                        if keyboard_capture {
                            "Keyboard input is captured by the remote viewer"
                        } else {
                            "Capture keyboard input for the remote viewer"
                        },
                        keyboard_capture,
                        true,
                    )
                    .clicked()
                    {
                        if let Some(connection) = &self.connection {
                            let enable = !keyboard_capture;
                            let connection_handle = connection.clone();
                            let connection = connection_handle.lock().unwrap();
                            self.capture.set_keyboard_capture(enable, |event| {
                                connection.send_input(event)
                            });
                            drop(connection);
                            #[cfg(target_os = "linux")]
                            {
                                self.native_keyboard = None;
                                self.native_keyboard_attempted = false;
                                self.keyboard_capture_feedback = None;
                            }
                            #[cfg(target_os = "windows")]
                            {
                                self.native_keyboard = None;
                                self.native_keyboard_attempted = false;
                                self.keyboard_capture_feedback = None;
                            }
                            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                            {
                                self.keyboard_capture_feedback = enable.then(|| {
                                    "System shortcuts may stay local; use Send keys to forward them."
                                        .into()
                                });
                            }
                        }
                    }
                    if let Some(feedback) = &self.keyboard_capture_feedback {
                        ui.label(feedback);
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
                        if ui.button("Edit settings").clicked() {
                            let mut output = std::io::stdout().lock();
                            if writeln!(output, "{}", super::session::EDIT_SETTINGS)
                                .and_then(|()| output.flush())
                                .is_err()
                            {
                                self.clipboard_notice =
                                    Some((Instant::now(), "Could not open settings".into()));
                            }
                        }
                        if ui.button("Retry").clicked() {
                            self.connection = Some(start_connection(
                                host.clone(),
                                &self.config_path,
                                self.protocol,
                                if self.dynamic_resize {
                                    self.last_window_size
                                } else {
                                    None
                                },
                                None,
                                ctx,
                            ));
                            self.texture = None;
                            ctx.request_repaint();
                        }
                    }
                }
                if toolbar_toggle_button(
                    ui,
                    crate::lucide::ACTIVITY,
                    "Show remote update rates",
                    self.show_update_rate,
                    true,
                )
                .clicked()
                {
                    self.show_update_rate = !self.show_update_rate;
                    ctx.request_repaint();
                }
                ui.add_enabled_ui(connected, |ui| {
                    let stats = ui.menu_button(toolbar_icon_text(crate::lucide::INFO), |ui| {
                        ui.set_min_width(360.0);
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
                            let now = Instant::now();
                            if self.receive_sample.map_or(true, |(sampled_at, _)| {
                                sampled_at.elapsed() >= Duration::from_secs(1)
                            }) {
                                if let Some((sampled_at, previous_bytes)) = self.receive_sample {
                                    self.receive_kbps = receive_kilobits_per_second(
                                        previous_bytes,
                                        received_bytes,
                                        sampled_at.elapsed(),
                                    );
                                }
                                self.receive_sample = Some((now, received_bytes));
                            }
                            ui.label(format!("Live receive rate  {:.1} kbps", self.receive_kbps));
                            ctx.request_repaint_after(Duration::from_secs(1));
                        }
                        if self.show_update_rate {
                            ui.label(format!(
                                "Live FPS  {:.1}",
                                self.live_fps
                            ));
                            ui.label(format!(
                                "Texture uploads/s  {:.1}",
                                self.texture_uploads_per_second
                            ));
                        }
                    });
                    stats.response.on_hover_text("Connection statistics");
                });
                if self.protocol == DesktopProtocol::Rdp {
                    if let Some(connection) = &self.connection {
                        let mut connection = connection.lock().unwrap();
                        let mut resize_to_fit = self.dynamic_resize;
                        let mut requested_size = None;
                        let response = ui
                            .add_enabled_ui(
                                connected && !self.view_only && connection.rdp_resize_supported,
                                |ui| {
                                    ui.menu_button(
                                        toolbar_icon_text(crate::lucide::MONITOR),
                                        |ui| {
                                            ui.checkbox(&mut resize_to_fit, "Resize to fit window");
                                            ui.separator();
                                            for size in [
                                                [1024, 768],
                                                [1280, 720],
                                                [1366, 768],
                                                [1600, 900],
                                                [1920, 1080],
                                            ] {
                                                if ui
                                                    .button(format!("{} × {}", size[0], size[1]))
                                                    .clicked()
                                                {
                                                    requested_size = Some(size);
                                                    ui.close_menu();
                                                }
                                            }
                                        },
                                    )
                                    .response
                                }
                            )
                            .inner
                            .on_hover_text("Change remote display resolution");
                        if resize_to_fit != self.dynamic_resize {
                            self.dynamic_resize = resize_to_fit;
                            connection.rdp_resize_enabled = resize_to_fit;
                            connection.resize_request = None;
                            self.last_window_size = None;
                        }
                        if let Some(size) = requested_size {
                            self.dynamic_resize = false;
                            connection.rdp_resize_enabled = true;
                            connection.resize_request = Some(size);
                            self.last_window_size = Some(size);
                        }
                        if !connection.rdp_resize_supported {
                            response.on_disabled_hover_text(
                                "Waiting for RDP Display Control support from the server.",
                            );
                        }
                    }
                }
                if self.protocol == DesktopProtocol::Vnc {
                    if let Some(connection) = &self.connection {
                        let connection = connection.lock().unwrap();
                        let menu = ui
                            .add_enabled_ui(
                                connected && !self.view_only && connection.vnc_screen.is_some(),
                                |ui| {
                                    ui.menu_button(toolbar_icon_text(crate::lucide::MONITOR), |ui| {
                                        for size in [
                                            [1024, 768],
                                            [1280, 720],
                                            [1366, 768],
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
                            .inner
                            .on_hover_text("Change remote display resolution");
                        if connection.vnc_screen.is_none() {
                            menu.on_disabled_hover_text(
                                "The VNC server must support single-screen remote resizing.",
                            );
                        }
                    }
                }
                if toolbar_icon_button(
                    ui,
                    if fullscreen { crate::lucide::MINIMIZE } else { crate::lucide::MAXIMIZE },
                    if fullscreen { "Exit fullscreen" } else { "Enter fullscreen" },
                    true,
                )
                .clicked()
                {
                    self.fullscreen = !fullscreen;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
                if remote_clipboard_file_count > 0 {
                    let response = ui.add_enabled(
                        connected
                            && !self.view_only
                            && self.receive_clipboard
                            && self.session_host.is_some(),
                        egui::Button::new(
                            toolbar_icon_text(&format!("{} {remote_clipboard_file_count}", crate::lucide::DOWNLOAD)),
                        )
                        .min_size(egui::vec2(30.0, 28.0)),
                    );
                    if response
                        .on_hover_text(if self.protocol == DesktopProtocol::Vnc {
                            "Download remote clipboard files over SSH."
                        } else {
                            "Save files currently offered by the remote clipboard."
                        })
                        .clicked()
                    {
                        if let Some(directory) = rfd::FileDialog::new()
                            .set_title("Choose a folder for remote clipboard files")
                            .pick_folder()
                        {
                            if let Some(connection) = &self.connection {
                                connection
                                    .lock()
                                    .unwrap()
                                    .send_input(Input::DownloadClipboardFiles(directory));
                            }
                        }
                    }
                }
                if self.protocol == DesktopProtocol::Rdp {
                    let response = ui.add_enabled(
                        connected && !self.view_only && self.send_clipboard,
                        egui::Button::new(toolbar_icon_text(crate::lucide::CLIPBOARD))
                            .min_size(egui::vec2(30.0, 28.0)),
                    );
                    if response
                        .on_hover_text("Share files through the remote clipboard")
                        .clicked()
                    {
                        if let Some(paths) = rfd::FileDialog::new()
                            .set_title("Share files with the remote PC")
                            .pick_files()
                        {
                            if let Some(connection) = &self.connection {
                                connection.lock().unwrap().send_input(Input::ClipboardFiles(paths));
                                self.clipboard_notice = Some((
                                    Instant::now(),
                                    "Files offered to the remote clipboard".into(),
                                ));
                            }
                        }
                    }
                }
                if !connected && self.texture.is_some() && !self.test_pattern {
                    ui.label(&status);
                }
                let screenshot_tooltip = if self.screenshot_queued {
                    "Waiting for a remote frame"
                } else if self.screenshot.is_some() {
                    "Saving screenshot"
                } else {
                    "Save screenshot"
                };
                if toolbar_icon_button(
                    ui,
                    crate::lucide::IMAGE,
                    screenshot_tooltip,
                    self.texture.is_some() && self.screenshot.is_none() && !self.screenshot_queued,
                )
                .clicked()
                {
                    self.take_screenshot(ctx);
                }
                if let Some(recorder) = &self.recording {
                    ui.label(
                        egui::RichText::new(format!("{} REC", crate::lucide::CIRCLE_DOT))
                            .color(ui.visuals().error_fg_color),
                    );
                    let pause = if recorder.paused() {
                        "Resume recording"
                    } else {
                        "Pause recording"
                    };
                    if toolbar_icon_button(
                        ui,
                        if recorder.paused() { crate::lucide::PLAY } else { crate::lucide::PAUSE },
                        pause,
                        true,
                    )
                    .clicked()
                    {
                        recorder.toggle_pause();
                        ctx.request_repaint();
                    }
                    if toolbar_icon_button(ui, crate::lucide::STOP, "Stop recording", true).clicked() {
                        recorder.stop();
                    }
                } else if ui
                    .add_enabled(
                        connected && self.texture.is_some(),
                        egui::Button::new(toolbar_icon_text(crate::lucide::CIRCLE_DOT))
                            .min_size(egui::vec2(30.0, 28.0)),
                    )
                    .on_hover_text("Start recording")
                    .clicked()
                {
                    self.toggle_recording(ctx);
                }
                if toolbar_toggle_button(
                    ui,
                    crate::lucide::COPY,
                    "Copy screenshots to the clipboard",
                    self.copy_screenshot,
                    true,
                )
                .clicked()
                {
                    self.copy_screenshot = !self.copy_screenshot;
                }
                ui.add_enabled_ui(connected && !self.view_only, |ui| {
                    let send_keys = ui.menu_button(toolbar_icon_text(crate::lucide::KEY), |ui| {
                        let shortcuts: &[(&str, &[u32])] = &[
                            ("Ctrl+Alt+Del", &[0xffe3, 0xffe9, 0xffff]),
                            ("Alt+Tab", &[0xffe9, 0xff09]),
                            ("Alt+F4", &[0xffe9, 0xffc1]),
                            ("Super / Windows key", &[0xffeb]),
                            ("Super+Tab", &[0xffeb, 0xff09]),
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
                    send_keys
                        .response
                        .on_hover_text("Send a keyboard shortcut to the remote PC");
                });
                let view_only_response = toolbar_toggle_button(
                    ui,
                    if self.view_only { crate::lucide::EYE } else { crate::lucide::EYE_OFF },
                    if self.view_only { "Disable view-only mode" } else { "Enable view-only mode" },
                    self.view_only,
                    connected,
                );
                if view_only_response.clicked() {
                    self.view_only = !self.view_only;
                    #[cfg(target_os = "linux")]
                    {
                        self.native_keyboard.take();
                        self.native_keyboard_attempted = false;
                    }
                    #[cfg(target_os = "windows")]
                    {
                        self.native_keyboard.take();
                        self.native_keyboard_attempted = false;
                    }
                    if let Some(connection) = &self.connection {
                        let connection = connection.lock().unwrap();
                        if self.view_only {
                            self.capture
                                .release(|event| connection.send_input(event));
                        } else {
                            self.capture.set_keyboard_capture(true, |_| {});
                        }
                    }
                }
                if connected {
                    let label = if self.toolbar_pinned { "Unpin" } else { "Pin" };
                    if ui
                        .add(
                            egui::Button::new(label)
                                .selected(self.toolbar_pinned)
                                .min_size(egui::vec2(48.0, 28.0))
                                .rounding(egui::Rounding::same(8.0)),
                        )
                        .on_hover_text(if self.toolbar_pinned {
                            "Unpin toolbar"
                        } else {
                            "Keep toolbar visible"
                        })
                        .clicked()
                    {
                        self.toolbar_pinned = !self.toolbar_pinned;
                        self.toolbar_until = Some(now + Duration::from_secs(2));
                        ctx.request_repaint();
                    }
                }
                if toolbar_icon_button(
                    ui,
                    crate::lucide::LOG_OUT,
                    "Disconnect remote desktop",
                    true,
                )
                .clicked()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                                });
                            });
                    },
                );
                if show_rates {
                    ui.separator();
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(format!("{} {}/s", crate::lucide::UPLOAD, crate::sensors::format_bytes(self.upload_bytes_per_second as u64)));
                            ui.label(format!("{} {}/s", crate::lucide::DOWNLOAD, crate::sensors::format_bytes(self.download_bytes_per_second as u64)));
                            ui.label(format!("{} {:.1} FPS", crate::lucide::GAUGE, self.live_fps));
                        },
                    );
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
        if let Some((shown, label)) = self.connection_error_notice.clone() {
            let remaining = Duration::from_secs(8).saturating_sub(shown.elapsed());
            if !remaining.is_zero() {
                let mut dismiss = false;
                egui::Area::new(egui::Id::new("remote_connection_error"))
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 42.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(egui::Color32::from_rgb(75, 25, 28))
                            .show(ui, |ui| {
                                ui.set_max_width(380.0);
                                ui.horizontal(|ui| {
                                    ui.label(toolbar_icon_text(crate::lucide::ALERT_TRIANGLE));
                                    ui.label(label);
                                    dismiss = ui
                                        .small_button(toolbar_icon_text(crate::lucide::X))
                                        .clicked();
                                });
                            });
                    });
                if dismiss {
                    self.connection_error_notice = None;
                } else {
                    ctx.request_repaint_after(remaining);
                }
            } else {
                self.connection_error_notice = None;
            }
        }
        if let Some((shown, label)) = self.clipboard_notice.clone() {
            let remaining = Duration::from_secs(2).saturating_sub(shown.elapsed());
            if !remaining.is_zero() {
                egui::Area::new(egui::Id::new("clipboard_activity"))
                    .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 92.0))
                    .order(egui::Order::Foreground)
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(label));
                    });
                ctx.request_repaint_after(remaining);
            } else {
                self.clipboard_notice = None;
            }
        }
        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if let Some(result) = self
            .upload
            .as_mut()
            .and_then(crate::app::DesktopUpload::poll)
        {
            self.upload = None;
            self.upload_result = Some(result);
        }
        if connected && !self.view_only && !dropped.is_empty() {
            if self.upload.is_some() {
                self.upload_result = Some(Err("An upload is already running.".into()));
            } else if let Some(host) = &self.session_host {
                self.upload = Some(crate::app::DesktopUpload::start(host, dropped, ctx.clone()));
                self.upload_result = None;
            }
        }
        if self.upload.is_some() || self.upload_result.is_some() {
            egui::Area::new(egui::Id::new("viewer_upload"))
                .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(12.0, -12.0))
                .order(egui::Order::Foreground)
                .movable(false)
                .show(ctx, |ui| {
                    ui.set_max_width((screen.width() - 24.0).min(460.0));
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.vertical(|ui| {
                            if let Some(upload) = &self.upload {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spinner();
                                    ui.label(&upload.progress);
                                    if ui.button("Cancel upload").clicked() {
                                        upload.cancel();
                                    }
                                });
                                if let Some(fraction) = upload.fraction {
                                    ui.add(
                                        egui::ProgressBar::new(fraction)
                                            .desired_width(ui.available_width())
                                            .show_percentage(),
                                    );
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if let Some(speed) = upload.speed {
                                        ui.label(format!(
                                            "{} /s",
                                            crate::sensors::format_bytes(speed)
                                        ));
                                    } else {
                                        ui.label("Measuring speed…");
                                    }
                                    if let Some(next) = &upload.next {
                                        ui.separator();
                                        ui.label(format!("Next: {next}"));
                                    }
                                });
                            }
                            if let Some(result) = &self.upload_result {
                                let message = match result {
                                    Ok(message) | Err(message) => message.clone(),
                                };
                                if ui
                                    .horizontal_wrapped(|ui| {
                                        ui.label(message);
                                        ui.button("Dismiss").clicked()
                                    })
                                    .inner
                                {
                                    self.upload_result = None;
                                }
                            }
                        });
                    });
                });
        }
        if self.recording.is_some() || self.recording_result.is_some() {
            egui::Area::new(egui::Id::new("recording_indicator"))
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 70.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        if let Some(recorder) = &self.recording {
                            let status = if recorder.paused() {
                                "Recording paused"
                            } else {
                                "● REC"
                            };
                            let seconds = recorder.duration().as_secs();
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!(
                                    "{status} {:02}:{:02} · {}",
                                    seconds / 60,
                                    seconds % 60,
                                    crate::sensors::format_bytes(recorder.output_bytes())
                                ),
                            );
                            if !recorder.paused() {
                                ctx.request_repaint_after(Duration::from_secs(1));
                            }
                        } else if let Some(result) = &self.recording_result {
                            match result {
                                Ok(path) => {
                                    ui.label(format!(
                                        "{} {}",
                                        recording_result_label(
                                            self.recording_format,
                                            path.is_dir(),
                                        ),
                                        path.display()
                                    ));
                                }
                                Err(error) => {
                                    ui.label(error);
                                }
                            }
                            if ui.button("Dismiss").clicked() {
                                self.recording_result = None;
                            }
                        }
                    });
                });
        }
        self.screenshot_notification(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dynamic_resize {
                let size = ui.available_size() * ctx.pixels_per_point();
                let dimensions = super::rdp::display_dimensions(size.x as u32, size.y as u32);
                if self.last_window_size != Some(dimensions) {
                    self.last_window_size = Some(dimensions);
                    if connected {
                        if let Some(connection) = &self.connection {
                            let mut connection = connection.lock().unwrap();
                            connection.rdp_resize_enabled = true;
                            connection.resize_request = Some(dimensions);
                        }
                    } else if self.connection.is_none() {
                        ctx.request_repaint();
                    }
                }
            }
            let Some(texture) = &self.texture else {
                self.send_loading_keyboard(ctx);
                if password_retry && self.session_host.is_some() {
                    let mut retry = false;
                    ui.with_layout(
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_max_width(420.0);
                                ui.heading("Sign-in needs a password");
                                ui.label(&status);
                                ui.add_space(8.0);
                                ui.label(
                                    "Enter a password for this session. It will not be saved.",
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut *self.password_entry)
                                        .password(true)
                                        .hint_text("Password"),
                                );
                                if self.protocol == DesktopProtocol::Vnc {
                                    ui.label("Classic VNC accepts at most 8 password bytes.");
                                }
                                let valid = !self.password_entry.is_empty()
                                    && (self.protocol != DesktopProtocol::Vnc
                                        || self.password_entry.len() <= 8);
                                retry = ui
                                    .add_enabled(
                                        valid,
                                        egui::Button::new("Connect for this session"),
                                    )
                                    .clicked();
                            });
                        },
                    );
                    if retry {
                        if let Some(host) = &self.session_host {
                            let password = std::mem::take(&mut self.password_entry);
                            self.connection = Some(start_connection(
                                host.clone(),
                                &self.config_path,
                                self.protocol,
                                if self.dynamic_resize {
                                    self.last_window_size
                                } else {
                                    None
                                },
                                Some(password),
                                ctx,
                            ));
                            self.texture = None;
                            ctx.request_repaint();
                        }
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if finished {
                            "Connection ended"
                        } else {
                            &status
                        });
                    });
                }
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
            if self.protocol == DesktopProtocol::Vnc && self.show_vnc_cursor {
                if let (Some(texture), Some((_, hotspot, Some(position), _))) =
                    (&self.vnc_cursor_texture, &remote_cursor)
                {
                    let scale = egui::vec2(
                        screen.width() / pixels[0] as f32,
                        screen.height() / pixels[1] as f32,
                    );
                    let cursor_origin = screen.min
                        + egui::vec2(
                            (position[0] as f32 - hotspot[0] as f32) * scale.x,
                            (position[1] as f32 - hotspot[1] as f32) * scale.y,
                        );
                    ui.painter().with_clip_rect(clip).image(
                        texture.id(),
                        egui::Rect::from_min_size(cursor_origin, texture.size_vec2() * scale),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
            self.send_input(ctx, screen, clip, pixels);
        });
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.handle_native_shortcuts(ctx);
        self.send_keys_window(ctx);
    }
}

fn key_sequence(keys: &[u32]) -> impl Iterator<Item = (u32, bool)> + '_ {
    keys.iter()
        .map(|&key| (key, true))
        .chain(keys.iter().rev().map(|&key| (key, false)))
}

fn should_close_after_failure(close_on_failure: bool, finished: bool, connected: bool) -> bool {
    close_on_failure && finished && !connected
}

fn recording_result_label(
    format: crate::config::RemoteRecordingFormat,
    is_directory: bool,
) -> &'static str {
    match (format, is_directory) {
        (crate::config::RemoteRecordingFormat::PngSequence, _) => "PNG sequence saved to",
        (_, true) => "FFmpeg unavailable. PNG frames saved to",
        (_, false) => "Recording saved to",
    }
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

#[cfg(test)]
#[test]
fn close_on_failure_waits_until_retries_finish() {
    assert!(!should_close_after_failure(true, false, false));
    assert!(!should_close_after_failure(true, true, true));
    assert!(should_close_after_failure(true, true, false));
    assert!(!should_close_after_failure(false, true, false));
}

#[cfg(test)]
#[test]
fn recording_result_distinguishes_selected_png_from_fallback() {
    use crate::config::RemoteRecordingFormat::{Mp4, PngSequence};

    assert_eq!(
        recording_result_label(PngSequence, true),
        "PNG sequence saved to"
    );
    assert_eq!(
        recording_result_label(Mp4, true),
        "FFmpeg unavailable. PNG frames saved to"
    );
    assert_eq!(recording_result_label(Mp4, false), "Recording saved to");
}

fn scale_label(scale: Scaling) -> &'static str {
    match scale {
        Scaling::Fit => "Fit",
        Scaling::Native => "100%",
        Scaling::Fill => "Fill",
    }
}

fn updates_per_second(previous: u64, current: u64, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        0.0
    } else {
        current.saturating_sub(previous) as f64 / elapsed.as_secs_f64()
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::session_persistence_path;
    use crate::remote_desktop::credentials::DesktopProtocol;
    use std::path::Path;

    #[test]
    fn window_state_is_stable_and_isolated_by_host_and_protocol() {
        let config = Path::new("/tmp/servers.json");
        let rdp = session_persistence_path(config, "host-a", DesktopProtocol::Rdp);
        assert_eq!(
            rdp,
            session_persistence_path(config, "host-a", DesktopProtocol::Rdp)
        );
        assert_ne!(
            rdp,
            session_persistence_path(config, "host-b", DesktopProtocol::Rdp)
        );
        assert_ne!(
            rdp,
            session_persistence_path(config, "host-a", DesktopProtocol::Vnc)
        );
        assert_eq!(rdp.parent(), config.parent());
    }
}

fn receive_kilobits_per_second(previous: u64, current: u64, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        0.0
    } else {
        current.saturating_sub(previous) as f64 * 8.0 / elapsed.as_secs_f64() / 1000.0
    }
}

fn paint_full_frame(framebuffer: &Framebuffer, updates: &mut Vec<Update>) {
    if updates.is_empty() {
        return;
    }
    if let Some(image) = framebuffer.snapshot() {
        let resized = updates.iter().any(|update| update.resized);
        *updates = vec![Update {
            origin: [0, 0],
            image,
            resized,
        }];
    }
}

#[cfg(test)]
#[test]
fn full_frame_painting_combines_damage_into_one_complete_image() {
    let mut framebuffer = Framebuffer::default();
    framebuffer.resize([2, 1]).unwrap();
    let _ = framebuffer.take_updates();
    framebuffer
        .write_rgba(
            Region {
                origin: [1, 0],
                size: [1, 1],
            },
            &[12, 34, 56, 255],
        )
        .unwrap();
    let mut updates = framebuffer.take_updates();
    paint_full_frame(&framebuffer, &mut updates);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].origin, [0, 0]);
    assert_eq!(updates[0].image.size, [2, 1]);
    assert_eq!(
        updates[0].image.pixels[1],
        egui::Color32::from_rgb(12, 34, 56)
    );
}

#[cfg(test)]
#[test]
fn update_rate_uses_received_display_updates() {
    assert_eq!(updates_per_second(10, 16, Duration::from_secs(2)), 3.0);
    assert_eq!(updates_per_second(16, 2, Duration::from_secs(1)), 0.0);
    assert_eq!(updates_per_second(1, 2, Duration::ZERO), 0.0);
}

#[cfg(test)]
#[test]
fn live_receive_rate_uses_the_latest_interval() {
    assert_eq!(
        receive_kilobits_per_second(1_000, 2_000, Duration::from_secs(2)),
        4.0
    );
    assert_eq!(
        receive_kilobits_per_second(2_000, 1_000, Duration::from_secs(1)),
        0.0
    );
    assert_eq!(receive_kilobits_per_second(0, 1, Duration::ZERO), 0.0);
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
