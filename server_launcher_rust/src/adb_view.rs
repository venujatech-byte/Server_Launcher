use egui::{pos2, Align, Align2, Layout, Rect, Vec2};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdbScreenMirrorMode {
    #[default]
    BuiltIn,
    Scrcpy,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdbSubTab {
    #[default]
    Overview,
    ScreenShare,
    Messages,
    Calls,
    Photos,
    Notifications,
    Storage,
    Apps,
    Clipboard,
    Terminal,
    Wireless,
    Firmware,
}

#[allow(dead_code)]
impl AdbSubTab {
    pub fn all() -> &'static [AdbSubTab] {
        &[
            AdbSubTab::Overview,
            AdbSubTab::ScreenShare,
            AdbSubTab::Messages,
            AdbSubTab::Calls,
            AdbSubTab::Photos,
            AdbSubTab::Notifications,
            AdbSubTab::Storage,
            AdbSubTab::Apps,
            AdbSubTab::Clipboard,
            AdbSubTab::Terminal,
            AdbSubTab::Wireless,
            AdbSubTab::Firmware,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AdbSubTab::Overview => "Overview",
            AdbSubTab::ScreenShare => "Screen Share",
            AdbSubTab::Messages => "Messages",
            AdbSubTab::Calls => "Calls",
            AdbSubTab::Photos => "Photos",
            AdbSubTab::Notifications => "Notifications",
            AdbSubTab::Storage => "Storage",
            AdbSubTab::Apps => "Apps",
            AdbSubTab::Clipboard => "Clipboard",
            AdbSubTab::Terminal => "Terminal",
            AdbSubTab::Wireless => "Wireless",
            AdbSubTab::Firmware => "Firmware",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            AdbSubTab::Overview => crate::lucide::SMARTPHONE,
            AdbSubTab::ScreenShare => crate::lucide::SCREEN_SHARE,
            AdbSubTab::Messages => crate::lucide::MESSAGE_SQUARE,
            AdbSubTab::Calls => crate::lucide::PHONE,
            AdbSubTab::Photos => crate::lucide::IMAGE,
            AdbSubTab::Notifications => crate::lucide::BELL,
            AdbSubTab::Storage => crate::lucide::STORAGE,
            AdbSubTab::Apps => crate::lucide::LAYOUT_GRID,
            AdbSubTab::Clipboard => crate::lucide::CLIPBOARD,
            AdbSubTab::Terminal => crate::lucide::TERMINAL,
            AdbSubTab::Wireless => crate::lucide::WIFI,
            AdbSubTab::Firmware => crate::lucide::ZAP,
        }
    }
}

#[allow(dead_code)]
impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // ADD DEVICE TYPE SELECTOR MODAL (SSH PC vs Android ADB)
    // ═════════════════════════════════════════════════════════════════════════
    pub fn render_add_device_selector_modal(&mut self, ctx: &egui::Context) {
        if !self.add_device_selector_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.add_device_selector_modal_open = false;
            return;
        }

        let p = self.palette();
        let screen = ctx.screen_rect();
        let modal_w = 560.0_f32.min(screen.width() - 32.0);

        // 1. Semi-transparent backdrop overlay with click-outside dismissal
        let backdrop_area = egui::Area::new(egui::Id::new("add_device_selector_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle);
        let bg_resp = backdrop_area.show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(150));
            resp
        }).inner;
        if bg_resp.clicked() {
            self.add_device_selector_modal_open = false;
        }

        egui::Window::new("Add Connection or Device")
            .id(egui::Id::new("add_device_selector_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center())
            .pivot(Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(16, 17, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(18.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 14.0),
                        blur: 32.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(180),
                    })
                    .inner_margin(egui::Margin::symmetric(20.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    Frame::none()
                        .fill(p.accent_subtle)
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(crate::lucide::PLUS).size(14.0).color(p.accent));
                        });
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Add Connection or Device").size(15.0).strong().color(p.text_primary));
                        ui.label(RichText::new("Choose the connection target type to configure").size(11.0).color(p.text_muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn_close = egui::Button::new(RichText::new(crate::lucide::X).size(12.0).color(p.text_muted))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0))
                            .min_size(Vec2::splat(26.0));
                        if ui.add(btn_close).on_hover_text("Close dialog (Esc)").clicked() {
                            self.add_device_selector_modal_open = false;
                        }
                    });
                });

                ui.add_space(14.0);

                // Option 1: SSH Remote PC
                let mut choose_ssh = false;
                let (ssh_rect, ssh_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 100.0), egui::Sense::click());
                if ssh_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let ssh_hover = ssh_resp.hovered();
                let ssh_bg = if ssh_hover { Color32::from_rgb(22, 26, 38) } else { Color32::from_rgb(14, 16, 23) };
                let ssh_border = if ssh_hover { p.accent } else { Color32::from_rgb(34, 38, 54) };

                ui.painter().rect(ssh_rect, Rounding::same(12.0), ssh_bg, Stroke::new(1.0_f32, ssh_border));

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(ssh_rect.shrink(12.0)), |ui| {
                    ui.horizontal(|ui| {
                        Frame::none()
                            .fill(p.accent_subtle)
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 14.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(crate::lucide::SERVER).size(22.0).color(p.accent));
                            });
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("SSH Remote PC / Server").size(13.5).strong().color(p.text_primary));
                                ui.label(RichText::new("Linux • Unix • Cloud").size(9.5).monospace().color(p.accent));
                            });
                            ui.add_space(3.0);
                            ui.label(RichText::new("Connect via SSH protocol to manage Linux servers, systemd units, custom bash scripts, terminal sessions, and live sensors telemetry.")
                                .size(10.5).color(p.text_muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let arrow_col = if ssh_hover { p.accent } else { p.text_muted.gamma_multiply(0.6) };
                            ui.label(RichText::new(crate::lucide::CHEVRON_RIGHT).size(16.0).color(arrow_col));
                        });
                    });
                });
                if ssh_resp.clicked() {
                    choose_ssh = true;
                }

                ui.add_space(10.0);

                // Option 2: Android / ADB Device
                let mut choose_adb = false;
                let (adb_rect, adb_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 100.0), egui::Sense::click());
                if adb_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let adb_hover = adb_resp.hovered();
                let adb_bg = if adb_hover { Color32::from_rgb(22, 26, 38) } else { Color32::from_rgb(14, 16, 23) };
                let adb_border = if adb_hover { Color32::from_rgb(56, 189, 248) } else { Color32::from_rgb(34, 38, 54) };

                ui.painter().rect(adb_rect, Rounding::same(12.0), adb_bg, Stroke::new(1.0_f32, adb_border));

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(adb_rect.shrink(12.0)), |ui| {
                    ui.horizontal(|ui| {
                        Frame::none()
                            .fill(Color32::from_rgb(14, 40, 58))
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 14.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(crate::lucide::SMARTPHONE).size(22.0).color(Color32::from_rgb(56, 189, 248)));
                            });
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Android / ADB Device").size(13.5).strong().color(p.text_primary));
                                ui.label(RichText::new("USB & Wireless ADB").size(9.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                            });
                            ui.add_space(3.0);
                            ui.label(RichText::new("Full Android control suite: screen mirror (built-in & scrcpy), bidirectional clipboard share, interactive shell terminal, storage explorer, and firmware / recovery tools.")
                                .size(10.5).color(p.text_muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let arrow_col = if adb_hover { Color32::from_rgb(56, 189, 248) } else { p.text_muted.gamma_multiply(0.6) };
                            ui.label(RichText::new(crate::lucide::CHEVRON_RIGHT).size(16.0).color(arrow_col));
                        });
                    });
                });
                if adb_resp.clicked() {
                    choose_adb = true;
                }

                ui.add_space(4.0);

                if choose_ssh {
                    self.add_device_selector_modal_open = false;
                    self.ssh_host_modal.open_add();
                } else if choose_adb {
                    self.add_device_selector_modal_open = false;
                    self.adb_add_modal_open = true;
                    self.adb_detected_devices = crate::adb::list_devices();
                }
            });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // ADD / CONNECT ADB DEVICE MODAL
    // ═════════════════════════════════════════════════════════════════════════
    pub fn render_adb_add_modal(&mut self, ctx: &egui::Context) {
        if !self.adb_add_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.adb_add_modal_open = false;
            return;
        }

        let p = self.palette();
        let screen = ctx.screen_rect();
        let modal_w = 600.0_f32.min(screen.width() - 32.0);

        // Backdrop overlay
        let backdrop_area = egui::Area::new(egui::Id::new("adb_add_modal_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle);
        let bg_resp = backdrop_area.show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(150));
            resp
        }).inner;
        if bg_resp.clicked() {
            self.adb_add_modal_open = false;
        }

        egui::Window::new("Connect Android Device (ADB)")
            .id(egui::Id::new("adb_add_device_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center())
            .pivot(Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(16, 17, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(18.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 14.0),
                        blur: 32.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(180),
                    })
                    .inner_margin(egui::Margin::symmetric(20.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    Frame::none()
                        .fill(Color32::from_rgb(14, 40, 58))
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(crate::lucide::SMARTPHONE).size(14.0).color(Color32::from_rgb(56, 189, 248)));
                        });
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Connect Android Device (ADB)").size(15.0).strong().color(p.text_primary));
                        ui.label(RichText::new("Pair attached USB devices or connect via Wireless ADB (TCP/IP)").size(11.0).color(p.text_muted));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn_close = egui::Button::new(RichText::new(crate::lucide::X).size(12.0).color(p.text_muted))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0))
                            .min_size(Vec2::splat(26.0));
                        if ui.add(btn_close).on_hover_text("Close dialog (Esc)").clicked() {
                            self.adb_add_modal_open = false;
                        }
                    });
                });

                ui.add_space(12.0);

                // Section 1: Detected Devices on Host
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DETECTED DEVICES ON HOST").size(10.0).monospace().strong().color(p.text_secondary));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn_rescan = egui::Button::new(RichText::new(format!("{} Refresh Devices", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_rescan).clicked() {
                            self.adb_detected_devices = crate::adb::list_devices();
                        }
                    });
                });

                ui.add_space(6.0);

                Frame::none()
                    .fill(Color32::from_rgb(12, 13, 19))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 30, 44)))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.adb_detected_devices.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(6.0);
                                ui.label(RichText::new(crate::lucide::INFO).size(16.0).color(p.text_muted));
                                ui.add_space(2.0);
                                ui.label(RichText::new("No attached ADB devices found.").size(11.0).color(p.text_secondary));
                                ui.label(RichText::new("Connect your Android device via USB with USB Debugging enabled, or connect wirelessly below.").size(9.5).color(p.text_muted));
                                ui.add_space(6.0);
                            });
                        } else {
                            let mut add_target: Option<crate::adb::AdbDeviceInfo> = None;
                            for dev in &self.adb_detected_devices {
                                ui.horizontal(|ui| {
                                    let is_online = dev.state == "device";
                                    let state_col = if is_online { p.success } else if dev.state == "unauthorized" { p.warning } else { p.danger };

                                    ui.label(RichText::new(crate::lucide::SMARTPHONE).size(15.0).color(state_col));
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&dev.model).size(12.0).strong().color(p.text_primary));
                                            ui.label(RichText::new(format!("• {}", dev.state)).size(9.5).color(state_col));
                                        });
                                        ui.label(RichText::new(format!("Serial: {}", dev.serial)).size(9.5).monospace().color(p.text_muted));
                                    });

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let already_added = self.adb_devices.iter().any(|d| d.serial == dev.serial);
                                        if already_added {
                                            ui.label(RichText::new("Added").size(10.5).color(p.text_muted));
                                        } else {
                                            let btn = egui::Button::new(RichText::new("+ Add Device").size(11.0).strong().color(Color32::WHITE))
                                                .fill(p.accent)
                                                .rounding(Rounding::same(6.0));
                                            if ui.add(btn).clicked() {
                                                add_target = Some(dev.clone());
                                            }
                                        }
                                    });
                                });
                                ui.separator();
                            }

                            if let Some(dev) = add_target {
                                let id = format!("adb_{}", dev.serial.replace(['.', ':', '-'], "_"));
                                let is_wireless = dev.serial.contains(':');
                                let cfg = crate::config::AdbDeviceConfig {
                                    id: id.clone(),
                                    name: dev.model.clone(),
                                    serial: dev.serial.clone(),
                                    is_wireless,
                                    ip_port: if is_wireless { Some(dev.serial.clone()) } else { None },
                                    scrcpy_max_fps: Some(60),
                                    scrcpy_bitrate_mbps: Some(8),
                                    scrcpy_stay_awake: true,
                                    scrcpy_turn_screen_off: false,
                                    scrcpy_no_audio: false,
                                };
                                self.adb_devices.push(cfg);
                                self.persist_config();
                                self.selected_machine = id;
                                self.adb_add_modal_open = false;
                            }
                        }
                    });

                ui.add_space(14.0);

                // Section 2: Connect Wireless ADB Target (IP:Port)
                ui.label(RichText::new("CONNECT WIRELESS ADB (TCP/IP)").size(10.0).monospace().strong().color(p.text_secondary));
                ui.add_space(6.0);

                Frame::none()
                    .fill(Color32::from_rgb(12, 13, 19))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 30, 44)))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Target IP Address:").size(10.5).color(p.text_secondary));
                                ui.add(egui::TextEdit::singleline(&mut self.adb_wireless_ip_input).hint_text("192.168.1.100").desired_width(180.0));
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Port:").size(10.5).color(p.text_secondary));
                                ui.add(egui::TextEdit::singleline(&mut self.adb_wireless_port_input).hint_text("5555").desired_width(70.0));
                            });
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Device Alias (Optional):").size(10.5).color(p.text_secondary));
                                ui.add(egui::TextEdit::singleline(&mut self.adb_device_name_input).hint_text("e.g. Pixel 7").desired_width(160.0));
                            });
                        });

                        ui.add_space(10.0);

                        let mut connect_now = false;
                        ui.horizontal(|ui| {
                            let btn_conn = egui::Button::new(RichText::new(format!("{} Connect Wireless Target", crate::lucide::WIFI)).size(11.5).strong().color(Color32::WHITE))
                                .fill(Color32::from_rgb(2, 132, 199))
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(180.0, 28.0));
                            if ui.add(btn_conn).clicked() {
                                connect_now = true;
                            }
                        });

                        if connect_now {
                            let ip = self.adb_wireless_ip_input.trim();
                            let port = self.adb_wireless_port_input.trim();
                            let target = format!("{}:{}", ip, port);
                            match crate::adb::connect_wireless(&target) {
                                Ok(msg) => {
                                    let id = format!("adb_{}", target.replace(['.', ':', '-'], "_"));
                                    let name = if self.adb_device_name_input.trim().is_empty() {
                                        format!("Wireless Android ({})", ip)
                                    } else {
                                        self.adb_device_name_input.trim().to_string()
                                    };
                                    let cfg = crate::config::AdbDeviceConfig {
                                        id: id.clone(),
                                        name,
                                        serial: target.clone(),
                                        is_wireless: true,
                                        ip_port: Some(target),
                                        scrcpy_max_fps: Some(60),
                                        scrcpy_bitrate_mbps: Some(8),
                                        scrcpy_stay_awake: true,
                                        scrcpy_turn_screen_off: false,
                                        scrcpy_no_audio: false,
                                    };
                                    if !self.adb_devices.iter().any(|d| d.serial == cfg.serial) {
                                        self.adb_devices.push(cfg);
                                        self.persist_config();
                                    }
                                    self.selected_machine = id;
                                    self.adb_status_msg = Some((format!("Connected: {}", msg), false, Instant::now()));
                                    self.adb_add_modal_open = false;
                                }
                                Err(err) => {
                                    self.adb_status_msg = Some((format!("Connect failed: {}", err), true, Instant::now()));
                                }
                            }
                        }
                    });
            });
    }

    pub fn start_builtin_mirror(&mut self, serial: &str) {
        if self.adb_mirror_active {
            return;
        }
        self.adb_mirror_active = true;
        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        self.adb_mirror_frame_rx = Some(frame_rx);
        self.adb_mirror_stop_tx = Some(stop_tx);

        let ser = serial.to_string();
        std::thread::spawn(move || {
            while stop_rx.try_recv().is_err() {
                let frame_start = Instant::now();
                match crate::adb::capture_screen_raw(&ser) {
                    Ok(bytes) => {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                            match frame_tx.try_send((rgba.into_raw(), w, h)) {
                                Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                            }
                        }
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                }
                let elapsed = frame_start.elapsed();
                let min_wait = std::time::Duration::from_millis(100);
                if elapsed < min_wait {
                    std::thread::sleep(min_wait - elapsed);
                }
            }
        });
    }

    pub fn stop_builtin_mirror(&mut self) {
        if let Some(stop_tx) = self.adb_mirror_stop_tx.take() {
            let _ = stop_tx.send(());
        }
        self.adb_mirror_active = false;
        self.adb_mirror_frame_rx = None;
        self.adb_mirror_drag_start = None;
    }

    pub fn is_scrcpy_active(&mut self, serial: &str) -> bool {
        if let Some(&pid) = self.adb_scrcpy_pids.get(serial) {
            if crate::adb::is_scrcpy_running(pid) {
                return true;
            } else {
                self.adb_scrcpy_pids.remove(serial);
            }
        }
        false
    }

    pub fn launch_scrcpy_for_device(&mut self, serial: &str) {
        let is_running = self.is_scrcpy_active(serial);
        if is_running {
            if let Some(pid) = self.adb_scrcpy_pids.remove(serial) {
                crate::adb::stop_scrcpy(pid);
                self.adb_status_msg = Some(("Stopped scrcpy mirror".to_string(), false, Instant::now()));
            }
        } else {
            match crate::adb::launch_scrcpy(
                serial,
                self.adb_scrcpy_stay_awake,
                self.adb_scrcpy_screen_off,
                self.adb_scrcpy_no_audio,
                Some(self.adb_scrcpy_max_fps),
                Some(self.adb_scrcpy_bitrate_mbps),
            ) {
                Ok(pid) => {
                    self.adb_scrcpy_pids.insert(serial.to_string(), pid);
                    self.adb_status_msg = Some((format!("scrcpy window launched (PID {})", pid), false, Instant::now()));
                }
                Err(e) => {
                    self.adb_status_msg = Some((format!("scrcpy error: {}", e), true, Instant::now()));
                }
            }
        }
    }

    pub fn connect_wireless_device(&mut self, current_serial: &str, target: &str, fallback_name: &str) -> Result<String, String> {
        let target = target.trim();
        let formatted = if !target.contains(':') {
            format!("{}:5555", target)
        } else {
            target.to_string()
        };

        // If a USB device is attached, ensure its TCP mode is enabled first
        let usb_serial = if !current_serial.contains(':') && !current_serial.is_empty() {
            Some(current_serial.to_string())
        } else {
            crate::adb::list_devices().into_iter().find(|d| !d.serial.contains(':')).map(|d| d.serial)
        };

        if let Some(usb_s) = usb_serial {
            let _ = crate::adb::enable_tcpip(&usb_s, 5555);
            std::thread::sleep(std::time::Duration::from_millis(800));
        }

        let msg = crate::adb::connect_wireless(&formatted)?;

        // Query device model name from newly connected wireless target
        let model = crate::adb::adb_cmd(Some(&formatted), &["shell", "getprop", "ro.product.model"])
            .unwrap_or_default()
            .trim()
            .to_string();

        let device_name = if !model.is_empty() {
            format!("{} (Wireless)", model)
        } else if !fallback_name.is_empty() {
            format!("{} (Wireless)", fallback_name.trim_end_matches(" (Wireless)"))
        } else {
            format!("Android ({})", formatted)
        };

        let dev_id = format!("adb_{}", formatted.replace(['.', ':', '-'], "_"));
        let cfg = crate::config::AdbDeviceConfig {
            id: dev_id.clone(),
            name: device_name.clone(),
            serial: formatted.clone(),
            is_wireless: true,
            ip_port: Some(formatted.clone()),
            scrcpy_max_fps: Some(60),
            scrcpy_bitrate_mbps: Some(8),
            scrcpy_stay_awake: true,
            scrcpy_turn_screen_off: false,
            scrcpy_no_audio: false,
        };

        if let Some(pos) = self.adb_devices.iter().position(|d| d.serial == formatted) {
            self.adb_devices[pos] = cfg;
        } else {
            self.adb_devices.push(cfg);
        }

        self.persist_config();
        self.selected_machine = dev_id;
        self.adb_active_subtab = AdbSubTab::Overview;

        Ok(format!("Connected to {} ({})", device_name, msg))
    }

    pub fn disconnect_wireless_device(&mut self, target: &str) -> Result<String, String> {
        let target = target.trim();
        let formatted = if !target.contains(':') {
            format!("{}:5555", target)
        } else {
            target.to_string()
        };

        let msg = crate::adb::disconnect_wireless(&formatted)?;

        if let Some(pos) = self.adb_devices.iter().position(|d| d.serial == formatted) {
            let removed = self.adb_devices.remove(pos);
            if self.selected_machine == removed.id {
                if let Some(usb) = self.adb_devices.iter().find(|d| !d.is_wireless) {
                    self.selected_machine = usb.id.clone();
                } else {
                    self.selected_machine = "local".to_string();
                }
            }
            self.persist_config();
        }

        Ok(msg)
    }

    // ═════════════════════════════════════════════════════════════════════════
    // ADB DEVICE COCKPIT VIEW: Full Android Management Center
    // ═════════════════════════════════════════════════════════════════════════
    pub fn render_adb_tauri_handoff(
        &mut self,
        ctx: &egui::Context,
        adb_dev: &crate::config::AdbDeviceConfig,
    ) {
        if self.adb_tauri_opened.insert(adb_dev.serial.clone()) {
            self.launch_adb_tauri_window(adb_dev);
        }

        let palette = self.palette();
        egui::CentralPanel::default()
            .frame(Frame::none().fill(palette.bg_main))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.28).max(48.0));
                    ui.label(
                        RichText::new(crate::lucide::SMARTPHONE)
                            .font(egui::FontId::new(30.0, egui::FontFamily::Proportional))
                            .color(palette.accent),
                    );
                    ui.add_space(14.0);
                    ui.heading("Android Cockpit");
                    ui.label(
                        RichText::new(format!("{} · {}", adb_dev.name, adb_dev.serial))
                            .color(palette.text_secondary),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Device management opens in the GTK-style Android Cockpit window.")
                            .color(palette.text_muted),
                    );
                    ui.add_space(18.0);
                    if ui.button("Open Android Cockpit").clicked() {
                        self.launch_adb_tauri_window(adb_dev);
                    }
                });
            });
    }

    fn launch_adb_tauri_window(
        &mut self,
        adb_dev: &crate::config::AdbDeviceConfig,
    ) {
        let [red, green, blue, _] = self.palette().accent.to_array();
        let accent = format!("#{red:02x}{green:02x}{blue:02x}");
        match std::env::current_exe() {
            Ok(exe) => {
                let mut command = std::process::Command::new(exe);
                command
                    .arg(format!("--adb-cockpit={}", adb_dev.serial))
                    .env("SERVER_LAUNCHER_ADB_ACCENT", accent);
                match command.spawn() {
                    Ok(_) => {
                        self.adb_status_msg = Some((
                            format!("Android Cockpit opened for {}", adb_dev.name),
                            false,
                            Instant::now(),
                        ));
                    }
                    Err(error) => {
                        self.adb_status_msg = Some((
                            format!("Could not open Android Cockpit: {error}"),
                            true,
                            Instant::now(),
                        ));
                    }
                }
            }
            Err(error) => {
                self.adb_status_msg = Some((
                    format!("Could not find application executable: {error}"),
                    true,
                    Instant::now(),
                ));
            }
        }
    }

    fn schedule_adb_telemetry(&mut self, serial: &str, ctx: egui::Context) {
        if !self.adb_telemetry_pending.insert(serial.to_string()) {
            return;
        }
        let serial = serial.to_string();
        let results = self.adb_telemetry_results.clone();
        std::thread::spawn(move || {
            let telemetry = crate::adb::query_telemetry(&serial);
            if let Ok(mut results) = results.lock() {
                results.insert(serial, telemetry);
            }
            ctx.request_repaint();
        });
    }

    pub fn render_adb_cockpit_view(&mut self, ctx: &egui::Context, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();

        // 1. Fetch telemetry if missing or cached
        let serial = adb_dev.serial.clone();
        if self.adb_data_serial != serial {
            self.adb_data_serial.clone_from(&serial);
            self.adb_queries_loaded.clear();
            self.adb_notifications.clear();
            self.adb_call_logs.clear();
            self.adb_sms_messages.clear();
            self.adb_photos.clear();
            self.adb_photo_preview = None;
        }
        if let Ok(mut results) = self.adb_telemetry_results.lock() {
            if let Some(telem) = results.remove(&serial) {
                self.adb_telemetry_cache.insert(serial.clone(), telem);
                self.adb_telemetry_pending.remove(&serial);
            }
        }
        if !self.adb_telemetry_cache.contains_key(&serial) {
            self.schedule_adb_telemetry(&serial, ctx.clone());
        }
        let telem = self.adb_telemetry_cache.get(&serial).cloned().unwrap_or_default();

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(p.bg_main)
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin {
                        left: 12.0,
                        right: 18.0,
                        top: 4.0,
                        bottom: 8.0,
                    }),
            )
            .show(ctx, |ui| {
                // Header Bar (Device metadata capsule & top quick actions)
                self.render_adb_header_bar(ui, adb_dev, &telem);

                ui.add_space(8.0);

                // Navigation Segmented Bar
                self.render_adb_subtabs_bar(ui);

                ui.add_space(8.0);

                // Subtab Content
                match self.adb_active_subtab {
                    AdbSubTab::Overview => self.render_adb_overview_tab(ui, adb_dev, &telem),
                    AdbSubTab::ScreenShare => self.render_adb_screenshare_tab(ui, adb_dev),
                    AdbSubTab::Messages => self.render_adb_messages_tab(ui, adb_dev),
                    AdbSubTab::Calls => self.render_adb_calls_tab(ui, adb_dev),
                    AdbSubTab::Photos => self.render_adb_photos_tab(ui, adb_dev),
                    AdbSubTab::Notifications => self.render_adb_notifications_tab(ui, adb_dev),
                    AdbSubTab::Storage => self.render_adb_storage_tab(ui, adb_dev),
                    AdbSubTab::Apps => self.render_adb_apps_tab(ui, adb_dev),
                    AdbSubTab::Clipboard => self.render_adb_clipboard_tab(ui, adb_dev),
                    AdbSubTab::Terminal => self.render_adb_terminal_tab(ui, adb_dev),
                    AdbSubTab::Wireless => self.render_adb_wireless_tab(ui, adb_dev),
                    AdbSubTab::Firmware => self.render_adb_firmware_tab(ui, adb_dev),
                }
            });

        // Photo Lightbox Dialog Modal
        self.render_adb_photo_lightbox(ctx, &adb_dev.serial);
    }

    fn render_adb_header_bar(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig, telem: &crate::adb::AdbDeviceTelemetry) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Left: Smartphone icon pill
                    Frame::none()
                        .fill(Color32::from_rgb(14, 40, 58))
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(crate::lucide::SMARTPHONE).size(16.0).color(Color32::from_rgb(56, 189, 248)));
                        });

                    ui.add_space(6.0);

                    // Title & Specs
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&adb_dev.name).size(15.0).strong().color(p.text_primary));

                            // Connection Type Chip
                            let conn_label = if adb_dev.is_wireless { "Wireless TCP/IP" } else { "USB Debug Link" };
                            Frame::none()
                                .fill(Color32::from_rgb(12, 34, 52))
                                .rounding(Rounding::same(4.0))
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(conn_label).size(9.0).monospace().color(Color32::from_rgb(56, 189, 248)));
                                });

                            if !telem.android_version.is_empty() {
                                Frame::none()
                                    .fill(p.accent_subtle)
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("Android {} (API {})", telem.android_version, telem.api_level)).size(9.0).monospace().color(p.accent));
                                    });
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Serial: {}", adb_dev.serial)).size(10.0).monospace().color(p.text_muted));
                            if let Some(pct) = telem.battery_pct {
                                ui.label(RichText::new("•").size(9.5).color(p.border));
                                ui.label(RichText::new(format!("Battery: {:.0}%", pct)).size(10.0).monospace().color(p.text_secondary));
                            }
                            if !telem.screen_res.is_empty() {
                                ui.label(RichText::new("•").size(9.5).color(p.border));
                                ui.label(RichText::new(format!("Display: {}", telem.screen_res)).size(10.0).monospace().color(p.text_secondary));
                            }
                        });
                    });

                    // Right Quick Action Buttons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Disconnect button
                        let btn_disc = egui::Button::new(RichText::new(format!("{} Disconnect", crate::lucide::LOG_OUT)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_disc).on_hover_text("Switch to local host and disconnect").clicked() {
                            self.selected_machine = "local".to_string();
                        }

                        // Reboot dropdown
                        ui.menu_button(RichText::new(format!("{} Reboot ▾", crate::lucide::POWER)).size(10.5).color(p.text_secondary), |ui| {
                            if ui.button("Reboot System").clicked() {
                                let _ = crate::adb::reboot_device(&serial, "system");
                                ui.close_menu();
                            }
                            if ui.button("Reboot to Recovery (TWRP / Lineage)").clicked() {
                                let _ = crate::adb::reboot_device(&serial, "recovery");
                                ui.close_menu();
                            }
                            if ui.button("Reboot to Bootloader / Fastboot").clicked() {
                                let _ = crate::adb::reboot_device(&serial, "bootloader");
                                ui.close_menu();
                            }
                            if ui.button("Reboot to FastbootD").clicked() {
                                let _ = crate::adb::reboot_device(&serial, "fastboot");
                                ui.close_menu();
                            }
                            if ui.button("Reboot to EDL (Emergency Download)").clicked() {
                                let _ = crate::adb::reboot_device(&serial, "edl");
                                ui.close_menu();
                            }
                        });

                        // Refresh Telemetry
                        let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_ref).on_hover_text("Refresh device telemetry and metrics").clicked() {
                            self.adb_telemetry_cache.remove(&serial);
                            if let Ok(mut results) = self.adb_telemetry_results.lock() {
                                results.remove(&serial);
                            }
                            self.adb_telemetry_pending.remove(&serial);
                            self.schedule_adb_telemetry(&serial, ui.ctx().clone());
                        }

                        // External Shell
                        let btn_term = egui::Button::new(RichText::new(format!("{} Shell", crate::lucide::TERMINAL)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_term).on_hover_text("Launch interactive ADB terminal in system emulator").clicked() {
                            match crate::adb::launch_external_adb_terminal(&serial) {
                                Ok(_) => self.adb_status_msg = Some(("External ADB terminal opened".to_string(), false, Instant::now())),
                                Err(e) => self.adb_status_msg = Some((format!("Terminal error: {}", e), true, Instant::now())),
                            }
                        }

                        // Built-in Screen Mirroring Quick Toggle
                        let is_mirror_on = self.adb_mirror_active;
                        let mirror_btn_text = if is_mirror_on {
                            RichText::new(format!("{} Pause Mirror", crate::lucide::PAUSE)).size(10.5).strong().color(Color32::WHITE)
                        } else {
                            RichText::new(format!("{} Screen Share", crate::lucide::SCREEN_SHARE)).size(10.5).strong().color(Color32::WHITE)
                        };
                        let mirror_btn_bg = if is_mirror_on { Color32::from_rgb(220, 38, 38) } else { Color32::from_rgb(2, 132, 199) };

                        let btn_mirror = egui::Button::new(mirror_btn_text)
                            .fill(mirror_btn_bg)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_mirror).on_hover_text("Open and toggle real-time built-in screen mirroring").clicked() {
                            self.adb_active_subtab = AdbSubTab::ScreenShare;
                            self.adb_mirror_mode = AdbScreenMirrorMode::BuiltIn;
                            if is_mirror_on {
                                self.stop_builtin_mirror();
                            } else {
                                self.start_builtin_mirror(&serial);
                            }
                        }

                        // scrcpy External HD Mirror Quick Toggle
                        let is_scrcpy_on = self.is_scrcpy_active(&serial);
                        let scrcpy_btn_text = if is_scrcpy_on {
                            RichText::new(format!("{} Stop scrcpy", crate::lucide::SQUARE)).size(10.5).strong().color(Color32::WHITE)
                        } else {
                            RichText::new("⚡ scrcpy HD").size(10.5).strong().color(Color32::WHITE)
                        };
                        let scrcpy_btn_bg = if is_scrcpy_on { Color32::from_rgb(220, 38, 38) } else { Color32::from_rgb(14, 116, 144) };
                        let btn_scrcpy = egui::Button::new(scrcpy_btn_text)
                            .fill(scrcpy_btn_bg)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_scrcpy).on_hover_text("Launch high-performance external scrcpy display window").clicked() {
                            self.launch_scrcpy_for_device(&serial);
                        }
                    });
                });
            });
    }

    fn render_adb_subtabs_bar(&mut self, ui: &mut Ui) {
        let p = self.palette();
        Frame::none()
            .fill(p.bg_card)
            .rounding(Rounding::same(12.0))
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .show(ui, |ui| {
                ScrollArea::horizontal()
                    .id_salt("adb_subtabs_scroll")
                    .auto_shrink([false, true])
                    .max_height(34.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                            for tab in AdbSubTab::all() {
                                let is_active = self.adb_active_subtab == *tab;
                                let bg = if is_active { p.accent } else { Color32::TRANSPARENT };
                                let fg = if is_active { p.accent_text } else { p.text_secondary };

                                let badge_str = match tab {
                                    AdbSubTab::Messages => {
                                        let count = self.adb_sms_messages.iter().filter(|m| !m.is_outgoing).count();
                                        if count > 0 { format!(" ({})", count) } else { String::new() }
                                    }
                                    AdbSubTab::Calls => {
                                        let count = self.adb_call_logs.iter().filter(|c| c.call_type == crate::adb::AdbCallType::Missed).count();
                                        if count > 0 { format!(" ({})", count) } else { String::new() }
                                    }
                                    AdbSubTab::Notifications => {
                                        if !self.adb_notifications.is_empty() {
                                            format!(" ({})", self.adb_notifications.len())
                                        } else {
                                            String::new()
                                        }
                                    }
                                    AdbSubTab::Photos => {
                                        if !self.adb_photos.is_empty() {
                                            format!(" ({})", self.adb_photos.len())
                                        } else {
                                            String::new()
                                        }
                                    }
                                    _ => String::new(),
                                };

                                let btn = egui::Button::new(
                                    RichText::new(format!("{} {}{}", tab.icon(), tab.label(), badge_str))
                                        .size(11.0)
                                        .strong()
                                        .color(fg),
                                )
                                .fill(bg)
                                .rounding(Rounding::same(999.0))
                                .min_size(Vec2::new(0.0, 28.0));

                                if ui.add(btn).clicked() {
                                    self.adb_active_subtab = *tab;
                                }
                            }
                        });
                    });
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 1: OVERVIEW & HARDWARE TELEMETRY
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_overview_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig, telem: &crate::adb::AdbDeviceTelemetry) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ScrollArea::vertical()
            .id_salt("adb_overview_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // ═════════════════════════════════════════════════════════════
                // PHONE LINK MASTER COCKPIT DECK
                // ═════════════════════════════════════════════════════════════
                let deck_avail_w = ui.available_width();
                let left_w = 340.0_f32.min(deck_avail_w * 0.45);
                let right_w = (deck_avail_w - left_w - 12.0).max(0.0);

                ui.horizontal_top(|ui| {
                    // LEFT COLUMN: Phone Link Hardware Card & Live Notifications Preview
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(14.0))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width((left_w - 28.0).max(120.0));
                            ui.vertical(|ui| {
                                // Phone Hardware Preview Box
                            Frame::none()
                                .fill(Color32::from_rgb(14, 16, 24))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 60)))
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Phone bezel miniature
                                        Frame::none()
                                            .fill(Color32::from_rgb(26, 32, 48))
                                            .stroke(Stroke::new(1.5_f32, Color32::from_rgb(56, 189, 248)))
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(10.0, 14.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(crate::lucide::SMARTPHONE).size(22.0).color(Color32::from_rgb(56, 189, 248)));
                                            });

                                        ui.add_space(10.0);
                                        ui.vertical(|ui| {
                                            let model_name = if telem.model.is_empty() { &adb_dev.name } else { &telem.model };
                                            ui.label(RichText::new(model_name).size(14.0).strong().color(p.text_primary));
                                            ui.add_space(2.0);

                                            // Connection status & Transport
                                            ui.horizontal(|ui| {
                                                let is_wireless = serial.contains(':');
                                                let conn_icon = if is_wireless { crate::lucide::WIFI } else { crate::lucide::USB };
                                                let conn_label = if is_wireless { "Wireless TCP/IP" } else { "USB Debugging" };
                                                ui.label(RichText::new(conn_icon).size(10.5).color(Color32::from_rgb(56, 189, 248)));
                                                ui.label(RichText::new(conn_label).size(9.5).monospace().color(p.text_secondary));
                                                ui.label(RichText::new("•").size(9.5).color(p.text_muted));
                                                ui.label(RichText::new("Saved ADB target").size(9.5).color(p.text_secondary));
                                            });

                                            ui.add_space(2.0);
                                            // Battery & Root Status Row
                                            ui.horizontal(|ui| {
                                                if let Some(pct) = telem.battery_pct {
                                                    let charging = telem.battery_status.as_deref().unwrap_or("").contains("Charging");
                                                    let b_icon = if charging { "⚡" } else { "🔋" };
                                                    let b_col = if pct > 20.0 { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(239, 68, 68) };
                                                    ui.label(RichText::new(format!("{} {:.0}%", b_icon, pct)).size(9.5).strong().color(b_col));
                                                    if let Some(temp) = telem.battery_temp_c {
                                                        ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                                        ui.label(RichText::new(format!("{:.1}°C", temp)).size(9.5).monospace().color(p.text_muted));
                                                    }
                                                    ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                                }

                                                if telem.is_rooted {
                                                    ui.label(RichText::new("Rooted (su)").size(9.5).strong().color(Color32::from_rgb(168, 85, 247)));
                                                } else {
                                                    ui.label(RichText::new("Unrooted").size(9.5).color(p.text_muted));
                                                }
                                            });
                                        });
                                    });

                                    ui.add_space(10.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    // Quick Toggles Row (Phone Link Style)
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);

                                        // DND Toggle
                                        let dnd_bg = if self.adb_dnd_enabled { Color32::from_rgb(79, 70, 229) } else { p.bg_button };
                                        let dnd_fg = if self.adb_dnd_enabled { Color32::WHITE } else { p.text_secondary };
                                        let btn_dnd = egui::Button::new(RichText::new(format!("{} DND", crate::lucide::BELL_OFF)).size(10.0).color(dnd_fg))
                                            .fill(dnd_bg)
                                            .rounding(Rounding::same(999.0));
                                        if ui.add(btn_dnd).on_hover_text("Toggle Do Not Disturb").clicked() {
                                            let enabled = !self.adb_dnd_enabled;
                                            match crate::adb::toggle_dnd(Some(&serial), enabled) {
                                                Ok(_) => self.adb_dnd_enabled = enabled,
                                                Err(e) => self.adb_status_msg = Some((format!("DND failed: {e}"), true, Instant::now())),
                                            }
                                        }

                                        // Audio Mute Toggle
                                        let mute_bg = if self.adb_audio_muted { Color32::from_rgb(220, 38, 38) } else { p.bg_button };
                                        let mute_fg = if self.adb_audio_muted { Color32::WHITE } else { p.text_secondary };
                                        let btn_mute = egui::Button::new(RichText::new(format!("{} Audio", if self.adb_audio_muted { crate::lucide::VOLUME_X } else { crate::lucide::VOLUME_2 })).size(10.0).color(mute_fg))
                                            .fill(mute_bg)
                                            .rounding(Rounding::same(999.0));
                                        if ui.add(btn_mute).on_hover_text("Toggle Audio Mute").clicked() {
                                            match crate::adb::toggle_mute(Some(&serial)) {
                                                Ok(_) => self.adb_audio_muted = !self.adb_audio_muted,
                                                Err(e) => self.adb_status_msg = Some((format!("Audio control failed: {e}"), true, Instant::now())),
                                            }
                                        }

                                        // Media Play/Pause
                                        let media_icon = if self.adb_media_playing { crate::lucide::PAUSE } else { crate::lucide::PLAY };
                                        let btn_media = egui::Button::new(RichText::new(format!("{} Media", media_icon)).size(10.0).color(p.text_secondary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(999.0));
                                        if ui.add(btn_media).on_hover_text("Play/Pause Media playback").clicked() {
                                            match crate::adb::media_play_pause(Some(&serial)) {
                                                Ok(_) => self.adb_media_playing = !self.adb_media_playing,
                                                Err(e) => self.adb_status_msg = Some((format!("Media control failed: {e}"), true, Instant::now())),
                                            }
                                        }

                                        // Screen Lock
                                        let btn_lock = egui::Button::new(RichText::new(format!("{} Lock", crate::lucide::LOCK)).size(10.0).color(p.text_secondary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(999.0));
                                        if ui.add(btn_lock).on_hover_text("Lock/Wake screen (Power)").clicked() {
                                            let _ = crate::adb::send_keyevent(&serial, 26);
                                        }
                                    });
                                });

                            ui.add_space(10.0);

                            // Live Notifications Preview Card
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::BELL).size(13.0).color(Color32::from_rgb(251, 191, 36)));
                                ui.label(RichText::new("Notifications").size(12.0).strong().color(p.text_primary));

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn_view_all = egui::Button::new(RichText::new("View all →").size(9.5).color(Color32::from_rgb(56, 189, 248)))
                                        .fill(Color32::TRANSPARENT);
                                    if ui.add(btn_view_all).clicked() {
                                        self.adb_active_subtab = AdbSubTab::Notifications;
                                    }
                                });
                            });

                            ui.add_space(6.0);

                            if self.adb_queries_loaded.insert("notifications".to_string()) {
                                self.adb_notifications = crate::adb::get_notifications(Some(&serial));
                            }

                            // Show top 2 preview notifications
                            for notif in self.adb_notifications.iter().take(2) {
                                Frame::none()
                                    .fill(Color32::from_rgb(16, 18, 26))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 32, 46)))
                                    .rounding(Rounding::same(8.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&notif.app_name).size(9.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.label(RichText::new(&notif.time_str).size(8.5).color(p.text_muted));
                                            });
                                        });
                                        ui.label(RichText::new(&notif.title).size(11.0).strong().color(p.text_primary));
                                        ui.add(egui::Label::new(RichText::new(&notif.text).size(9.5).color(p.text_secondary)).truncate());
                                    });
                                ui.add_space(4.0);
                            }
                            });
                        });

                    ui.add_space(12.0);

                    // RIGHT COLUMN: Quick Communications & Apps Launchpad
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(14.0))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_width((right_w - 32.0).max(120.0));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::STORAGE).size(14.0).color(p.accent));
                                    ui.label(RichText::new("DEVICE STORAGE & MEMORY").size(11.0).monospace().strong().color(p.text_primary));
                                });

                                ui.add_space(10.0);

                                // /data Partition
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Internal Storage (/data)").size(11.0).strong().color(p.text_primary));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let (Some(total), Some(free)) = (telem.storage_internal_total_gb, telem.storage_internal_free_gb) {
                                            let used = (total - free).max(0.0);
                                            let pct = if total > 0.0 { (used / total) * 100.0 } else { 0.0 };
                                            ui.label(RichText::new(format!("{:.1} GB / {:.1} GB ({:.0}%)", used, total, pct)).size(10.0).monospace().color(p.text_secondary));
                                        } else {
                                            ui.label(RichText::new("Scanning...").size(10.0).color(p.text_muted));
                                        }
                                    });
                                });

                                ui.add_space(4.0);
                                if let (Some(total), Some(free)) = (telem.storage_internal_total_gb, telem.storage_internal_free_gb) {
                                    let used = (total - free).max(0.0);
                                    let pct = if total > 0.0 { used / total } else { 0.0 };
                                    ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB Free", free)).animate(false));
                                } else {
                                    ui.add(egui::ProgressBar::new(0.0).text("Measuring storage..."));
                                }

                                ui.add_space(12.0);

                                // /sdcard Partition
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Shared Storage (/sdcard)").size(11.0).strong().color(p.text_primary));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let (Some(total), Some(free)) = (telem.storage_sdcard_total_gb, telem.storage_sdcard_free_gb) {
                                            let used = (total - free).max(0.0);
                                            let pct = if total > 0.0 { (used / total) * 100.0 } else { 0.0 };
                                            ui.label(RichText::new(format!("{:.1} GB / {:.1} GB ({:.0}%)", used, total, pct)).size(10.0).monospace().color(p.text_secondary));
                                        } else {
                                            ui.label(RichText::new("Scanning...").size(10.0).color(p.text_muted));
                                        }
                                    });
                                });

                                ui.add_space(4.0);
                                if let (Some(total), Some(free)) = (telem.storage_sdcard_total_gb, telem.storage_sdcard_free_gb) {
                                    let used = (total - free).max(0.0);
                                    let pct = if total > 0.0 { used / total } else { 0.0 };
                                    ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB Free", free)).animate(false));
                                } else {
                                    ui.add(egui::ProgressBar::new(0.0).text("Measuring storage..."));
                                }

                                ui.add_space(14.0);
                                ui.separator();
                                ui.add_space(10.0);

                                // Quick Tools Deck
                                ui.label(RichText::new("DEVICE CONTROL SHORTCUTS").size(9.5).monospace().strong().color(p.text_muted));
                                ui.add_space(6.0);

                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                                    let btn_term = egui::Button::new(RichText::new(format!("{} Terminal", crate::lucide::TERMINAL)).size(10.5).color(p.text_primary))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_term).clicked() {
                                        self.adb_active_subtab = AdbSubTab::Terminal;
                                    }

                                    let btn_mirror = egui::Button::new(RichText::new(format!("{} Mirror", crate::lucide::SCREEN_SHARE)).size(10.5).color(Color32::from_rgb(168, 85, 247)))
                                        .fill(Color32::from_rgb(35, 20, 45))
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_mirror).clicked() {
                                        self.adb_active_subtab = AdbSubTab::ScreenShare;
                                    }

                                    let btn_apps = egui::Button::new(RichText::new(format!("{} Apps", crate::lucide::BOX)).size(10.5).color(p.text_primary))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_apps).clicked() {
                                        self.adb_active_subtab = AdbSubTab::Apps;
                                    }

                                    let btn_files = egui::Button::new(RichText::new(format!("{} Files", crate::lucide::FOLDER)).size(10.5).color(p.text_primary))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_files).clicked() {
                                        self.adb_active_subtab = AdbSubTab::Storage;
                                    }

                                    let btn_reboot = egui::Button::new(RichText::new(format!("{} Reboot", crate::lucide::REFRESH_CW)).size(10.5).color(Color32::from_rgb(248, 113, 113)))
                                        .fill(Color32::from_rgb(45, 20, 20))
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_reboot).on_hover_text("Reboot Android Device").clicked() {
                                        let _ = crate::adb::reboot_device(&serial, "system");
                                    }
                                });
                            });
                        });
                });

                ui.add_space(12.0);

                // Two columns keep hardware cards readable in narrow windows.
                ui.columns(2, |cols| {
                    // Col 1: Platform & OS
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[0], |ui| {
                        ui.label(RichText::new("ANDROID PLATFORM").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(if telem.android_version.is_empty() { "Android version unavailable".to_string() } else { format!("Android {}", telem.android_version) }).size(16.0).strong().color(p.text_primary));
                        ui.label(RichText::new(if telem.api_level == 0 { "API Level: unavailable".to_string() } else { format!("API Level: {}", telem.api_level) }).size(10.5).color(p.text_secondary));
                        ui.label(RichText::new(format!("Patch: {}", if telem.security_patch.is_empty() { "Unavailable" } else { &telem.security_patch })).size(9.5).monospace().color(p.text_muted));
                    });

                    // Col 2: Hardware & SoC
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[1], |ui| {
                        ui.label(RichText::new("DEVICE HARDWARE").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        let model_name = if telem.model.is_empty() { &adb_dev.name } else { &telem.model };
                        ui.label(RichText::new(model_name).size(15.0).strong().color(p.text_primary));
                        ui.label(RichText::new(format!("Brand: {}", if telem.brand.is_empty() { "Unavailable" } else { &telem.brand })).size(10.5).color(p.text_secondary));
                        ui.label(RichText::new(format!("ABI: {}", if telem.cpu_abi.is_empty() { "Unavailable" } else { &telem.cpu_abi })).size(9.5).monospace().color(p.text_muted));
                    });

                    // Col 3: Battery & Power
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[0], |ui| {
                        ui.label(RichText::new("BATTERY & THERMAL").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        if let Some(pct) = telem.battery_pct {
                            ui.label(RichText::new(format!("{:.0}%", pct)).size(16.0).strong().color(p.accent));
                        } else {
                            ui.label(RichText::new("--%").size(16.0).strong().color(p.text_muted));
                        }
                        ui.label(RichText::new(telem.battery_status.as_deref().unwrap_or("Battery status unavailable")).size(10.5).color(p.text_secondary));
                        if let Some(t) = telem.battery_temp_c {
                            ui.label(RichText::new(format!("Temp: {:.1} °C", t)).size(9.5).monospace().color(p.text_muted));
                        } else {
                            ui.label(RichText::new("Temp: --").size(9.5).monospace().color(p.text_muted));
                        }
                    });

                    // Col 4: Display & Security
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[1], |ui| {
                        ui.label(RichText::new("DISPLAY & SECURITY").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        let res_str = if telem.screen_res.is_empty() { "Unknown" } else { &telem.screen_res };
                        ui.label(RichText::new(res_str).size(15.0).strong().color(p.text_primary));
                        let dpi_str = if telem.screen_dpi.is_empty() { "-- DPI" } else { &telem.screen_dpi };
                        ui.label(RichText::new(dpi_str).size(10.5).color(p.text_secondary));
                        let root_str = if telem.is_rooted { "Rooted (su)" } else { "Unrooted" };
                        let selinux_str = if telem.selinux.is_empty() { "--" } else { &telem.selinux };
                        ui.label(RichText::new(format!("SELinux: {} • {}", selinux_str, root_str)).size(9.5).monospace().color(p.text_muted));
                    });
                });

                ui.add_space(10.0);

                // Row 2: Storage Partitions
                ui.columns(2, |cols| {
                    // /data Partition
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(&mut cols[0], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::STORAGE).size(13.0).color(p.accent));
                            ui.label(RichText::new("INTERNAL STORAGE (/data)").size(10.0).monospace().strong().color(p.text_secondary));
                        });
                        ui.add_space(6.0);
                        if let (Some(total), Some(free)) = (telem.storage_internal_total_gb, telem.storage_internal_free_gb) {
                            let used = (total - free).max(0.0);
                            let pct = if total > 0.0 { (used / total).clamp(0.0, 1.0) } else { 0.0 };
                            ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB used of {:.1} GB ({:.0}%)", used, total, pct * 100.0)).animate(false));
                        } else {
                            ui.label(RichText::new("Storage metrics unavailable").size(11.0).color(p.text_muted));
                        }
                    });

                    // /sdcard Partition
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(&mut cols[1], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::FOLDER).size(13.0).color(Color32::from_rgb(56, 189, 248)));
                            ui.label(RichText::new("SHARED STORAGE (/sdcard)").size(10.0).monospace().strong().color(p.text_secondary));
                        });
                        ui.add_space(6.0);
                        if let (Some(total), Some(free)) = (telem.storage_sdcard_total_gb, telem.storage_sdcard_free_gb) {
                            let used = (total - free).max(0.0);
                            let pct = if total > 0.0 { (used / total).clamp(0.0, 1.0) } else { 0.0 };
                            ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB used of {:.1} GB ({:.0}%)", used, total, pct * 100.0)).animate(false));
                        } else {
                            ui.label(RichText::new("Shared storage unavailable or not mounted").size(11.0).color(p.text_muted));
                        }
                    });
                });

                ui.add_space(10.0);

                // Row 3: Hardware Simulation & Remote Touchpad
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::GAUGE).size(14.0).color(p.accent));
                            ui.label(RichText::new("HARDWARE CONTROLS & REMOTE INPUT").size(11.0).monospace().strong().color(p.text_primary));
                        });

                        ui.add_space(10.0);

                        // Buttons grid
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                            // Power Button (26)
                            let btn_pwr = egui::Button::new(RichText::new(format!("{} Power", crate::lucide::POWER)).size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(90.0, 30.0));
                            if ui.add(btn_pwr).on_hover_text("Toggle device power / lock screen (Keycode 26)").clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 26);
                            }

                            // Volume Up (24)
                            let btn_vup = egui::Button::new(RichText::new("🔊 Volume +").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(90.0, 30.0));
                            if ui.add(btn_vup).clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 24);
                            }

                            // Volume Down (25)
                            let btn_vdn = egui::Button::new(RichText::new("🔉 Volume -").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(90.0, 30.0));
                            if ui.add(btn_vdn).clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 25);
                            }

                            // Back (4)
                            let btn_back = egui::Button::new(RichText::new("◀ Back").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(80.0, 30.0));
                            if ui.add(btn_back).clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 4);
                            }

                            // Home (3)
                            let btn_home = egui::Button::new(RichText::new("⌂ Home").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(80.0, 30.0));
                            if ui.add(btn_home).clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 3);
                            }

                            // Recents (187)
                            let btn_rec = egui::Button::new(RichText::new("▢ Recents").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(80.0, 30.0));
                            if ui.add(btn_rec).clicked() {
                                let _ = crate::adb::send_keyevent(&serial, 187);
                            }

                            // Screenshot
                            let btn_shot = egui::Button::new(RichText::new("📸 Screenshot").size(11.0).strong().color(Color32::WHITE))
                                .fill(p.accent)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(100.0, 30.0));
                            if ui.add(btn_shot).on_hover_text("Capture device screen and save to Pictures/Screenshots").clicked() {
                                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                                let shot_dir = PathBuf::from(home).join("Pictures/Screenshots");
                                let _ = std::fs::create_dir_all(&shot_dir);
                                let now_str = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                                let dest = shot_dir.join(format!("android_screenshot_{}.png", now_str));
                                match crate::adb::take_screenshot(&serial, &dest) {
                                    Ok(_) => {
                                        self.adb_status_msg = Some((format!("Saved screenshot to {}", dest.display()), false, Instant::now()));
                                    }
                                    Err(err) => {
                                        self.adb_status_msg = Some((format!("Screenshot error: {}", err), true, Instant::now()));
                                    }
                                }
                            }
                        });

                        ui.add_space(10.0);

                        // Text Injector Row
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Inject Text:").size(11.0).color(p.text_secondary));
                            ui.add(egui::TextEdit::singleline(&mut self.adb_input_text_field).hint_text("Type message or URL to inject into Android device...").desired_width(320.0));
                            let btn_send = egui::Button::new(RichText::new("Send to Device").size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_send).clicked() {
                                let txt = self.adb_input_text_field.trim();
                                if !txt.is_empty() {
                                    let _ = crate::adb::send_text(&serial, txt);
                                    self.adb_input_text_field.clear();
                                }
                            }
                        });
                    });

                // Status Message Banner
                if let Some((ref msg, is_err, time)) = self.adb_status_msg {
                    if time.elapsed().as_secs() < 6 {
                        ui.add_space(8.0);
                        let bg_col = if is_err { Color32::from_rgb(220, 38, 38).gamma_multiply(0.2) } else { p.accent.gamma_multiply(0.2) };
                        let fg_col = if is_err { Color32::from_rgb(248, 113, 113) } else { p.accent };
                        Frame::none()
                            .fill(bg_col)
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).size(10.5).color(fg_col));
                            });
                    }
                }
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 2: SCREEN SHARE (BUILT-IN EMBEDDED & SCRCPY DUAL MODES)
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_screenshare_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        // 1. Dual Mode Switcher Bar
        Frame::none()
            .fill(p.bg_card)
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::symmetric(14.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let is_builtin = self.adb_mirror_mode == AdbScreenMirrorMode::BuiltIn;
                    let is_scrcpy = self.adb_mirror_mode == AdbScreenMirrorMode::Scrcpy;
                    let is_scrcpy_on = self.is_scrcpy_active(&serial);

                    let btn_builtin = egui::Button::new(
                        RichText::new("📱 Built-in Embedded View")
                            .size(11.0)
                            .strong()
                            .color(if is_builtin { Color32::WHITE } else { p.text_secondary }),
                    )
                    .fill(if is_builtin { p.accent } else { Color32::TRANSPARENT })
                    .rounding(Rounding::same(6.0));
                    if ui.add(btn_builtin).clicked() {
                        self.adb_mirror_mode = AdbScreenMirrorMode::BuiltIn;
                    }

                    let scrcpy_tag = if is_scrcpy_on { "⚡ scrcpy HD Mirror (Active)" } else { "⚡ scrcpy HD Mirror" };
                    let scrcpy_col = if is_scrcpy {
                        Color32::WHITE
                    } else if is_scrcpy_on {
                        Color32::from_rgb(34, 197, 94)
                    } else {
                        p.text_secondary
                    };
                    let btn_scrcpy = egui::Button::new(
                        RichText::new(scrcpy_tag)
                            .size(11.0)
                            .strong()
                            .color(scrcpy_col),
                    )
                    .fill(if is_scrcpy { Color32::from_rgb(2, 132, 199) } else { Color32::TRANSPARENT })
                    .rounding(Rounding::same(6.0));
                    if ui.add(btn_scrcpy).clicked() {
                        self.adb_mirror_mode = AdbScreenMirrorMode::Scrcpy;
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if is_scrcpy_on {
                            let btn_stop = egui::Button::new(RichText::new("■ Stop scrcpy").size(10.5).color(Color32::from_rgb(248, 113, 113)))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_stop).on_hover_text("Terminate active external scrcpy process").clicked() {
                                self.launch_scrcpy_for_device(&serial);
                            }
                        } else {
                            let btn_launch = egui::Button::new(RichText::new("⚡ Quick Launch scrcpy").size(10.5).color(Color32::from_rgb(56, 189, 248)))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_launch).on_hover_text("Launch high-performance external scrcpy window").clicked() {
                                self.launch_scrcpy_for_device(&serial);
                            }
                        }
                    });
                });
            });

        ui.add_space(6.0);

        match self.adb_mirror_mode {
            AdbScreenMirrorMode::BuiltIn => {
                self.render_adb_builtin_mirror(ui, &serial);
            }
            AdbScreenMirrorMode::Scrcpy => {
                self.render_adb_scrcpy_panel(ui, &serial);
            }
        }
    }

    fn render_adb_builtin_mirror(&mut self, ui: &mut Ui, serial: &str) {
        let p = self.palette();

        // 1. Repaint trigger for real-time video stream
        if self.adb_mirror_active {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }

        // 2. Poll incoming frames from background screencap worker
        if let Some(ref rx) = self.adb_mirror_frame_rx {
            let mut latest_frame = None;
            while let Ok(frame) = rx.try_recv() {
                latest_frame = Some(frame);
            }
            if let Some((raw_bytes, w, h)) = latest_frame {
                self.adb_mirror_device_res = (w, h);
                let color_img = egui::ColorImage::from_rgba_unmultiplied([w, h], &raw_bytes);
                if let Some(ref mut tex) = self.adb_mirror_texture {
                    tex.set(color_img, egui::TextureOptions::LINEAR);
                } else {
                    self.adb_mirror_texture = Some(ui.ctx().load_texture("adb_mirror_screen", color_img, egui::TextureOptions::LINEAR));
                }
                let now = Instant::now();
                if let Some(last) = self.adb_mirror_last_frame_time {
                    let delta = (now - last).as_secs_f32();
                    if delta > 0.001 {
                        let cur_fps = 1.0 / delta;
                        self.adb_mirror_fps = self.adb_mirror_fps * 0.7 + cur_fps * 0.3;
                    }
                }
                self.adb_mirror_last_frame_time = Some(now);
            }
        }

        // 3. Auto-start mirror on first tab entry if not active
        if !self.adb_mirror_active && self.adb_mirror_texture.is_none() {
            self.start_builtin_mirror(serial);
        }

        ui.vertical(|ui| {
            // Control Toolbar Bar
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Status indicator
                        if self.adb_mirror_active {
                            ui.label(RichText::new("● LIVE").size(11.0).strong().color(Color32::from_rgb(34, 197, 94)));
                            ui.label(RichText::new(format!("{:.1} FPS", self.adb_mirror_fps)).size(10.0).monospace().color(p.text_secondary));
                            ui.label(RichText::new(format!("{}×{}", self.adb_mirror_device_res.0, self.adb_mirror_device_res.1)).size(10.0).monospace().color(p.text_muted));
                        } else {
                            ui.label(RichText::new("○ PAUSED").size(11.0).color(p.text_muted));
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Quick text injection
                        ui.label(RichText::new(crate::lucide::KEYBOARD).size(12.0).color(p.text_muted));
                        let text_edit = ui.add(
                            egui::TextEdit::singleline(&mut self.adb_screenshare_text_input)
                                .hint_text("Type on device (press Enter)...")
                                .desired_width(170.0),
                        );
                        let enter_hit = text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let btn_send = egui::Button::new(RichText::new("Send").size(10.5).color(p.text_primary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(4.0));
                        if (enter_hit || ui.add(btn_send).clicked()) && !self.adb_screenshare_text_input.trim().is_empty() {
                            let text = self.adb_screenshare_text_input.clone();
                            self.adb_screenshare_text_input.clear();
                            let ser = serial.to_string();
                            std::thread::spawn(move || {
                                let _ = crate::adb::send_text(&ser, &text);
                            });
                        }

                        // Right-aligned toolbar actions
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // Pop out to scrcpy
                            let btn_pop = egui::Button::new(RichText::new("⚡ Pop Out scrcpy").size(10.5).color(Color32::from_rgb(56, 189, 248)))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_pop).on_hover_text("Open external hardware-accelerated scrcpy window").clicked() {
                                self.launch_scrcpy_for_device(serial);
                            }

                            // Wake / Screen Lock toggle
                            let btn_wake = egui::Button::new(RichText::new(format!("{} Power / Wake", crate::lucide::POWER)).size(10.5).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_wake).on_hover_text("Send Power keyevent to wake or sleep device").clicked() {
                                let ser = serial.to_string();
                                std::thread::spawn(move || {
                                    let _ = crate::adb::wake_screen(Some(&ser));
                                });
                            }

                            // Screenshot button
                            let btn_shot = egui::Button::new(RichText::new(format!("{} Screenshot", crate::lucide::SAVE)).size(10.5).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_shot).on_hover_text("Capture device screenshot and save to ~/Downloads").clicked() {
                                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                                let dest = std::path::PathBuf::from(home).join("Downloads").join(format!("screenshot_{}.png", ts));
                                match crate::adb::take_screenshot(serial, &dest) {
                                    Ok(_) => self.adb_status_msg = Some((format!("Screenshot saved to {}", dest.display()), false, Instant::now())),
                                    Err(e) => self.adb_status_msg = Some((format!("Screenshot failed: {}", e), true, Instant::now())),
                                }
                            }

                            // Pause / Resume toggle button
                            if self.adb_mirror_active {
                                let btn_pause = egui::Button::new(RichText::new(format!("{} Pause Stream", crate::lucide::PAUSE)).size(10.5).strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(220, 38, 38))
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_pause).clicked() {
                                    self.stop_builtin_mirror();
                                }
                            } else {
                                let btn_resume = egui::Button::new(RichText::new(format!("{} Resume Stream", crate::lucide::PLAY)).size(10.5).strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(2, 132, 199))
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_resume).clicked() {
                                    self.start_builtin_mirror(serial);
                                }
                            }
                        });
                    });
                });

            ui.add_space(6.0);

            // Centered Phone Display Mockup Bezel
            let (dw, dh) = self.adb_mirror_device_res;
            let aspect = if dh > 0 { (dw as f32) / (dh as f32) } else { 0.625 };
            let avail_h = (ui.available_height() - 54.0).clamp(240.0, 680.0);
            let screen_h = avail_h;
            let screen_w = screen_h * aspect;
            let bezel_pad_x = 10.0_f32;
            let bezel_pad_y = 10.0_f32;
            let total_bezel_w = screen_w + bezel_pad_x * 2.0;
            let total_bezel_h = screen_h + bezel_pad_y * 2.0 + 20.0;

            ui.horizontal(|ui| {
                let margin_x = (ui.available_width() - total_bezel_w).max(0.0) / 2.0;
                ui.add_space(margin_x);

                ui.vertical(|ui| {
                    // Outer phone chassis
                    let (bezel_rect, _) = ui.allocate_exact_size(Vec2::new(total_bezel_w, total_bezel_h), egui::Sense::hover());
                    ui.painter().rect(
                        bezel_rect,
                        Rounding::same(24.0),
                        Color32::from_rgb(14, 16, 23),
                        Stroke::new(1.5_f32, Color32::from_rgb(38, 44, 62)),
                    );

                    // Top phone speaker slit / camera punch hole
                    let speaker_w = 40.0_f32.min(screen_w * 0.25);
                    let speaker_rect = Rect::from_center_size(
                        pos2(bezel_rect.center().x, bezel_rect.min.y + 10.0),
                        Vec2::new(speaker_w, 4.0),
                    );
                    ui.painter().rect_filled(speaker_rect, Rounding::same(2.0), Color32::from_rgb(30, 34, 48));

                    // Screen area
                    let screen_rect = Rect::from_min_size(
                        pos2(bezel_rect.min.x + bezel_pad_x, bezel_rect.min.y + bezel_pad_y + 10.0),
                        Vec2::new(screen_w, screen_h),
                    );
                    ui.painter().rect_filled(screen_rect, Rounding::same(10.0), Color32::BLACK);

                    if let Some(ref tex) = self.adb_mirror_texture {
                        // Render texture directly inside screen_rect
                        ui.painter().image(
                            tex.id(),
                            screen_rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        // Touch / tap & drag interactive handler
                        let screen_resp = ui.allocate_rect(screen_rect, egui::Sense::click_and_drag());
                        if screen_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }

                        if screen_resp.drag_started() {
                            self.adb_mirror_drag_start = screen_resp.interact_pointer_pos();
                        }

                        if screen_resp.drag_stopped() {
                            if let Some(start_p) = self.adb_mirror_drag_start.take() {
                                if let Some(end_p) = screen_resp.interact_pointer_pos() {
                                    let delta = end_p - start_p;
                                    let (dev_w, dev_h) = (self.adb_mirror_device_res.0 as f32, self.adb_mirror_device_res.1 as f32);
                                    let x1 = (((start_p.x - screen_rect.min.x) / screen_rect.width()) * dev_w).clamp(0.0, dev_w) as i32;
                                    let y1 = (((start_p.y - screen_rect.min.y) / screen_rect.height()) * dev_h).clamp(0.0, dev_h) as i32;
                                    let x2 = (((end_p.x - screen_rect.min.x) / screen_rect.width()) * dev_w).clamp(0.0, dev_w) as i32;
                                    let y2 = (((end_p.y - screen_rect.min.y) / screen_rect.height()) * dev_h).clamp(0.0, dev_h) as i32;
                                    let ser = serial.to_string();
                                    if delta.length() > 8.0 {
                                        std::thread::spawn(move || {
                                            let _ = crate::adb::send_swipe(&ser, x1, y1, x2, y2, 220);
                                        });
                                    } else {
                                        std::thread::spawn(move || {
                                            let _ = crate::adb::send_tap(&ser, x1, y1);
                                        });
                                    }
                                }
                            }
                        } else if screen_resp.clicked() {
                            if let Some(pos) = screen_resp.interact_pointer_pos() {
                                let (dev_w, dev_h) = (self.adb_mirror_device_res.0 as f32, self.adb_mirror_device_res.1 as f32);
                                let x = (((pos.x - screen_rect.min.x) / screen_rect.width()) * dev_w).clamp(0.0, dev_w) as i32;
                                let y = (((pos.y - screen_rect.min.y) / screen_rect.height()) * dev_h).clamp(0.0, dev_h) as i32;
                                let ser = serial.to_string();
                                std::thread::spawn(move || {
                                    let _ = crate::adb::send_tap(&ser, x, y);
                                });
                            }
                        }
                    } else {
                        // Connecting placeholder
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(screen_rect), |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(screen_h * 0.4);
                                ui.label(RichText::new(crate::lucide::SCREEN_SHARE).size(36.0).color(Color32::from_rgb(56, 189, 248)));
                                ui.add_space(10.0);
                                ui.label(RichText::new("Connecting to Android Display...").size(12.5).strong().color(Color32::WHITE));
                                ui.label(RichText::new("Built-in native frame grabber").size(10.0).color(p.text_muted));
                                ui.add_space(12.0);
                                let btn_start = egui::Button::new(RichText::new("Start Streaming").size(11.0).color(Color32::WHITE))
                                    .fill(p.accent)
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_start).clicked() {
                                    self.start_builtin_mirror(serial);
                                }
                            });
                        });
                    }

                    ui.add_space(8.0);

                    // Android Hardware Navigation Pill Bar
                    Frame::none()
                        .fill(Color32::from_rgb(18, 20, 28))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 36, 50)))
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 5.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let nav_btns = [
                                    ("◀ Back", 4u32),
                                    ("● Home", 3u32),
                                    ("■ Recents", 187u32),
                                    ("🔈 Vol-", 25u32),
                                    ("🔊 Vol+", 24u32),
                                    ("⏻ Power", 26u32),
                                ];
                                for (label, keycode) in nav_btns {
                                    let btn = egui::Button::new(RichText::new(label).size(10.5).color(p.text_secondary))
                                        .fill(Color32::TRANSPARENT)
                                        .rounding(Rounding::same(999.0));
                                    if ui.add(btn).clicked() {
                                        let ser = serial.to_string();
                                        std::thread::spawn(move || {
                                            let _ = crate::adb::send_keyevent(&ser, keycode);
                                        });
                                    }
                                    ui.add_space(4.0);
                                }
                            });
                        });
                });
            });
        });
    }

    fn render_adb_scrcpy_panel(&mut self, ui: &mut Ui, serial: &str) {
        let p = self.palette();
        let is_running = self.is_scrcpy_active(serial);
        let mut toggle_scrcpy = false;

        ScrollArea::vertical()
            .id_salt("adb_scrcpy_dashboard_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // 1. Status & Launcher Banner
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 16.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            Frame::none()
                                .fill(Color32::from_rgb(14, 40, 58))
                                .rounding(Rounding::same(10.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 14.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(crate::lucide::SCREEN_SHARE).size(26.0).color(Color32::from_rgb(56, 189, 248)));
                                });

                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Real-time Screen Mirroring & Control (scrcpy)").size(15.0).strong().color(p.text_primary));
                                    if is_running {
                                        let pid = self.adb_scrcpy_pids.get(serial).copied().unwrap_or(0);
                                        Frame::none()
                                            .fill(Color32::from_rgb(22, 101, 52))
                                            .rounding(Rounding::same(999.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(format!("● LIVE (PID {})", pid)).size(9.5).monospace().color(Color32::from_rgb(187, 247, 208)));
                                            });
                                    } else {
                                        Frame::none()
                                            .fill(Color32::from_rgb(31, 41, 55))
                                            .rounding(Rounding::same(999.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new("○ INACTIVE").size(9.5).monospace().color(p.text_muted));
                                            });
                                    }
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new("Hardware-accelerated ultra-low latency (<35ms), 60–120 FPS external window with keyboard/mouse forwarding and audio streaming.")
                                    .size(11.0).color(p.text_muted));
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if is_running {
                                    let btn_stop = egui::Button::new(RichText::new(format!("{} Stop Mirroring", crate::lucide::SQUARE)).size(12.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(220, 38, 38))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(140.0, 34.0));
                                    if ui.add(btn_stop).clicked() {
                                        toggle_scrcpy = true;
                                    }
                                } else {
                                    let btn_start = egui::Button::new(RichText::new(format!("{} Launch Screen Mirror", crate::lucide::PLAY)).size(12.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(2, 132, 199))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(170.0, 34.0));
                                    if ui.add(btn_start).clicked() {
                                        toggle_scrcpy = true;
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(12.0);

                // 2. Stream & Display Parameters Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("STREAM & DISPLAY PARAMETERS").size(10.5).monospace().strong().color(p.text_secondary));
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.adb_scrcpy_stay_awake, "Stay Awake on Host Power (--stay-awake)");
                            ui.add_space(16.0);
                            ui.checkbox(&mut self.adb_scrcpy_screen_off, "Turn Off Physical Device Screen (--turn-screen-off)");
                            ui.add_space(16.0);
                            ui.checkbox(&mut self.adb_scrcpy_no_audio, "Mute Audio Forwarding (--no-audio)");
                        });

                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Target Bitrate:").size(11.0).color(p.text_secondary));
                            ui.add(egui::Slider::new(&mut self.adb_scrcpy_bitrate_mbps, 2..=32).text("Mbps").step_by(2.0));

                            ui.add_space(20.0);

                            ui.label(RichText::new("Max Framerate:").size(11.0).color(p.text_secondary));
                            ui.add(egui::Slider::new(&mut self.adb_scrcpy_max_fps, 30..=120).text("FPS").step_by(10.0));
                        });
                    });

                ui.add_space(12.0);

                // 3. scrcpy Keyboard Shortcuts & Controls Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("SCRCPY KEYBOARD SHORTCUTS (MOD = Alt or Super)").size(10.5).monospace().strong().color(p.text_secondary));
                        ui.add_space(10.0);

                        let shortcuts = [
                            ("MOD + f", "Toggle Fullscreen mode"),
                            ("MOD + h", "Press Home button"),
                            ("MOD + b", "Press Back button"),
                            ("MOD + s", "Open App Switcher (Recents)"),
                            ("MOD + p", "Power on / off screen"),
                            ("MOD + Up / Down", "Increase / decrease volume"),
                            ("MOD + c / v", "Copy / Paste to or from device clipboard"),
                            ("Drag & Drop .apk", "Directly install APK on device"),
                        ];

                        ui.columns(2, |cols| {
                            for (i, (key, desc)) in shortcuts.iter().enumerate() {
                                let target_col = &mut cols[i % 2];
                                target_col.horizontal(|ui| {
                                    Frame::none()
                                        .fill(Color32::from_rgb(24, 28, 40))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(*key).size(10.0).monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                                        });
                                    ui.label(RichText::new(*desc).size(10.5).color(p.text_muted));
                                });
                                target_col.add_space(6.0);
                            }
                        });
                    });
            });

        if toggle_scrcpy {
            self.launch_scrcpy_for_device(serial);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 3: CLIPBOARD SYNC & SHARE
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_clipboard_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ScrollArea::vertical()
            .id_salt("adb_clipboard_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.columns(2, |cols| {
                    // Left Col: Fetch from Device Clipboard
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(&mut cols[0], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::CLIPBOARD).size(14.0).color(p.accent));
                            ui.label(RichText::new("ANDROID DEVICE CLIPBOARD").size(10.5).monospace().strong().color(p.text_primary));
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new("Inspect and pull text currently copied on your Android device.").size(10.0).color(p.text_muted));
                        ui.add_space(8.0);

                        let cached = self.adb_clipboard_cache.entry(serial.clone()).or_default();
                        ui.add(egui::TextEdit::multiline(cached).desired_rows(8).desired_width(ui.available_width()).hint_text("Device clipboard is empty or unqueried"));

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            let btn_get = egui::Button::new(RichText::new(format!("{} Fetch Clipboard", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_get).clicked() {
                                match crate::adb::get_clipboard(&serial) {
                                    Ok(txt) => {
                                        self.adb_clipboard_cache.insert(serial.clone(), txt);
                                        self.adb_status_msg = Some(("Clipboard fetched".to_string(), false, Instant::now()));
                                    }
                                    Err(e) => {
                                        self.adb_status_msg = Some((format!("Fetch clipboard error: {}", e), true, Instant::now()));
                                    }
                                }
                            }

                            let btn_copy = egui::Button::new(RichText::new(format!("{} Copy to PC", crate::lucide::COPY)).size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_copy).clicked() {
                                if let Some(txt) = self.adb_clipboard_cache.get(&serial) {
                                    ui.output_mut(|o| o.copied_text = txt.clone());
                                    self.adb_status_msg = Some(("Copied to PC clipboard".to_string(), false, Instant::now()));
                                }
                            }
                        });
                    });

                    // Right Col: Send to Device Clipboard
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(&mut cols[1], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::UPLOAD).size(14.0).color(Color32::from_rgb(56, 189, 248)));
                            ui.label(RichText::new("SEND TEXT TO ANDROID CLIPBOARD").size(10.5).monospace().strong().color(p.text_primary));
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new("Paste or type text on PC and inject directly into Android clipboard.").size(10.0).color(p.text_muted));
                        ui.add_space(8.0);

                        ui.add(egui::TextEdit::multiline(&mut self.adb_clipboard_send_text).desired_rows(8).desired_width(ui.available_width()).hint_text("Enter text to send to device clipboard..."));

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            let btn_send = egui::Button::new(RichText::new("Send to Device Clipboard").size(11.0).strong().color(Color32::WHITE))
                                .fill(p.accent)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_send).clicked() {
                                let txt = self.adb_clipboard_send_text.clone();
                                if !txt.is_empty() {
                                    let _ = crate::adb::set_clipboard(&serial, &txt);
                                    self.adb_status_msg = Some(("Sent to Android clipboard".to_string(), false, Instant::now()));
                                }
                            }

                            let btn_clear = egui::Button::new(RichText::new("Clear").size(11.0).color(p.text_muted))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_clear).clicked() {
                                self.adb_clipboard_send_text.clear();
                            }
                        });
                    });
                });
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 4: INTERACTIVE SHELL TERMINAL
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_terminal_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ui.vertical(|ui| {
            // Presets row
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Presets:").size(10.5).color(p.text_muted));

                let presets = [
                    ("Logcat (Latest 100)", "logcat -d -t 100"),
                    ("Kernel dmesg", "dmesg | tail -n 50"),
                    ("Dumpsys Battery", "dumpsys battery"),
                    ("Top Processes", "top -m 10 -n 1"),
                    ("IP Addr", "ip addr show"),
                    ("Getprop", "getprop"),
                ];

                for (label, cmd) in presets {
                    let btn = egui::Button::new(RichText::new(label).size(10.0).color(p.text_secondary))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn).clicked() {
                        match crate::adb::adb_cmd(Some(&serial), &["shell", cmd]) {
                            Ok(out) => self.adb_shell_history.push((cmd.to_string(), out, false)),
                            Err(e) => self.adb_shell_history.push((cmd.to_string(), e, true)),
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let btn_clear = egui::Button::new(RichText::new("Clear Output").size(10.0).color(p.text_muted))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn_clear).clicked() {
                        self.adb_shell_history.clear();
                    }

                    let btn_ext = egui::Button::new(RichText::new(format!("{} External Shell", crate::lucide::EXTERNAL_LINK)).size(10.0).color(Color32::from_rgb(56, 189, 248)))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(4.0));
                    if ui.add(btn_ext).on_hover_text("Open interactive ADB shell session in system terminal emulator").clicked() {
                        match crate::adb::launch_external_adb_terminal(&serial) {
                            Ok(_) => self.adb_status_msg = Some(("External ADB terminal opened".to_string(), false, Instant::now())),
                            Err(e) => self.adb_status_msg = Some((format!("Terminal error: {}", e), true, Instant::now())),
                        }
                    }
                });
            });

            ui.add_space(6.0);

            // Terminal Output Well
            let total_h = (ui.available_height() - 42.0).max(100.0);
            Frame::none()
                .fill(Color32::from_rgb(10, 11, 16))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(26, 28, 40)))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(total_h);
                    ScrollArea::vertical()
                        .id_salt("adb_shell_history_scroll")
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if self.adb_shell_history.is_empty() {
                                ui.label(RichText::new(format!("Connected to Android Shell: {}\nType command below or click a preset.", serial))
                                    .size(10.5).monospace().color(p.text_muted));
                            } else {
                                for (cmd, out, is_err) in &self.adb_shell_history {
                                    ui.label(RichText::new(format!("$ {}", cmd)).size(11.0).monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                                    let col = if *is_err { Color32::from_rgb(248, 113, 113) } else { Color32::from_rgb(226, 232, 240) };
                                    ui.label(RichText::new(out).size(10.0).monospace().color(col));
                                    ui.add_space(4.0);
                                }
                            }
                        });
                });

            ui.add_space(6.0);

            // Command input row
            ui.horizontal(|ui| {
                ui.label(RichText::new("$").size(12.0).monospace().strong().color(Color32::from_rgb(56, 189, 248)));
                let enter = ui.add(egui::TextEdit::singleline(&mut self.adb_shell_cmd_input)
                    .hint_text("Enter Android shell command (e.g. ls -la /sdcard)...")
                    .desired_width(ui.available_width() - 80.0));

                let btn_run = egui::Button::new(RichText::new("Run").size(11.0).strong().color(Color32::WHITE))
                    .fill(p.accent)
                    .rounding(Rounding::same(6.0));

                if (enter.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.add(btn_run).clicked() {
                    let cmd = self.adb_shell_cmd_input.trim().to_string();
                    if !cmd.is_empty() {
                        match crate::adb::adb_cmd(Some(&serial), &["shell", &cmd]) {
                            Ok(out) => self.adb_shell_history.push((cmd, out, false)),
                            Err(e) => self.adb_shell_history.push((cmd, e, true)),
                        }
                        self.adb_shell_cmd_input.clear();
                    }
                }
            });
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ─────────────────────────────────────────────────────────────────────────
    // TAB 5: STORAGE SHARE & FILE EXPLORER
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_storage_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        let mut nav_into: Option<String> = None;
        let mut nav_to_exact_path: Option<String> = None;
        let mut pull_target: Option<String> = None;
        let mut delete_target: Option<String> = None;

        ui.vertical(|ui| {
            // Path Navigation and Action Bar Card
            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Up Button
                        let btn_up = egui::Button::new(RichText::new("⬆ Up").size(11.0).color(p.text_primary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_up).clicked() {
                            let mut parts: Vec<&str> = self.adb_file_current_path.split('/').filter(|s| !s.is_empty()).collect();
                            if !parts.is_empty() {
                                parts.pop();
                                nav_to_exact_path = Some(if parts.is_empty() { "/".to_string() } else { format!("/{}", parts.join("/")) });
                            }
                        }

                        ui.add_space(6.0);

                        // Clickable Breadcrumbs
                        ui.horizontal(|ui| {
                            let segments: Vec<String> = self.adb_file_current_path.trim_matches('/').split('/').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();

                            if ui.link(RichText::new("/").size(12.0).monospace().color(p.text_secondary)).clicked() {
                                nav_to_exact_path = Some("/".to_string());
                            }

                            let mut accumulated = String::new();
                            for (idx, seg) in segments.iter().enumerate() {
                                ui.label(RichText::new("/").size(11.0).monospace().color(p.text_muted));
                                accumulated.push('/');
                                accumulated.push_str(seg);
                                let is_last = idx == segments.len() - 1;
                                let col = if is_last { Color32::from_rgb(56, 189, 248) } else { p.text_secondary };
                                let link = ui.link(RichText::new(seg).size(11.5).monospace().strong().color(col));
                                if link.clicked() && !is_last {
                                    nav_to_exact_path = Some(accumulated.clone());
                                }
                            }
                        });

                        ui.add_space(8.0);
                        // Filter input
                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut self.adb_file_filter).hint_text("Filter files...").desired_width(120.0));

                        // Action Buttons on Right with proper spacing
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(4.0);

                            // Refresh
                            let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_ref).clicked() {
                                if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                                    self.adb_file_entries = entries;
                                }
                            }

                            // Install APK button
                            let btn_apk = egui::Button::new(RichText::new(format!("{} Install APK", crate::lucide::PACKAGE)).size(11.0).strong().color(Color32::WHITE))
                                .fill(p.accent)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_apk).on_hover_text("Select .apk from host PC and install to device").clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("Android APK (*.apk)", &["apk"]).pick_file() {
                                    match crate::adb::install_apk(&serial, &path) {
                                        Ok(_) => self.adb_status_msg = Some(("APK installed successfully".to_string(), false, Instant::now())),
                                        Err(e) => self.adb_status_msg = Some((format!("APK install error: {}", e), true, Instant::now())),
                                    }
                                }
                            }

                            // Push File button
                            let btn_push = egui::Button::new(RichText::new(format!("{} Push File", crate::lucide::UPLOAD)).size(11.0).color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_push).on_hover_text("Select local file and push to current folder on device").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_file() {
                                    let dest = format!("{}/{}", self.adb_file_current_path.trim_end_matches('/'), path.file_name().unwrap_or_default().to_string_lossy());
                                    match crate::adb::push_file(&serial, &path, &dest) {
                                        Ok(_) => {
                                            self.adb_status_msg = Some(("File pushed to device".to_string(), false, Instant::now()));
                                            if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                                                self.adb_file_entries = entries;
                                            }
                                        }
                                        Err(e) => self.adb_status_msg = Some((format!("Push failed: {}", e), true, Instant::now())),
                                    }
                                }
                            }
                        });
                    });
                });

            ui.add_space(6.0);

            // Initial load if empty
            if self.adb_file_entries.is_empty() {
                if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                    self.adb_file_entries = entries;
                }
            }

            // File Listing Table with Auto-Shrink False to fill available space
            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    // Header row
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("NAME").size(9.5).monospace().strong().color(p.text_muted));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("ACTIONS").size(9.5).monospace().strong().color(p.text_muted));
                            ui.add_space(50.0);
                            ui.label(RichText::new("SIZE").size(9.5).monospace().strong().color(p.text_muted));
                            ui.add_space(45.0);
                            ui.label(RichText::new("PERMISSIONS").size(9.5).monospace().strong().color(p.text_muted));
                        });
                    });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ScrollArea::vertical()
                        .id_salt("adb_files_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            let filter = self.adb_file_filter.to_lowercase();
                            let mut sorted_entries = self.adb_file_entries.clone();
                            sorted_entries.sort_by(|a, b| {
                                b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                            });

                            let mut visible_count = 0;
                            for entry in &sorted_entries {
                                if !filter.is_empty() && !entry.name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                visible_count += 1;

                                ui.horizontal(|ui| {
                                    let icon = if entry.is_dir { crate::lucide::FOLDER } else { crate::lucide::FILE_TEXT };
                                    let icon_col = if entry.is_dir { Color32::from_rgb(56, 189, 248) } else { p.text_secondary };
                                    ui.label(RichText::new(icon).size(13.0).color(icon_col));

                                    if entry.is_dir {
                                        if ui.link(RichText::new(&entry.name).size(11.5).strong().color(Color32::from_rgb(224, 242, 254))).clicked() {
                                            nav_into = Some(entry.name.clone());
                                        }
                                    } else {
                                        ui.label(RichText::new(&entry.name).size(11.0).color(p.text_primary));
                                    }

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(4.0);
                                        if !entry.is_dir {
                                            let btn_del = egui::Button::new(RichText::new(format!("{} Delete", crate::lucide::TRASH)).size(9.5).color(Color32::from_rgb(239, 68, 68)))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_del).on_hover_text("Delete file from Android device").clicked() {
                                                delete_target = Some(entry.name.clone());
                                            }

                                            let btn_dl = egui::Button::new(RichText::new(format!("{} Pull", crate::lucide::DOWNLOAD)).size(9.5).color(p.text_primary))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_dl).on_hover_text("Download to host PC ~/Downloads").clicked() {
                                                pull_target = Some(entry.name.clone());
                                            }

                                            let sz_kb = entry.size_bytes as f64 / 1024.0;
                                            let sz_str = if sz_kb >= 1024.0 { format!("{:.1} MB", sz_kb / 1024.0) } else { format!("{:.0} KB", sz_kb) };
                                            ui.label(RichText::new(sz_str).size(9.5).monospace().color(p.text_muted));
                                        } else {
                                            ui.label(RichText::new("<DIR>").size(9.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                        }
                                        ui.label(RichText::new(&entry.permissions).size(9.5).monospace().color(Color32::from_rgb(100, 116, 139)));
                                    });
                                });
                                ui.separator();
                            }

                            if visible_count == 0 {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(RichText::new(crate::lucide::FOLDER_OPEN).size(28.0).color(p.text_muted));
                                    ui.add_space(6.0);
                                    ui.label(RichText::new("Folder is empty or no files match search").size(11.0).color(p.text_muted));
                                });
                            }
                        });
                });

            if let Some(target) = nav_to_exact_path {
                self.adb_file_current_path = target;
                if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                    self.adb_file_entries = entries;
                }
            }
            if let Some(folder) = nav_into {
                self.adb_file_current_path = format!("{}/{}", self.adb_file_current_path.trim_end_matches('/'), folder);
                if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                    self.adb_file_entries = entries;
                }
            }
            if let Some(filename) = pull_target {
                let remote = format!("{}/{}", self.adb_file_current_path.trim_end_matches('/'), filename);
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dest = PathBuf::from(home).join("Downloads").join(&filename);
                match crate::adb::pull_file(&serial, &remote, &dest) {
                    Ok(_) => self.adb_status_msg = Some((format!("Pulled to {}", dest.display()), false, Instant::now())),
                    Err(e) => self.adb_status_msg = Some((format!("Pull failed: {}", e), true, Instant::now())),
                }
            }
            if let Some(filename) = delete_target {
                let remote = format!("{}/{}", self.adb_file_current_path.trim_end_matches('/'), filename);
                match crate::adb::delete_file(&serial, &remote) {
                    Ok(_) => {
                        self.adb_status_msg = Some((format!("Deleted {}", filename), false, Instant::now()));
                        if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                            self.adb_file_entries = entries;
                        }
                    }
                    Err(e) => self.adb_status_msg = Some((format!("Delete failed: {}", e), true, Instant::now())),
                }
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 6: FIRMWARE UPDATES & FASTBOOT TOOLKIT
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_firmware_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ScrollArea::vertical()
            .id_salt("adb_firmware_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Card 1: Reboot Target Actions
                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::POWER).size(14.0).color(p.accent));
                        ui.label(RichText::new("SYSTEM & RECOVERY REBOOT CONTROLS").size(11.0).monospace().strong().color(p.text_primary));
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let btn_norm = egui::Button::new(RichText::new("Reboot System").size(11.0).color(p.text_primary)).fill(p.bg_button).rounding(Rounding::same(6.0));
                        if ui.add(btn_norm).clicked() { let _ = crate::adb::reboot_device(&serial, "system"); }

                        let btn_rec = egui::Button::new(RichText::new("Reboot to Recovery (TWRP / Lineage)").size(11.0).color(Color32::from_rgb(56, 189, 248))).fill(p.bg_button).rounding(Rounding::same(6.0));
                        if ui.add(btn_rec).clicked() { let _ = crate::adb::reboot_device(&serial, "recovery"); }

                        let btn_bl = egui::Button::new(RichText::new("Reboot to Bootloader / Fastboot").size(11.0).color(Color32::from_rgb(245, 158, 11))).fill(p.bg_button).rounding(Rounding::same(6.0));
                        if ui.add(btn_bl).clicked() { let _ = crate::adb::reboot_device(&serial, "bootloader"); }

                        let btn_edl = egui::Button::new(RichText::new("Reboot to EDL Mode").size(11.0).color(Color32::from_rgb(239, 68, 68))).fill(p.bg_button).rounding(Rounding::same(6.0));
                        if ui.add(btn_edl).clicked() { let _ = crate::adb::reboot_device(&serial, "edl"); }
                    });
                });

                ui.add_space(10.0);

                // Card 2: OTA Sideload (Recovery Mode)
                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::DOWNLOAD).size(14.0).color(p.accent));
                        ui.label(RichText::new("OTA FIRMWARE SIDELOAD").size(11.0).monospace().strong().color(p.text_primary));
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new("Sideload an Android OTA update package, official firmware update, or custom ROM while device is in Recovery mode (`adb sideload update.zip`).")
                        .size(10.5).color(p.text_muted));
                    ui.add_space(8.0);

                    let btn_side = egui::Button::new(RichText::new("📦 Select Firmware ZIP & Sideload").size(11.5).strong().color(Color32::WHITE))
                        .fill(p.accent)
                        .rounding(Rounding::same(6.0));
                    if ui.add(btn_side).clicked() {
                        if let Some(zip_path) = rfd::FileDialog::new().add_filter("OTA Update Package (*.zip)", &["zip"]).pick_file() {
                            match crate::adb::sideload_ota(&serial, &zip_path) {
                                Ok(_) => self.adb_status_msg = Some(("Sideload completed".to_string(), false, Instant::now())),
                                Err(e) => self.adb_status_msg = Some((format!("Sideload error: {}", e), true, Instant::now())),
                            }
                        }
                    }
                });

                ui.add_space(10.0);

                // Card 3: Fastboot Partition Flashing & Diagnostics
                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::ZAP).size(14.0).color(Color32::from_rgb(245, 158, 11)));
                        ui.label(RichText::new("FASTBOOT PARTITION FLASHER").size(11.0).monospace().strong().color(p.text_primary));
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new("Direct partition flashing via fastboot protocol. Make sure device is in bootloader/fastboot mode.")
                        .size(10.5).color(p.text_muted));
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Target Partition:").size(11.0).color(p.text_secondary));
                        egui::ComboBox::from_id_salt("fastboot_part_combo")
                            .selected_text(RichText::new(&self.adb_fastboot_partition).size(11.0).color(p.text_primary))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.adb_fastboot_partition, "boot".to_string(), "boot (Kernel/Ramdisk)");
                                ui.selectable_value(&mut self.adb_fastboot_partition, "recovery".to_string(), "recovery (TWRP/Custom)");
                                ui.selectable_value(&mut self.adb_fastboot_partition, "init_boot".to_string(), "init_boot (Android 13+)");
                                ui.selectable_value(&mut self.adb_fastboot_partition, "vbmeta".to_string(), "vbmeta (AVB Verification)");
                            });

                        let btn_flash = egui::Button::new(RichText::new("⚡ Select .img & Flash").size(11.0).strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(217, 119, 6))
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_flash).clicked() {
                            if let Some(img_path) = rfd::FileDialog::new().add_filter("Partition Image (*.img)", &["img"]).pick_file() {
                                match crate::adb::fastboot_flash(&self.adb_fastboot_partition, &img_path) {
                                    Ok(res) => self.adb_status_msg = Some((format!("Flashed: {}", res), false, Instant::now())),
                                    Err(e) => self.adb_status_msg = Some((format!("Flash error: {}", e), true, Instant::now())),
                                }
                            }
                        }
                    });

                    ui.add_space(10.0);

                    // Bugreport button
                    let btn_bug = egui::Button::new(RichText::new(format!("{} Generate Android Bugreport (.zip)", crate::lucide::FILE_TEXT)).size(11.0).color(p.text_primary))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(6.0));
                    if ui.add(btn_bug).clicked() {
                        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                        let dest = PathBuf::from(home).join("Downloads/bugreport.zip");
                        match crate::adb::generate_bugreport(&serial, &dest) {
                            Ok(_) => self.adb_status_msg = Some((format!("Bugreport saved to {}", dest.display()), false, Instant::now())),
                            Err(e) => self.adb_status_msg = Some((format!("Bugreport failed: {}", e), true, Instant::now())),
                        }
                    }
                });
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ─────────────────────────────────────────────────────────────────────────
    // TAB 7: APPS & PACKAGE MANAGER
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_apps_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ui.vertical(|ui| {
            // Controls Card
            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(14.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut self.adb_package_filter).hint_text("Filter packages...").desired_width(170.0));

                        ui.add_space(8.0);
                        if ui.selectable_label(!self.adb_package_is_system, "Third-Party Apps").clicked() {
                            self.adb_package_is_system = false;
                            self.adb_packages_cache = crate::adb::list_packages(&serial, true);
                        }
                        if ui.selectable_label(self.adb_package_is_system, "System Apps").clicked() {
                            self.adb_package_is_system = true;
                            self.adb_packages_cache = crate::adb::list_packages(&serial, false);
                        }

                        // App count pill
                        let count = self.adb_packages_cache.len();
                        ui.add_space(4.0);
                        Frame::none()
                            .fill(Color32::from_rgb(18, 22, 32))
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(format!("{} Packages", count)).size(10.0).monospace().color(p.text_secondary));
                            });

                        // Refresh button on right with clean padding
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(4.0);
                            let btn_ref = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                            if ui.add(btn_ref).clicked() {
                                self.adb_packages_cache = crate::adb::list_packages(&serial, !self.adb_package_is_system);
                            }
                        });
                    });
                });

            ui.add_space(6.0);

            if self.adb_packages_cache.is_empty() {
                self.adb_packages_cache = crate::adb::list_packages(&serial, !self.adb_package_is_system);
            }

            // Package listing with auto_shrink false
            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("adb_apps_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let filter = self.adb_package_filter.to_lowercase();
                            let mut visible_count = 0;
                            let mut uninstall_target: Option<String> = None;
                            let mut clear_target: Option<String> = None;
                            let mut stop_target: Option<String> = None;
                            let mut launch_target: Option<String> = None;

                            for pkg in &self.adb_packages_cache {
                                if !filter.is_empty() && !pkg.name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                visible_count += 1;

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::PACKAGE).size(13.0).color(Color32::from_rgb(56, 189, 248)));
                                    ui.label(RichText::new(&pkg.name).size(11.0).monospace().strong().color(p.text_primary));

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(4.0);

                                        // Uninstall (3rd party only)
                                        if !pkg.is_system {
                                            let btn_un = egui::Button::new(RichText::new("Uninstall").size(9.5).color(Color32::from_rgb(239, 68, 68)))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_un).on_hover_text("Uninstall application from device").clicked() {
                                                uninstall_target = Some(pkg.name.clone());
                                            }
                                        }

                                        // Clear Data
                                        let btn_clr = egui::Button::new(RichText::new("Clear").size(9.5).color(p.text_muted))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_clr).on_hover_text("Clear all app cache & user data").clicked() {
                                            clear_target = Some(pkg.name.clone());
                                        }

                                        // Force Stop
                                        let btn_stp = egui::Button::new(RichText::new("Stop").size(9.5).color(p.text_secondary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stp).on_hover_text("Force stop running process").clicked() {
                                            stop_target = Some(pkg.name.clone());
                                        }

                                        // Launch
                                        let btn_run = egui::Button::new(RichText::new("Launch").size(9.5).color(Color32::from_rgb(56, 189, 248)))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).on_hover_text("Launch main activity on device").clicked() {
                                            launch_target = Some(pkg.name.clone());
                                        }
                                    });
                                });
                                ui.separator();
                            }

                            if visible_count == 0 {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(RichText::new(crate::lucide::PACKAGE).size(28.0).color(p.text_muted));
                                    ui.add_space(6.0);
                                    ui.label(RichText::new("No packages match current search or category").size(11.0).color(p.text_muted));
                                });
                            }

                            if let Some(pkg_name) = uninstall_target {
                                match crate::adb::uninstall_app(&serial, &pkg_name) {
                                    Ok(_) => {
                                        self.adb_status_msg = Some((format!("Uninstalled {}", pkg_name), false, Instant::now()));
                                        self.adb_packages_cache = crate::adb::list_packages(&serial, !self.adb_package_is_system);
                                    }
                                    Err(e) => self.adb_status_msg = Some((format!("Uninstall failed: {}", e), true, Instant::now())),
                                }
                            }
                            if let Some(pkg_name) = clear_target {
                                match crate::adb::clear_app_data(&serial, &pkg_name) {
                                    Ok(_) => self.adb_status_msg = Some((format!("Cleared data for {}", pkg_name), false, Instant::now())),
                                    Err(e) => self.adb_status_msg = Some((format!("Clear data failed: {}", e), true, Instant::now())),
                                }
                            }
                            if let Some(pkg_name) = stop_target {
                                let _ = crate::adb::force_stop_app(&serial, &pkg_name);
                                self.adb_status_msg = Some((format!("Stopped {}", pkg_name), false, Instant::now()));
                            }
                            if let Some(pkg_name) = launch_target {
                                let _ = crate::adb::launch_app(&serial, &pkg_name);
                                self.adb_status_msg = Some((format!("Launched {}", pkg_name), false, Instant::now()));
                            }
                        });
                });
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TAB 8: WIRELESS ADB & TCP/IP SWITCHER
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_wireless_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ScrollArea::vertical()
            .id_salt("adb_wireless_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // 1. Controller Header Banner & Mode Quick Switcher
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            Frame::none()
                                .fill(Color32::from_rgb(14, 40, 58))
                                .rounding(Rounding::same(10.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(crate::lucide::WIFI).size(22.0).color(Color32::from_rgb(56, 189, 248)));
                                });

                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Wireless ADB & TCP/IP Controller").size(14.0).strong().color(p.text_primary));
                                    let status_text = if adb_dev.is_wireless { "Wireless Connected" } else { "USB Debug Attached" };
                                    let status_col = if adb_dev.is_wireless { Color32::from_rgb(34, 197, 94) } else { Color32::from_rgb(56, 189, 248) };
                                    Frame::none()
                                        .fill(Color32::from_rgb(18, 22, 32))
                                        .rounding(Rounding::same(999.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(status_text).size(9.5).monospace().color(status_col));
                                        });
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new("Untether your Android device from USB cables and manage via Wi-Fi network.")
                                    .size(10.5).color(p.text_muted));
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(4.0);
                                let btn_usb = egui::Button::new(RichText::new(format!("{} Revert to USB Mode", crate::lucide::USB)).size(11.0).color(p.text_primary))
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_usb).on_hover_text("Switch Android ADB back to USB mode (adb usb)").clicked() {
                                    match crate::adb::reset_to_usb(&serial) {
                                        Ok(res) => self.adb_status_msg = Some((format!("Reverted to USB: {}", res), false, Instant::now())),
                                        Err(e) => self.adb_status_msg = Some((format!("USB revert error: {}", e), true, Instant::now())),
                                    }
                                }

                                let btn_tcpip = egui::Button::new(RichText::new(format!("{} Enable TCP/IP (Port 5555)", crate::lucide::WIFI)).size(11.0).strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(2, 132, 199))
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_tcpip).on_hover_text("Start ADB TCP/IP server on device port 5555 (adb tcpip 5555)").clicked() {
                                    match crate::adb::enable_tcpip(&serial, 5555) {
                                        Ok(res) => self.adb_status_msg = Some((format!("TCP/IP 5555 enabled: {}", res), false, Instant::now())),
                                        Err(e) => self.adb_status_msg = Some((format!("TCP/IP error: {}", e), true, Instant::now())),
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(10.0);

                // Discover valid network endpoints on device (filtering dummy interfaces like ds-br0, p2p, dummy)
                let endpoints = crate::adb::discover_device_endpoints(&serial);

                // Auto-fill manual IP if empty
                if self.adb_wireless_ip_input.is_empty() {
                    if let Some(first) = endpoints.first() {
                        self.adb_wireless_ip_input = first.ip.clone();
                    }
                }

                let mut action_connect: Option<String> = None;
                let mut action_disconnect: Option<String> = None;
                let mut action_switch: Option<String> = None;

                // 2-Column Grid: Discovered Endpoints (Left) vs Manual Connect (Right)
                ui.columns(2, |cols| {
                    // Left Column: Discovered Endpoints
                    cols[0].vertical(|ui| {
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::RADIO).size(13.0).color(Color32::from_rgb(56, 189, 248)));
                                    ui.label(RichText::new("DISCOVERED LOCAL ENDPOINTS").size(10.5).monospace().strong().color(p.text_secondary));
                                });
                                ui.add_space(4.0);
                                ui.label(RichText::new("Detected valid network interfaces on this Android device.").size(10.0).color(p.text_muted));
                                ui.add_space(10.0);

                                if endpoints.is_empty() {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(20.0);
                                        ui.label(RichText::new("No Wi-Fi or Ethernet IP addresses discovered on device.").size(10.5).color(p.text_muted));
                                        ui.add_space(10.0);
                                    });
                                } else {
                                    for ep in &endpoints {
                                        let target = format!("{}:5555", ep.ip);
                                        let is_active = self.adb_devices.iter().any(|d| d.serial == target);

                                        Frame::none()
                                            .fill(Color32::from_rgb(18, 20, 28))
                                            .stroke(Stroke::new(1.0_f32, if is_active { Color32::from_rgb(34, 197, 94) } else { Color32::from_rgb(30, 34, 48) }))
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(crate::lucide::WIFI).size(14.0).color(Color32::from_rgb(56, 189, 248)));
                                                    ui.vertical(|ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(&target).size(12.0).monospace().strong().color(p.text_primary));
                                                            Frame::none()
                                                                .fill(if ep.is_wifi { Color32::from_rgb(14, 34, 28) } else { Color32::from_rgb(22, 28, 42) })
                                                                .rounding(Rounding::same(4.0))
                                                                .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                                                .show(ui, |ui| {
                                                                    ui.label(RichText::new(&ep.label).size(8.5).monospace().color(if ep.is_wifi { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(148, 163, 184) }));
                                                                });

                                                            if is_active {
                                                                Frame::none()
                                                                    .fill(Color32::from_rgb(20, 44, 30))
                                                                    .rounding(Rounding::same(4.0))
                                                                    .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                                                    .show(ui, |ui| {
                                                                        ui.label(RichText::new("● ACTIVE").size(8.5).monospace().color(Color32::from_rgb(34, 197, 94)));
                                                                    });
                                                            }
                                                        });
                                                    });

                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                        if is_active {
                                                            let btn_d = egui::Button::new(RichText::new("Disconnect").size(10.0).color(Color32::from_rgb(239, 68, 68)))
                                                                .fill(p.bg_button)
                                                                .rounding(Rounding::same(4.0));
                                                            if ui.add(btn_d).clicked() {
                                                                action_disconnect = Some(target.clone());
                                                            }

                                                            let target_id = format!("adb_{}", target.replace(['.', ':', '-'], "_"));
                                                            if self.selected_machine != target_id {
                                                                let btn_s = egui::Button::new(RichText::new("Switch To").size(10.0).strong().color(Color32::WHITE))
                                                                    .fill(p.accent)
                                                                    .rounding(Rounding::same(4.0));
                                                                if ui.add(btn_s).clicked() {
                                                                    action_switch = Some(target_id);
                                                                }
                                                            }
                                                        } else {
                                                            let btn_c = egui::Button::new(RichText::new("⚡ Connect").size(10.0).strong().color(Color32::WHITE))
                                                                .fill(Color32::from_rgb(2, 132, 199))
                                                                .rounding(Rounding::same(4.0));
                                                            if ui.add(btn_c).clicked() {
                                                                action_connect = Some(target.clone());
                                                            }
                                                        }
                                                    });
                                                });
                                            });
                                        ui.add_space(6.0);
                                    }
                                }
                            });
                    });

                    // Right Column: Manual Connect & Android 11+ Pairing
                    cols[1].vertical(|ui| {
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::LINK).size(13.0).color(Color32::from_rgb(56, 189, 248)));
                                    ui.label(RichText::new("MANUAL DIRECT CONNECT").size(10.5).monospace().strong().color(p.text_secondary));
                                });
                                ui.add_space(4.0);
                                ui.label(RichText::new("Connect to any Android device using its IP address and TCP port.").size(10.0).color(p.text_muted));
                                ui.add_space(10.0);

                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("Device IP Address:").size(10.5).color(p.text_secondary));
                                        ui.add(egui::TextEdit::singleline(&mut self.adb_wireless_ip_input).hint_text("e.g. 192.168.1.100").desired_width(180.0));
                                    });

                                    ui.add_space(10.0);
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("Port:").size(10.5).color(p.text_secondary));
                                        ui.add(egui::TextEdit::singleline(&mut self.adb_wireless_port_input).hint_text("5555").desired_width(70.0));
                                    });
                                });

                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    let btn_manual_connect = egui::Button::new(RichText::new("⚡ Connect").size(11.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(2, 132, 199))
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(100.0, 28.0));
                                    if ui.add(btn_manual_connect).clicked() {
                                        let ip = self.adb_wireless_ip_input.trim();
                                        let port = self.adb_wireless_port_input.trim();
                                        if !ip.is_empty() {
                                            let target = format!("{}:{}", ip, if port.is_empty() { "5555" } else { port });
                                            action_connect = Some(target);
                                        }
                                    }

                                    let btn_manual_disc = egui::Button::new(RichText::new(format!("{} Disconnect", crate::lucide::X)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(110.0, 28.0));
                                    if ui.add(btn_manual_disc).clicked() {
                                        let ip = self.adb_wireless_ip_input.trim();
                                        let port = self.adb_wireless_port_input.trim();
                                        if !ip.is_empty() {
                                            let target = format!("{}:{}", ip, if port.is_empty() { "5555" } else { port });
                                            action_disconnect = Some(target);
                                        }
                                    }
                                });

                                ui.add_space(16.0);
                                ui.separator();
                                ui.add_space(10.0);

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::SHIELD).size(13.0).color(Color32::from_rgb(245, 158, 11)));
                                    ui.label(RichText::new("WIRELESS DEBUGGING INSTRUCTIONS").size(10.0).monospace().strong().color(p.text_primary));
                                });
                                ui.add_space(6.0);
                                ui.label(RichText::new("1. Ensure device and host PC are on the same Wi-Fi network.\n2. Click '⚡ Connect' on the discovered Wi-Fi address (TCP mode is auto-configured).\n3. The wireless device is added to your cockpit dock immediately.\n4. You can now safely unplug the USB cable!").size(9.5).color(p.text_muted));
                            });
                    });
                });

                // Execute actions outside UI closures to maintain borrow integrity
                if let Some(target) = action_connect {
                    match self.connect_wireless_device(&serial, &target, &adb_dev.name) {
                        Ok(msg) => self.adb_status_msg = Some((msg, false, Instant::now())),
                        Err(e) => self.adb_status_msg = Some((format!("Connect failed: {}", e), true, Instant::now())),
                    }
                }
                if let Some(target) = action_disconnect {
                    match self.disconnect_wireless_device(&target) {
                        Ok(msg) => self.adb_status_msg = Some((msg, false, Instant::now())),
                        Err(e) => self.adb_status_msg = Some((format!("Disconnect failed: {}", e), true, Instant::now())),
                    }
                }
                if let Some(id) = action_switch {
                    self.selected_machine = id;
                    self.adb_active_subtab = AdbSubTab::Overview;
                }
            });
    }

    // TAB 3: PHONE LINK MESSAGES (SMS) THREADS & COMPOSER
    // ═════════════════════════════════════════════════════════════════════════
    fn render_adb_messages_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        if self.adb_queries_loaded.insert("messages".to_string()) {
            self.adb_sms_messages = crate::adb::get_sms_messages(Some(&serial));
        }

        let mut action_send_msg: Option<(String, String)> = None;
        let mut action_switch_to_call: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("adb_messages_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width();
                let left_w = 320.0_f32.min(avail_w * 0.45);
                let right_w = (avail_w - left_w - 12.0).max(0.0);
                ui.horizontal_top(|ui| {
                    // LEFT COLUMN: Conversation Threads List (320px)
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width((left_w - 28.0).max(120.0));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::MESSAGE_SQUARE).size(15.0).color(Color32::from_rgb(56, 189, 248)));
                                ui.label(RichText::new("Messages").size(15.0).strong().color(p.text_primary));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn_refresh = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(12.0).color(p.text_muted))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_refresh).on_hover_text("Refresh SMS messages").clicked() {
                                        self.adb_sms_messages = crate::adb::get_sms_messages(Some(&serial));
                                    }
                                });
                            });

                            ui.add_space(8.0);

                            // Search bar
                            Frame::none()
                                .fill(Color32::from_rgb(14, 15, 22))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.adb_sms_search)
                                                .hint_text("Search messages...")
                                                .frame(false)
                                                .desired_width(ui.available_width()),
                                        );
                                    });
                                });

                            ui.add_space(10.0);

                            // Group messages by contact
                            let mut contacts: Vec<String> = Vec::new();
                            for msg in &self.adb_sms_messages {
                                if !contacts.contains(&msg.contact_name) {
                                    contacts.push(msg.contact_name.clone());
                                }
                            }

                            if self.adb_sms_selected_contact.is_none() && !contacts.is_empty() {
                                self.adb_sms_selected_contact = Some(contacts[0].clone());
                            }

                            let query = self.adb_sms_search.to_lowercase();

                            ScrollArea::vertical()
                                .id_salt("adb_sms_contacts_scroll")
                                .max_height(560.0)
                                .show(ui, |ui| {
                                    for contact in &contacts {
                                        if !query.is_empty() && !contact.to_lowercase().contains(&query) {
                                            continue;
                                        }

                                        let is_selected = self.adb_sms_selected_contact.as_deref() == Some(contact);
                                        let last_msg = self.adb_sms_messages.iter().rev().find(|m| &m.contact_name == contact);
                                        let last_snippet = last_msg.map(|m| m.text.as_str()).unwrap_or("");
                                        let last_time = last_msg.map(|m| m.time_str.as_str()).unwrap_or("");
                                        let has_incoming = last_msg.map(|m| !m.is_outgoing).unwrap_or(false);

                                        let card_bg = if is_selected {
                                            Color32::from_rgb(30, 36, 56)
                                        } else {
                                            Color32::from_rgb(18, 20, 29)
                                        };
                                        let border_col = if is_selected {
                                            Color32::from_rgb(56, 189, 248)
                                        } else {
                                            Color32::from_rgb(28, 30, 42)
                                        };

                                        let card_resp = Frame::none()
                                            .fill(card_bg)
                                            .stroke(Stroke::new(1.0_f32, border_col))
                                            .rounding(Rounding::same(10.0))
                                            .inner_margin(egui::Margin::symmetric(10.0, 9.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    let initials: String = contact
                                                        .split_whitespace()
                                                        .filter_map(|w| w.chars().next())
                                                        .take(2)
                                                        .collect();
                                                    let avatar_initials = if initials.is_empty() { "U".to_string() } else { initials.to_uppercase() };

                                                    Frame::none()
                                                        .fill(Color32::from_rgb(79, 70, 229))
                                                        .rounding(Rounding::same(999.0))
                                                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                                        .show(ui, |ui| {
                                                            ui.label(RichText::new(&avatar_initials).size(10.0).strong().color(Color32::WHITE));
                                                        });

                                                    ui.add_space(8.0);
                                                    ui.vertical(|ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(contact).size(12.0).strong().color(p.text_primary));
                                                            if has_incoming {
                                                                ui.label(RichText::new("●").size(8.0).color(Color32::from_rgb(56, 189, 248)));
                                                            }
                                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                ui.label(RichText::new(last_time).size(9.5).color(p.text_muted));
                                                            });
                                                        });
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(last_snippet)
                                                                    .size(10.0)
                                                                    .color(p.text_secondary),
                                                            )
                                                            .truncate(),
                                                        );
                                                    });
                                                });
                                            });

                                        if card_resp.response.interact(egui::Sense::click()).clicked() {
                                            self.adb_sms_selected_contact = Some(contact.clone());
                                        }
                                        ui.add_space(4.0);
                                    }
                                });
                            });
                        });

                    ui.add_space(12.0);

                    // RIGHT COLUMN: Active Chat Thread View & Composer
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_width((right_w - 32.0).max(120.0));
                            ui.vertical(|ui| {
                            let sel_contact = self.adb_sms_selected_contact.clone().unwrap_or_default();
                            let thread_msgs: Vec<&crate::adb::AdbSmsMessage> = self
                                .adb_sms_messages
                                .iter()
                                .filter(|m| m.contact_name == sel_contact || m.address == sel_contact)
                                .collect();

                            let contact_phone = thread_msgs.first().map(|m| m.address.clone()).unwrap_or_else(|| sel_contact.clone());

                            // Chat Header Bar
                            ui.horizontal(|ui| {
                                let initials: String = sel_contact
                                    .split_whitespace()
                                    .filter_map(|w| w.chars().next())
                                    .take(2)
                                    .collect();
                                let avatar_initials = if initials.is_empty() { "U".to_string() } else { initials.to_uppercase() };

                                Frame::none()
                                    .fill(Color32::from_rgb(56, 189, 248))
                                    .rounding(Rounding::same(999.0))
                                    .inner_margin(egui::Margin::symmetric(9.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&avatar_initials).size(11.0).strong().color(Color32::from_rgb(10, 15, 25)));
                                    });

                                ui.add_space(6.0);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&sel_contact).size(14.0).strong().color(p.text_primary));
                                    ui.label(RichText::new(&contact_phone).size(10.0).monospace().color(p.text_secondary));
                                });

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn_call = egui::Button::new(RichText::new(format!("{} Call", crate::lucide::PHONE)).size(11.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(22, 163, 74))
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(75.0, 28.0));
                                    if ui.add(btn_call).on_hover_text(format!("Call {}", sel_contact)).clicked() {
                                        action_switch_to_call = Some(contact_phone.clone());
                                    }
                                });
                            });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // Chat Message Bubbles Stream
                            ScrollArea::vertical()
                                .id_salt("adb_sms_bubbles_scroll")
                                .max_height(450.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 10.0);
                                    for msg in thread_msgs {
                                        if msg.is_outgoing {
                                            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                                Frame::none()
                                                    .fill(Color32::from_rgb(79, 70, 229))
                                                    .rounding(Rounding {
                                                        nw: 14.0,
                                                        ne: 14.0,
                                                        se: 2.0,
                                                        sw: 14.0,
                                                    })
                                                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                                    .show(ui, |ui| {
                                                        ui.vertical(|ui| {
                                                            ui.label(RichText::new(&msg.text).size(11.5).color(Color32::WHITE));
                                                            ui.add_space(2.0);
                                                            ui.horizontal(|ui| {
                                                                ui.label(RichText::new(format!("{} ✓✓", msg.time_str)).size(9.0).color(Color32::from_rgb(199, 210, 254)));
                                                            });
                                                        });
                                                    });
                                            });
                                        } else {
                                            ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                                                Frame::none()
                                                    .fill(Color32::from_rgb(24, 26, 38))
                                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 60)))
                                                    .rounding(Rounding {
                                                        nw: 14.0,
                                                        ne: 14.0,
                                                        se: 14.0,
                                                        sw: 2.0,
                                                    })
                                                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                                    .show(ui, |ui| {
                                                        ui.vertical(|ui| {
                                                            ui.label(RichText::new(&msg.text).size(11.5).color(Color32::from_rgb(241, 245, 249)));
                                                            ui.add_space(2.0);
                                                            ui.label(RichText::new(&msg.time_str).size(9.0).color(p.text_muted));
                                                        });
                                                    });
                                            });
                                        }
                                    }
                                });

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);

                            // Message Composer Bar
                            ui.horizontal(|ui| {
                                let edit_resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.adb_sms_input)
                                        .hint_text("Enter a message...")
                                        .desired_width(ui.available_width() - 85.0),
                                );

                                let enter_pressed = edit_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                                let btn_send = egui::Button::new(RichText::new(format!("{} Send", crate::lucide::SEND)).size(11.0).strong().color(Color32::WHITE))
                                    .fill(Color32::from_rgb(79, 70, 229))
                                    .rounding(Rounding::same(8.0))
                                    .min_size(Vec2::new(75.0, 30.0));

                                if (ui.add(btn_send).clicked() || enter_pressed) && !self.adb_sms_input.trim().is_empty() {
                                    let text = self.adb_sms_input.trim().to_string();
                                    action_send_msg = Some((contact_phone.clone(), text));
                                    self.adb_sms_input.clear();
                                }
                            });
                            });
                        });
                });
            });

        if let Some((phone, text)) = action_send_msg {
            match crate::adb::send_sms(Some(&serial), &phone, &text) {
                Ok(_) => {
                    self.adb_sms_messages = crate::adb::get_sms_messages(Some(&serial));
                    self.adb_status_msg = Some(("SMS request opened on device".to_string(), false, Instant::now()));
                }
                Err(e) => self.adb_status_msg = Some((format!("SMS failed: {e}"), true, Instant::now())),
            }
        }

        if let Some(number) = action_switch_to_call {
            self.adb_dialer_input = number;
            self.adb_active_subtab = AdbSubTab::Calls;
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // TAB 4: PHONE LINK CALLS & KEYPAD DIALER
    // ═════════════════════════════════════════════════════════════════════════
    // TAB 4: PHONE LINK CALLS & KEYPAD DIALER
    // ═════════════════════════════════════════════════════════════════════════
    fn render_adb_calls_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        if self.adb_queries_loaded.insert("calls".to_string()) {
            self.adb_call_logs = crate::adb::get_call_logs(Some(&serial));
        }

        let mut action_call: Option<String> = None;
        let mut action_hangup = false;

        ScrollArea::vertical()
            .id_salt("adb_calls_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width();
                let left_w = 360.0_f32.min(avail_w * 0.45);
                let right_w = (avail_w - left_w - 12.0).max(0.0);
                ui.horizontal_top(|ui| {
                    // LEFT COLUMN: Recent Calls History (360px)
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width((left_w - 28.0).max(120.0));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::PHONE).size(15.0).color(Color32::from_rgb(74, 222, 128)));
                                ui.label(RichText::new("Calls & Call Log").size(15.0).strong().color(p.text_primary));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn_refresh = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(12.0).color(p.text_muted))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_refresh).on_hover_text("Refresh call log").clicked() {
                                        self.adb_call_logs = crate::adb::get_call_logs(Some(&serial));
                                    }
                                });
                            });

                            ui.add_space(8.0);

                            // Search bar
                            Frame::none()
                                .fill(Color32::from_rgb(14, 15, 22))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.adb_call_search)
                                                .hint_text("Search contacts or number...")
                                                .frame(false)
                                                .desired_width(ui.available_width()),
                                        );
                                    });
                                });

                            ui.add_space(10.0);

                            let query = self.adb_call_search.to_lowercase();

                            if self.adb_call_logs.is_empty() {
                                let clicked = render_empty_state_card(
                                    ui,
                                    &p,
                                    crate::lucide::PHONE,
                                    Color32::from_rgb(74, 222, 128),
                                    "No Recent Calls",
                                    "Call history is empty on this device.",
                                    Some("Refresh Call Log"),
                                );
                                if clicked {
                                    self.adb_call_logs = crate::adb::get_call_logs(Some(&serial));
                                }
                            } else {
                                ScrollArea::vertical()
                                    .id_salt("adb_call_log_items_scroll")
                                    .max_height(560.0)
                                    .show(ui, |ui| {
                                        for entry in &self.adb_call_logs {
                                            if !query.is_empty()
                                                && !entry.name.to_lowercase().contains(&query)
                                                && !entry.number.contains(&query)
                                            {
                                                continue;
                                            }

                                            let (type_color, type_icon) = match entry.call_type {
                                                crate::adb::AdbCallType::Missed => (Color32::from_rgb(239, 68, 68), "📞 Missed"),
                                                crate::adb::AdbCallType::Incoming => (Color32::from_rgb(34, 197, 94), "📞 Incoming"),
                                                crate::adb::AdbCallType::Outgoing => (Color32::from_rgb(6, 182, 212), "📞 Outgoing"),
                                            };

                                            Frame::none()
                                                .fill(Color32::from_rgb(18, 20, 29))
                                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 30, 42)))
                                                .rounding(Rounding::same(10.0))
                                                .inner_margin(egui::Margin::symmetric(10.0, 9.0))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        let initials: String = entry
                                                            .name
                                                            .split_whitespace()
                                                            .filter_map(|w| w.chars().next())
                                                            .take(2)
                                                            .collect();
                                                        let avatar_initials = if initials.is_empty() { "U".to_string() } else { initials.to_uppercase() };

                                                        Frame::none()
                                                            .fill(Color32::from_rgb(30, 41, 59))
                                                            .stroke(Stroke::new(1.0_f32, type_color))
                                                            .rounding(Rounding::same(999.0))
                                                            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                                            .show(ui, |ui| {
                                                                ui.label(RichText::new(&avatar_initials).size(10.0).strong().color(Color32::WHITE));
                                                            });

                                                        ui.add_space(6.0);
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.label(RichText::new(&entry.name).size(12.0).strong().color(p.text_primary));
                                                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                    ui.label(RichText::new(&entry.time_str).size(9.0).color(p.text_muted));
                                                                });
                                                            });

                                                            ui.horizontal(|ui| {
                                                                ui.label(RichText::new(type_icon).size(9.5).color(type_color));
                                                                ui.label(RichText::new(format!("• {}", entry.duration_str)).size(9.5).color(p.text_muted));
                                                            });

                                                            ui.label(RichText::new(&entry.number).size(9.5).monospace().color(p.text_secondary));
                                                        });

                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            let btn_dial = egui::Button::new(RichText::new(crate::lucide::PHONE).size(11.0).color(Color32::from_rgb(74, 222, 128)))
                                                                .fill(Color32::from_rgb(20, 45, 30))
                                                                .rounding(Rounding::same(6.0));
                                                            if ui.add(btn_dial).on_hover_text("Load into dialer and call").clicked() {
                                                                self.adb_dialer_input = entry.number.clone();
                                                                action_call = Some(entry.number.clone());
                                                            }
                                                        });
                                                    });
                                                });
                                            ui.add_space(4.0);
                                        }
                                    });
                            }
                            });
                        });

                    ui.add_space(12.0);

                    // RIGHT COLUMN: Keypad & Dialer
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(24.0, 18.0))
                        .show(ui, |ui| {
                            ui.set_width((right_w - 28.0).max(120.0));
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("KEYPAD DIALER").size(10.5).monospace().strong().color(p.text_muted));
                                ui.add_space(8.0);

                                // Monospace Number Readout Display
                                Frame::none()
                                    .fill(Color32::from_rgb(12, 13, 20))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                                    .rounding(Rounding::same(10.0))
                                    .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let display_digits = if self.adb_dialer_input.is_empty() {
                                                "Enter a phone number..."
                                            } else {
                                                &self.adb_dialer_input
                                            };
                                            let text_color = if self.adb_dialer_input.is_empty() {
                                                p.text_muted
                                            } else {
                                                Color32::WHITE
                                            };
                                            ui.label(RichText::new(display_digits).size(18.0).monospace().strong().color(text_color));

                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                if !self.adb_dialer_input.is_empty() {
                                                    let btn_bs = egui::Button::new(RichText::new("⌫").size(14.0).color(p.text_secondary))
                                                        .fill(Color32::TRANSPARENT);
                                                    if ui.add(btn_bs).on_hover_text("Backspace").clicked() {
                                                        self.adb_dialer_input.pop();
                                                    }

                                                    let btn_clear = egui::Button::new(RichText::new("✕").size(11.0).color(p.text_muted))
                                                        .fill(Color32::TRANSPARENT);
                                                    if ui.add(btn_clear).on_hover_text("Clear all").clicked() {
                                                        self.adb_dialer_input.clear();
                                                    }
                                                }
                                            });
                                        });
                                    });

                                ui.add_space(16.0);

                                // 3x4 Telephone Keypad Matrix
                                let dial_keys = [
                                    ("1", ""),
                                    ("2", "ABC"),
                                    ("3", "DEF"),
                                    ("4", "GHI"),
                                    ("5", "JKL"),
                                    ("6", "MNO"),
                                    ("7", "PQRS"),
                                    ("8", "TUV"),
                                    ("9", "WXYZ"),
                                    ("*", ""),
                                    ("0", "+"),
                                    ("#", ""),
                                ];

                                for row in dial_keys.chunks(3) {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 0.0);
                                        for &(num, letters) in row {
                                            let (rect, response) = ui.allocate_exact_size(Vec2::new(76.0, 60.0), egui::Sense::click());
                                            let fill = if response.is_pointer_button_down_on() {
                                                Color32::from_rgb(54, 58, 78)
                                            } else if response.hovered() {
                                                Color32::from_rgb(39, 43, 61)
                                            } else {
                                                Color32::from_rgb(27, 30, 43)
                                            };
                                            ui.painter().rect_filled(rect, Rounding::same(14.0), fill);
                                            ui.painter().rect_stroke(
                                                rect,
                                                Rounding::same(14.0),
                                                Stroke::new(1.0_f32, Color32::from_rgb(48, 53, 73)),
                                            );
                                            let center = rect.center();
                                            ui.painter().text(
                                                center - egui::vec2(0.0, if letters.is_empty() { 0.0 } else { 7.0 }),
                                                Align2::CENTER_CENTER,
                                                num,
                                                egui::FontId::proportional(19.0),
                                                Color32::WHITE,
                                            );
                                            if !letters.is_empty() {
                                                ui.painter().text(
                                                    center + egui::vec2(0.0, 13.0),
                                                    Align2::CENTER_CENTER,
                                                    letters,
                                                    egui::FontId::proportional(8.5),
                                                    p.text_muted,
                                                );
                                            }
                                            if response.clicked() {
                                                self.adb_dialer_input.push_str(num);
                                            }
                                        }
                                    });
                                    ui.add_space(10.0);
                                }

                                ui.add_space(14.0);

                                // Call and Hangup Action Row
                                ui.horizontal(|ui| {
                                    let btn_call = egui::Button::new(RichText::new(format!("{} Call", crate::lucide::PHONE)).size(13.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(22, 163, 74))
                                        .rounding(Rounding::same(999.0))
                                        .min_size(Vec2::new(130.0, 38.0));
                                    if ui.add(btn_call).on_hover_text("Initiate call via Android device").clicked() {
                                        if !self.adb_dialer_input.is_empty() {
                                            action_call = Some(self.adb_dialer_input.clone());
                                        }
                                    }

                                    ui.add_space(12.0);

                                    let btn_hangup = egui::Button::new(RichText::new("☎ End Call").size(13.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(220, 38, 38))
                                        .rounding(Rounding::same(999.0))
                                        .min_size(Vec2::new(110.0, 38.0));
                                    if ui.add(btn_hangup).on_hover_text("End active call (KEYCODE_ENDCALL)").clicked() {
                                        action_hangup = true;
                                    }
                                });

                                ui.add_space(14.0);

                                // Speed Dial Presets
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Presets:").size(10.0).color(p.text_muted));
                                    for &(label, number) in &[("Emergency (911)", "911"), ("Voicemail", "*86"), ("Service", "611")] {
                                        let btn = egui::Button::new(RichText::new(label).size(9.5).color(p.text_secondary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(6.0));
                                        if ui.add(btn).clicked() {
                                            self.adb_quick_call_number = number.to_string();
                                            self.adb_dialer_input = number.to_string();
                                        }
                                    }
                                });
                            });
                        });
                });
            });

        if let Some(num) = action_call {
            match crate::adb::start_phone_call(Some(&serial), &num) {
                Ok(_) => {
                    self.adb_status_msg = Some((format!("Calling {}...", num), false, Instant::now()));
                    self.adb_call_logs = crate::adb::get_call_logs(Some(&serial));
                }
                Err(e) => {
                    self.adb_status_msg = Some((format!("Call failed: {}", e), true, Instant::now()));
                }
            }
        }

        if action_hangup {
            let _ = crate::adb::end_phone_call(Some(&serial));
            self.adb_status_msg = Some(("Call ended".to_string(), false, Instant::now()));
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // TAB 5: PHONE LINK PHOTOS GALLERY & LIGHTBOX
    // ═════════════════════════════════════════════════════════════════════════
    fn render_adb_photos_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        if self.adb_queries_loaded.insert("photos".to_string()) {
            self.adb_photos = crate::adb::list_photos(Some(&serial));
        }

        let mut action_download: Option<String> = None;
        let mut action_delete: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("adb_photos_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Header Bar
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::IMAGE).size(16.0).color(Color32::from_rgb(244, 114, 182)));
                            ui.label(RichText::new("Photos & Camera Roll").size(15.0).strong().color(p.text_primary));
                            ui.label(RichText::new(format!("({} photos)", self.adb_photos.len())).size(11.0).color(p.text_muted));

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let btn_refresh = egui::Button::new(RichText::new(format!("{} Refresh Gallery", crate::lucide::REFRESH_CW)).size(11.0).color(p.text_secondary))
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_refresh).clicked() {
                                    self.adb_photos = crate::adb::list_photos(Some(&serial));
                                }
                            });
                        });
                    });

                ui.add_space(12.0);

                if self.adb_photos.is_empty() {
                    let clicked = render_empty_state_card(
                        ui,
                        &p,
                        crate::lucide::IMAGE,
                        Color32::from_rgb(244, 114, 182),
                        "No Photos Found",
                        "No camera photos or images found on device storage.",
                        Some("Refresh Gallery"),
                    );
                    if clicked {
                        self.adb_photos = crate::adb::list_photos(Some(&serial));
                    }
                } else {
                    let num_cols = 4;
                    let card_w = ((ui.available_width() - ((num_cols - 1) as f32 * 12.0)) / num_cols as f32).max(100.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                        for photo in &self.adb_photos {
                            let photo_clone = photo.clone();
                            Frame::none()
                                .fill(Color32::from_rgb(18, 20, 29))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 10.0))
                                .show(ui, |ui| {
                                    ui.set_width(card_w);
                                    ui.vertical(|ui| {
                                        // Visual Photo Canvas Placeholder
                                        let canvas_resp = Frame::none()
                                            .fill(Color32::from_rgb(24, 28, 42))
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 28.0))
                                            .show(ui, |ui| {
                                                ui.vertical_centered(|ui| {
                                                    ui.label(RichText::new(crate::lucide::IMAGE).size(32.0).color(Color32::from_rgb(244, 114, 182)));
                                                    ui.add_space(4.0);
                                                    ui.label(RichText::new("HIGH RES JPEG").size(9.0).monospace().color(p.text_muted));
                                                });
                                            });

                                        if canvas_resp.response.interact(egui::Sense::click()).clicked() {
                                            self.adb_selected_photo = Some(photo_clone.clone());
                                        }

                                        ui.add_space(8.0);
                                        ui.add(egui::Label::new(RichText::new(&photo.name).size(11.0).strong().color(p.text_primary)).truncate());

                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&photo.size_str).size(9.5).monospace().color(Color32::from_rgb(56, 189, 248)));
                                            ui.label(RichText::new("•").size(9.5).color(p.text_muted));
                                            ui.add(egui::Label::new(RichText::new(&photo.date_str).size(9.0).color(p.text_muted)).truncate());
                                        });

                                        ui.add_space(6.0);

                                        // Action buttons row
                                        ui.horizontal(|ui| {
                                            let btn_view = egui::Button::new(RichText::new("👁 View").size(10.0).color(p.text_primary))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(6.0));
                                            if ui.add(btn_view).clicked() {
                                                self.adb_selected_photo = Some(photo_clone.clone());
                                            }

                                            let btn_dl = egui::Button::new(RichText::new("💾 Save").size(10.0).color(Color32::from_rgb(74, 222, 128)))
                                                .fill(Color32::from_rgb(20, 45, 30))
                                                .rounding(Rounding::same(6.0));
                                            if ui.add(btn_dl).on_hover_text("Download to host ~/Downloads").clicked() {
                                                action_download = Some(photo_clone.path.clone());
                                            }

                                            let btn_del = egui::Button::new(RichText::new("🗑").size(10.0).color(Color32::from_rgb(248, 113, 113)))
                                                .fill(Color32::from_rgb(45, 20, 20))
                                                .rounding(Rounding::same(6.0));
                                            if ui.add(btn_del).on_hover_text("Delete from phone").clicked() {
                                                action_delete = Some(photo_clone.path.clone());
                                            }
                                        });
                                    });
                                });
                        }
                    });
                }
            });

        if let Some(path) = action_download {
            match crate::adb::pull_photo_to_downloads(Some(&serial), &path) {
                Ok(msg) => self.adb_status_msg = Some((msg, false, Instant::now())),
                Err(e) => self.adb_status_msg = Some((format!("Download failed: {}", e), true, Instant::now())),
            }
        }

        if let Some(path) = action_delete {
            match crate::adb::delete_device_photo(Some(&serial), &path) {
                Ok(_) => {
                    self.adb_photos.retain(|p| p.path != path);
                    self.adb_status_msg = Some(("Photo deleted from device".to_string(), false, Instant::now()));
                }
                Err(e) => self.adb_status_msg = Some((format!("Photo delete failed: {e}"), true, Instant::now())),
            }
        }
    }

    fn render_adb_photo_lightbox(&mut self, ctx: &egui::Context, serial: &str) {
        let photo = match &self.adb_selected_photo {
            Some(p) => p.clone(),
            None => return,
        };

        if self.adb_photo_preview.as_ref().map(|(path, _)| path.as_str()) != Some(photo.path.as_str()) {
            match crate::adb::read_remote_file(serial, &photo.path)
                .and_then(|bytes| image::load_from_memory(&bytes).map_err(|e| format!("Invalid image: {e}")))
            {
                Ok(image) => {
                    let rgba = image.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    self.adb_photo_preview = Some((photo.path.clone(), Ok(ctx.load_texture(
                        format!("adb_photo_{}", photo.name), color_image, egui::TextureOptions::LINEAR,
                    ))));
                }
                Err(e) => {
                    self.adb_photo_preview = Some((photo.path.clone(), Err(e.clone())));
                    self.adb_status_msg = Some((format!("Photo preview failed: {e}"), true, Instant::now()));
                }
            }
        }

        let screen = ctx.screen_rect();
        let p = self.palette();

        let backdrop_area = egui::Area::new(egui::Id::new("adb_lightbox_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle);
        let bg_resp = backdrop_area
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(190));
                resp
            })
            .inner;

        if bg_resp.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.adb_selected_photo = None;
            return;
        }

        let modal_w = 640.0_f32.min(screen.width() - 32.0);
        let mut close_modal = false;
        let mut do_download: Option<String> = None;

        egui::Window::new("Photo Preview Lightbox")
            .id(egui::Id::new("adb_photo_lightbox_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center())
            .pivot(Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(16, 17, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 14.0),
                        blur: 32.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(180),
                    })
                    .inner_margin(egui::Margin::symmetric(20.0, 16.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::lucide::IMAGE).size(16.0).color(Color32::from_rgb(244, 114, 182)));
                    ui.label(RichText::new(&photo.name).size(14.0).strong().color(p.text_primary));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn_x = egui::Button::new(RichText::new(crate::lucide::X).size(13.0).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_x).clicked() {
                            close_modal = true;
                        }
                    });
                });

                ui.add_space(12.0);

                Frame::none()
                    .fill(Color32::from_rgb(10, 12, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 32, 46)))
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(24.0, 48.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            if let Some((_, Ok(texture))) = &self.adb_photo_preview {
                                let source = texture.size_vec2();
                                let max_size = Vec2::new(ui.available_width().min(560.0), 440.0);
                                let fit = (max_size.x / source.x).min(max_size.y / source.y).min(1.0);
                                ui.image((texture.id(), source * fit));
                            } else if self.adb_photo_preview.as_ref().is_some_and(|(_, result)| result.is_err()) {
                                ui.label(RichText::new("Could not load photo preview").size(11.0).color(p.danger));
                            } else {
                                ui.label(RichText::new(crate::lucide::IMAGE).size(48.0).color(Color32::from_rgb(244, 114, 182)));
                                ui.label(RichText::new("Loading photo preview…").size(11.0).color(p.text_muted));
                            }
                            ui.add_space(8.0);
                            ui.label(RichText::new(format!("{} • {} • {}", photo.name, photo.size_str, photo.date_str)).size(10.5).color(Color32::from_rgb(56, 189, 248)));
                        });
                    });

                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    let btn_dl = egui::Button::new(RichText::new(format!("{} Download to Host PC", crate::lucide::DOWNLOAD)).size(11.5).strong().color(Color32::WHITE))
                        .fill(Color32::from_rgb(22, 163, 74))
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(160.0, 32.0));
                    if ui.add(btn_dl).clicked() {
                        do_download = Some(photo.path.clone());
                    }

                    ui.add_space(8.0);

                    let btn_copy = egui::Button::new(RichText::new("📋 Copy Path").size(11.0).color(p.text_secondary))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(8.0));
                    if ui.add(btn_copy).clicked() {
                        ui.output_mut(|o| o.copied_text = photo.path.clone());
                        self.adb_status_msg = Some(("Path copied to clipboard".to_string(), false, Instant::now()));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn_close = egui::Button::new(RichText::new("Close").size(11.0).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(8.0));
                        if ui.add(btn_close).clicked() {
                            close_modal = true;
                        }
                    });
                });
            });

        if close_modal {
            self.adb_selected_photo = None;
        }

        if let Some(path) = do_download {
            match crate::adb::pull_photo_to_downloads(Some(serial), &path) {
                Ok(msg) => self.adb_status_msg = Some((msg, false, Instant::now())),
                Err(e) => self.adb_status_msg = Some((format!("Download failed: {}", e), true, Instant::now())),
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // TAB 6: PHONE LINK NOTIFICATIONS CENTER
    // ═════════════════════════════════════════════════════════════════════════
    fn render_adb_notifications_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        if self.adb_queries_loaded.insert("notifications".to_string()) {
            self.adb_notifications = crate::adb::get_notifications(Some(&serial));
        }

        let mut action_dismiss: Option<String> = None;
        let mut action_reply: Option<String> = None;
        let mut action_call: Option<String> = None;

        ScrollArea::vertical()
            .id_salt("adb_notifications_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Header Bar
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::BELL).size(16.0).color(Color32::from_rgb(251, 191, 36)));
                            ui.label(RichText::new("Live Notifications").size(15.0).strong().color(p.text_primary));
                            ui.label(RichText::new(format!("({} active)", self.adb_notifications.len())).size(11.0).color(p.text_muted));

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let btn_clear = egui::Button::new(RichText::new("Clear All").size(10.5).color(Color32::from_rgb(248, 113, 113)))
                                    .fill(Color32::from_rgb(45, 20, 20))
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_clear).clicked() {
                                    self.adb_notifications.clear();
                                }

                                ui.add_space(8.0);

                                let btn_refresh = egui::Button::new(RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW)).size(10.5).color(p.text_secondary))
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0));
                                if ui.add(btn_refresh).clicked() {
                                    self.adb_notifications = crate::adb::get_notifications(Some(&serial));
                                }
                            });
                        });
                    });

                ui.add_space(8.0);

                // Notifications Search Filter
                Frame::none()
                    .fill(Color32::from_rgb(14, 15, 22))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.adb_notifications_filter)
                                    .hint_text("Search notifications by app, title, or message...")
                                    .frame(false)
                                    .desired_width(ui.available_width()),
                            );
                        });
                    });

                ui.add_space(10.0);

                let notif_q = self.adb_notifications_filter.to_lowercase();

                // Notifications List
                for notif in &self.adb_notifications {
                    if !notif_q.is_empty()
                        && !notif.app_name.to_lowercase().contains(&notif_q)
                        && !notif.title.to_lowercase().contains(&notif_q)
                        && !notif.text.to_lowercase().contains(&notif_q)
                    {
                        continue;
                    }
                    let notif_clone = notif.clone();
                    let (app_icon, app_color) = match notif.package.as_str() {
                        pkg if pkg.contains("outlook") || pkg.contains("email") => (crate::lucide::FILE_TEXT, Color32::from_rgb(56, 189, 248)),
                        pkg if pkg.contains("messaging") || pkg.contains("sms") => (crate::lucide::MESSAGE_SQUARE, Color32::from_rgb(99, 102, 241)),
                        pkg if pkg.contains("dialer") || pkg.contains("phone") => (crate::lucide::PHONE, Color32::from_rgb(34, 197, 94)),
                        _ => (crate::lucide::BELL, Color32::from_rgb(251, 191, 36)),
                    };

                    Frame::none()
                        .fill(Color32::from_rgb(18, 20, 29))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                Frame::none()
                                    .fill(app_color.gamma_multiply(0.2))
                                    .rounding(Rounding::same(8.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(app_icon).size(16.0).color(app_color));
                                    });

                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&notif.app_name).size(10.5).monospace().color(app_color));
                                        ui.label(RichText::new("•").size(9.5).color(p.text_muted));
                                        ui.label(RichText::new(&notif.time_str).size(9.5).color(p.text_muted));
                                    });
                                    ui.label(RichText::new(&notif.title).size(13.0).strong().color(p.text_primary));
                                    ui.label(RichText::new(&notif.text).size(11.0).color(p.text_secondary));
                                });

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let btn_dismiss = egui::Button::new(RichText::new("Dismiss").size(10.0).color(p.text_muted))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(6.0));
                                    if ui.add(btn_dismiss).clicked() {
                                        action_dismiss = Some(notif_clone.id.clone());
                                    }

                                    ui.add_space(6.0);

                                    if notif.package.contains("dialer") || notif.title.contains("Call") {
                                        let btn_call = egui::Button::new(RichText::new("📞 Call Back").size(10.0).strong().color(Color32::WHITE))
                                            .fill(Color32::from_rgb(22, 163, 74))
                                            .rounding(Rounding::same(6.0));
                                        if ui.add(btn_call).clicked() {
                                            action_call = Some(notif_clone.title.clone());
                                        }
                                    } else {
                                        let btn_reply = egui::Button::new(RichText::new("💬 Reply").size(10.0).strong().color(Color32::WHITE))
                                            .fill(Color32::from_rgb(79, 70, 229))
                                            .rounding(Rounding::same(6.0));
                                        if ui.add(btn_reply).clicked() {
                                            action_reply = Some(notif_clone.title.clone());
                                        }
                                    }
                                });
                            });
                        });
                    ui.add_space(8.0);
                }
            });

        if let Some(id) = action_dismiss {
            self.adb_notifications.retain(|n| n.id != id);
        }

        if let Some(sender) = action_reply {
            self.adb_sms_selected_contact = Some(sender);
            self.adb_active_subtab = AdbSubTab::Messages;
        }

        if let Some(num) = action_call {
            self.adb_dialer_input = num;
            self.adb_active_subtab = AdbSubTab::Calls;
        }
    }
}

/// Renders a Nocturnal Command empty state card with icon badge, headline, subtitle, and action button.
#[allow(dead_code)]
fn render_empty_state_card(
    ui: &mut Ui,
    p: &crate::theme::ThemePalette,
    icon: &'static str,
    icon_color: Color32,
    headline: &str,
    subtitle: &str,
    button_label: Option<&str>,
) -> bool {
    let mut clicked = false;
    Frame::none()
        .fill(Color32::from_rgb(15, 17, 24))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 34, 48)))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(24.0, 32.0))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                Frame::none()
                    .fill(icon_color.gamma_multiply(0.12))
                    .stroke(Stroke::new(1.0_f32, icon_color.gamma_multiply(0.35)))
                    .rounding(Rounding::same(999.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(icon).size(26.0).color(icon_color));
                    });

                ui.add_space(10.0);
                ui.label(RichText::new(headline).size(14.0).strong().color(p.text_primary));
                ui.add_space(4.0);
                ui.label(RichText::new(subtitle).size(10.5).color(p.text_secondary));

                if let Some(lbl) = button_label {
                    ui.add_space(14.0);
                    let btn = egui::Button::new(RichText::new(lbl).size(10.5).strong().color(p.accent))
                        .fill(p.bg_button)
                        .stroke(Stroke::new(1.0_f32, p.accent.gamma_multiply(0.35)))
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(110.0, 26.0));
                    if ui.add(btn).clicked() {
                        clicked = true;
                    }
                }
            });
        });
    clicked
}
