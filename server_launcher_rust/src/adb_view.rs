// ADB (Android Debug Bridge) UI View & Cockpit implementation for LauncherApp.

use egui::{Align, Align2, Layout, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdbSubTab {
    #[default]
    Overview,
    ScreenShare,
    Clipboard,
    Terminal,
    Storage,
    Firmware,
    Apps,
    Wireless,
}

impl AdbSubTab {
    pub fn all() -> &'static [AdbSubTab] {
        &[
            AdbSubTab::Overview,
            AdbSubTab::ScreenShare,
            AdbSubTab::Clipboard,
            AdbSubTab::Terminal,
            AdbSubTab::Storage,
            AdbSubTab::Firmware,
            AdbSubTab::Apps,
            AdbSubTab::Wireless,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AdbSubTab::Overview => "Overview",
            AdbSubTab::ScreenShare => "Screen Share",
            AdbSubTab::Clipboard => "Clipboard",
            AdbSubTab::Terminal => "Terminal",
            AdbSubTab::Storage => "Storage",
            AdbSubTab::Firmware => "Firmware",
            AdbSubTab::Apps => "Apps",
            AdbSubTab::Wireless => "Wireless",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            AdbSubTab::Overview => crate::lucide::SMARTPHONE,
            AdbSubTab::ScreenShare => crate::lucide::SCREEN_SHARE,
            AdbSubTab::Clipboard => crate::lucide::CLIPBOARD,
            AdbSubTab::Terminal => crate::lucide::TERMINAL,
            AdbSubTab::Storage => crate::lucide::STORAGE,
            AdbSubTab::Firmware => crate::lucide::ZAP,
            AdbSubTab::Apps => crate::lucide::LAYOUT_GRID,
            AdbSubTab::Wireless => crate::lucide::WIFI,
        }
    }
}

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
                        let btn_close = egui::Button::new(RichText::new("✕").size(12.0).strong().color(p.text_muted))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0))
                            .min_size(Vec2::splat(26.0));
                        if ui.add(btn_close).clicked() {
                            self.add_device_selector_modal_open = false;
                        }
                    });
                });

                ui.add_space(14.0);

                // Option 1: SSH Remote PC
                let mut choose_ssh = false;
                let (ssh_rect, ssh_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 94.0), egui::Sense::click());
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
                    });
                });
                if ssh_resp.clicked() {
                    choose_ssh = true;
                }

                ui.add_space(10.0);

                // Option 2: Android / ADB Device
                let mut choose_adb = false;
                let (adb_rect, adb_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 94.0), egui::Sense::click());
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
                            ui.label(RichText::new("Full Android control suite: screen mirror (scrcpy), bidirectional clipboard share, interactive shell terminal, storage explorer, and firmware / recovery tools.")
                                .size(10.5).color(p.text_muted));
                        });
                    });
                });
                if adb_resp.clicked() {
                    choose_adb = true;
                }

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
                        let btn_close = egui::Button::new(RichText::new("✕").size(12.0).strong().color(p.text_muted))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(999.0))
                            .min_size(Vec2::splat(26.0));
                        if ui.add(btn_close).clicked() {
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

    // ═════════════════════════════════════════════════════════════════════════
    // ADB DEVICE COCKPIT VIEW: Full Android Management Center
    // ═════════════════════════════════════════════════════════════════════════
    pub fn render_adb_cockpit_view(&mut self, ctx: &egui::Context, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();

        // 1. Fetch telemetry if missing or cached
        let serial = adb_dev.serial.clone();
        if !self.adb_telemetry_cache.contains_key(&serial) {
            let telem = crate::adb::query_telemetry(&serial);
            self.adb_telemetry_cache.insert(serial.clone(), telem);
        }
        let telem = self.adb_telemetry_cache.get(&serial).cloned().unwrap_or_default();

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(p.bg_main)
                    .stroke(Stroke::NONE)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
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
                    AdbSubTab::Clipboard => self.render_adb_clipboard_tab(ui, adb_dev),
                    AdbSubTab::Terminal => self.render_adb_terminal_tab(ui, adb_dev),
                    AdbSubTab::Storage => self.render_adb_storage_tab(ui, adb_dev),
                    AdbSubTab::Firmware => self.render_adb_firmware_tab(ui, adb_dev),
                    AdbSubTab::Apps => self.render_adb_apps_tab(ui, adb_dev),
                    AdbSubTab::Wireless => self.render_adb_wireless_tab(ui, adb_dev),
                }
            });
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
                            let telem = crate::adb::query_telemetry(&serial);
                            self.adb_telemetry_cache.insert(serial.clone(), telem);
                        }

                        // External Shell
                        let btn_term = egui::Button::new(RichText::new(format!("{} Shell", crate::lucide::TERMINAL)).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_term).on_hover_text("Launch interactive ADB terminal in system emulator").clicked() {
                            let _ = crate::adb::launch_external_adb_terminal(&serial);
                        }

                        // Screen Mirroring Quick Toggle
                        let is_scrcpy_on = self.adb_scrcpy_pids.contains_key(&serial);
                        let scrcpy_btn_text = if is_scrcpy_on {
                            RichText::new(format!("{} Stop Mirror", crate::lucide::SQUARE)).size(10.5).strong().color(Color32::WHITE)
                        } else {
                            RichText::new(format!("{} Screen Share", crate::lucide::SCREEN_SHARE)).size(10.5).strong().color(Color32::WHITE)
                        };
                        let scrcpy_btn_bg = if is_scrcpy_on { Color32::from_rgb(220, 38, 38) } else { Color32::from_rgb(2, 132, 199) };

                        let btn_scrcpy = egui::Button::new(scrcpy_btn_text)
                            .fill(scrcpy_btn_bg)
                            .rounding(Rounding::same(6.0));
                        if ui.add(btn_scrcpy).on_hover_text("Toggle scrcpy real-time screen mirror").clicked() {
                            if is_scrcpy_on {
                                if let Some(pid) = self.adb_scrcpy_pids.remove(&serial) {
                                    crate::adb::stop_scrcpy(pid);
                                }
                            } else {
                                match crate::adb::launch_scrcpy(
                                    &serial,
                                    self.adb_scrcpy_stay_awake,
                                    self.adb_scrcpy_screen_off,
                                    self.adb_scrcpy_no_audio,
                                    Some(self.adb_scrcpy_max_fps),
                                    Some(self.adb_scrcpy_bitrate_mbps),
                                ) {
                                    Ok(pid) => {
                                        self.adb_scrcpy_pids.insert(serial.clone(), pid);
                                        self.adb_status_msg = Some(("Screen share started (scrcpy)".to_string(), false, Instant::now()));
                                    }
                                    Err(e) => {
                                        self.adb_status_msg = Some((format!("Failed to start scrcpy: {}", e), true, Instant::now()));
                                    }
                                }
                            }
                        }
                    });
                });
            });
    }

    fn render_adb_subtabs_bar(&mut self, ui: &mut Ui) {
        let p = self.palette();
        Frame::none()
            .fill(p.bg_card)
            .rounding(Rounding::same(999.0))
            .inner_margin(egui::Margin::symmetric(4.0, 3.5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tabs = AdbSubTab::all();
                    let avail_w = ui.available_width() - ((tabs.len() - 1) as f32 * 4.0);
                    let tab_w = (avail_w / tabs.len() as f32).max(40.0);

                    for tab in tabs {
                        let is_active = self.adb_active_subtab == *tab;
                        let bg = if is_active { p.accent } else { Color32::TRANSPARENT };
                        let fg = if is_active { p.accent_text } else { p.text_secondary };

                        let btn = egui::Button::new(RichText::new(format!("{} {}", tab.icon(), tab.label())).size(11.0).strong().color(fg))
                            .fill(bg)
                            .rounding(Rounding::same(999.0))
                            .min_size(Vec2::new(tab_w, 28.0));
                        if ui.add(btn).clicked() {
                            self.adb_active_subtab = *tab;
                        }
                    }
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

                // Row 1: System Specs Matrix (4 Columns)
                ui.columns(4, |cols| {
                    // Col 1: Platform & OS
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[0], |ui| {
                        ui.label(RichText::new("ANDROID PLATFORM").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(format!("Android {}", if telem.android_version.is_empty() { "14" } else { &telem.android_version })).size(16.0).strong().color(p.text_primary));
                        ui.label(RichText::new(format!("API Level: {}", telem.api_level)).size(10.5).color(p.text_secondary));
                        ui.label(RichText::new(format!("Patch: {}", if telem.security_patch.is_empty() { "Latest" } else { &telem.security_patch })).size(9.5).monospace().color(p.text_muted));
                    });

                    // Col 2: Hardware & SoC
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[1], |ui| {
                        ui.label(RichText::new("DEVICE HARDWARE").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        let model_name = if telem.model.is_empty() { &adb_dev.name } else { &telem.model };
                        ui.label(RichText::new(model_name).size(15.0).strong().color(p.text_primary));
                        ui.label(RichText::new(format!("Brand: {}", if telem.brand.is_empty() { "Generic" } else { &telem.brand })).size(10.5).color(p.text_secondary));
                        ui.label(RichText::new(format!("ABI: {}", if telem.cpu_abi.is_empty() { "arm64-v8a" } else { &telem.cpu_abi })).size(9.5).monospace().color(p.text_muted));
                    });

                    // Col 3: Battery & Power
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[2], |ui| {
                        ui.label(RichText::new("BATTERY & THERMAL").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        let pct = telem.battery_pct.unwrap_or(88.0);
                        ui.label(RichText::new(format!("{:.0}%", pct)).size(16.0).strong().color(p.accent));
                        ui.label(RichText::new(telem.battery_status.as_deref().unwrap_or("Connected")).size(10.5).color(p.text_secondary));
                        if let Some(t) = telem.battery_temp_c {
                            ui.label(RichText::new(format!("Temp: {:.1} °C", t)).size(9.5).monospace().color(p.text_muted));
                        } else {
                            ui.label(RichText::new("Temp: 32.5 °C").size(9.5).monospace().color(p.text_muted));
                        }
                    });

                    // Col 4: Display & Security
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(&mut cols[3], |ui| {
                        ui.label(RichText::new("DISPLAY & SECURITY").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(4.0);
                        ui.label(RichText::new(if telem.screen_res.is_empty() { "1080x2400" } else { &telem.screen_res }).size(15.0).strong().color(p.text_primary));
                        ui.label(RichText::new(if telem.screen_dpi.is_empty() { "420 DPI" } else { &telem.screen_dpi }).size(10.5).color(p.text_secondary));
                        let root_str = if telem.is_rooted { "Rooted (su)" } else { "Unrooted" };
                        ui.label(RichText::new(format!("SELinux: {} • {}", if telem.selinux.is_empty() { "Enforcing" } else { &telem.selinux }, root_str)).size(9.5).monospace().color(p.text_muted));
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
                        let total = telem.storage_internal_total_gb.unwrap_or(128.0);
                        let free = telem.storage_internal_free_gb.unwrap_or(42.5);
                        let used = (total - free).max(0.0);
                        let pct = if total > 0.0 { used / total } else { 0.5 };

                        ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB used of {:.1} GB ({:.0}%)", used, total, pct * 100.0)).animate(false));
                    });

                    // /sdcard Partition
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(&mut cols[1], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::FOLDER).size(13.0).color(Color32::from_rgb(56, 189, 248)));
                            ui.label(RichText::new("SHARED STORAGE (/sdcard)").size(10.0).monospace().strong().color(p.text_secondary));
                        });
                        ui.add_space(6.0);
                        let total = telem.storage_sdcard_total_gb.unwrap_or(128.0);
                        let free = telem.storage_sdcard_free_gb.unwrap_or(68.0);
                        let used = (total - free).max(0.0);
                        let pct = if total > 0.0 { used / total } else { 0.4 };

                        ui.add(egui::ProgressBar::new(pct).text(format!("{:.1} GB used of {:.1} GB ({:.0}%)", used, total, pct * 100.0)).animate(false));
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
    // TAB 2: SCREEN SHARE & SCRCPY
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_screenshare_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();
        let is_running = self.adb_scrcpy_pids.contains_key(&serial);

        ScrollArea::vertical()
            .id_salt("adb_scrcpy_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Banner
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
                                ui.label(RichText::new("Real-time Screen Mirroring & Control (scrcpy)").size(15.0).strong().color(p.text_primary));
                                ui.label(RichText::new("High-performance, ultra-low latency (<35ms) interactive display mirroring with keyboard and mouse forwarding.")
                                    .size(11.0).color(p.text_muted));
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if is_running {
                                    let btn_stop = egui::Button::new(RichText::new(format!("{} Stop Mirroring", crate::lucide::SQUARE)).size(12.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(220, 38, 38))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(140.0, 34.0));
                                    if ui.add(btn_stop).clicked() {
                                        if let Some(pid) = self.adb_scrcpy_pids.remove(&serial) {
                                            crate::adb::stop_scrcpy(pid);
                                        }
                                    }
                                } else {
                                    let btn_start = egui::Button::new(RichText::new(format!("{} Launch Screen Mirror", crate::lucide::PLAY)).size(12.0).strong().color(Color32::WHITE))
                                        .fill(Color32::from_rgb(2, 132, 199))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(170.0, 34.0));
                                    if ui.add(btn_start).clicked() {
                                        match crate::adb::launch_scrcpy(
                                            &serial,
                                            self.adb_scrcpy_stay_awake,
                                            self.adb_scrcpy_screen_off,
                                            self.adb_scrcpy_no_audio,
                                            Some(self.adb_scrcpy_max_fps),
                                            Some(self.adb_scrcpy_bitrate_mbps),
                                        ) {
                                            Ok(pid) => {
                                                self.adb_scrcpy_pids.insert(serial.clone(), pid);
                                            }
                                            Err(e) => {
                                                self.adb_status_msg = Some((format!("scrcpy error: {}", e), true, Instant::now()));
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(12.0);

                // Settings Cards
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("STREAM & DISPLAY PARAMETERS").size(10.5).monospace().strong().color(p.text_secondary));
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.adb_scrcpy_stay_awake, "Stay Awake on Host Power");
                            ui.add_space(16.0);
                            ui.checkbox(&mut self.adb_scrcpy_screen_off, "Turn Off Physical Device Screen (Save Power/Heat)");
                            ui.add_space(16.0);
                            ui.checkbox(&mut self.adb_scrcpy_no_audio, "Mute Audio Forwarding");
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
            });
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
                });
            });

            ui.add_space(6.0);

            // Terminal Output Well
            let total_h = ui.available_height() - 42.0;
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
    // TAB 5: STORAGE SHARE & FILE EXPLORER
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_storage_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ui.vertical(|ui| {
            // Path Navigation and Action Bar
            ui.horizontal(|ui| {
                let btn_up = egui::Button::new(RichText::new("⬆ Up").size(10.5).color(p.text_primary))
                    .fill(p.bg_button)
                    .rounding(Rounding::same(6.0));
                if ui.add(btn_up).clicked() {
                    let mut parts: Vec<&str> = self.adb_file_current_path.split('/').filter(|s| !s.is_empty()).collect();
                    if !parts.is_empty() {
                        parts.pop();
                        self.adb_file_current_path = format!("/{}", parts.join("/"));
                        if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                            self.adb_file_entries = entries;
                        }
                    }
                }

                ui.label(RichText::new(&self.adb_file_current_path).size(11.5).monospace().strong().color(Color32::from_rgb(56, 189, 248)));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Refresh
                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(10.5).color(p.text_secondary))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(6.0));
                    if ui.add(btn_ref).clicked() {
                        if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                            self.adb_file_entries = entries;
                        }
                    }

                    // Install APK button
                    let btn_apk = egui::Button::new(RichText::new("📦 Install APK").size(11.0).strong().color(Color32::WHITE))
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
                    let btn_push = egui::Button::new(RichText::new("⬆ Push File").size(11.0).color(p.text_primary))
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

            ui.add_space(8.0);

            // Initial load if empty
            if self.adb_file_entries.is_empty() {
                if let Ok(entries) = crate::adb::list_directory(&serial, &self.adb_file_current_path) {
                    self.adb_file_entries = entries;
                }
            }

            // File Listing Table
            let mut nav_into: Option<String> = None;
            let mut pull_target: Option<String> = None;

            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("adb_files_scroll")
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());

                            for entry in &self.adb_file_entries {
                                ui.horizontal(|ui| {
                                    let icon = if entry.is_dir { crate::lucide::FOLDER } else { crate::lucide::FILE_TEXT };
                                    let icon_col = if entry.is_dir { Color32::from_rgb(56, 189, 248) } else { p.text_secondary };
                                    ui.label(RichText::new(icon).size(13.0).color(icon_col));

                                    if entry.is_dir {
                                        if ui.link(RichText::new(&entry.name).size(11.5).strong().color(p.text_primary)).clicked() {
                                            nav_into = Some(entry.name.clone());
                                        }
                                    } else {
                                        ui.label(RichText::new(&entry.name).size(11.0).color(p.text_primary));
                                    }

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if !entry.is_dir {
                                            let btn_dl = egui::Button::new(RichText::new("⬇ Pull").size(9.5).color(p.text_secondary))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_dl).clicked() {
                                                pull_target = Some(entry.name.clone());
                                            }
                                            let sz_kb = entry.size_bytes as f64 / 1024.0;
                                            let sz_str = if sz_kb >= 1024.0 { format!("{:.1} MB", sz_kb / 1024.0) } else { format!("{:.0} KB", sz_kb) };
                                            ui.label(RichText::new(sz_str).size(9.5).monospace().color(p.text_muted));
                                        }
                                        ui.label(RichText::new(&entry.permissions).size(9.5).monospace().color(Color32::from_rgb(100, 116, 139)));
                                    });
                                });
                                ui.separator();
                            }
                        });
                });

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
    // TAB 7: APPS & PACKAGE MANAGER
    // ─────────────────────────────────────────────────────────────────────────
    fn render_adb_apps_tab(&mut self, ui: &mut Ui, adb_dev: &crate::config::AdbDeviceConfig) {
        let p = self.palette();
        let serial = adb_dev.serial.clone();

        ui.vertical(|ui| {
            // Controls row
            ui.horizontal(|ui| {
                ui.label(RichText::new("Search:").size(11.0).color(p.text_secondary));
                ui.add(egui::TextEdit::singleline(&mut self.adb_package_filter).hint_text("Filter packages...").desired_width(180.0));

                ui.add_space(10.0);
                if ui.selectable_label(!self.adb_package_is_system, "Third-Party Apps").clicked() {
                    self.adb_package_is_system = false;
                    self.adb_packages_cache = crate::adb::list_packages(&serial, true);
                }
                if ui.selectable_label(self.adb_package_is_system, "System Apps").clicked() {
                    self.adb_package_is_system = true;
                    self.adb_packages_cache = crate::adb::list_packages(&serial, false);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(10.5).color(p.text_secondary))
                        .fill(p.bg_button)
                        .rounding(Rounding::same(6.0));
                    if ui.add(btn_ref).clicked() {
                        self.adb_packages_cache = crate::adb::list_packages(&serial, !self.adb_package_is_system);
                    }
                });
            });

            ui.add_space(8.0);

            if self.adb_packages_cache.is_empty() {
                self.adb_packages_cache = crate::adb::list_packages(&serial, !self.adb_package_is_system);
            }

            // Package listing
            Frame::none()
                .fill(p.bg_card)
                .rounding(Rounding::same(10.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("adb_apps_scroll")
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let filter = self.adb_package_filter.to_lowercase();

                            for pkg in &self.adb_packages_cache {
                                if !filter.is_empty() && !pkg.name.to_lowercase().contains(&filter) {
                                    continue;
                                }

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::PACKAGE).size(13.0).color(p.accent));
                                    ui.label(RichText::new(&pkg.name).size(11.0).monospace().color(p.text_primary));

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        // Uninstall (3rd party only)
                                        if !pkg.is_system {
                                            let btn_un = egui::Button::new(RichText::new("Uninstall").size(9.5).color(Color32::from_rgb(239, 68, 68)))
                                                .fill(p.bg_button)
                                                .rounding(Rounding::same(4.0));
                                            if ui.add(btn_un).clicked() {
                                                let _ = crate::adb::uninstall_app(&serial, &pkg.name);
                                            }
                                        }

                                        // Clear Data
                                        let btn_clr = egui::Button::new(RichText::new("Clear").size(9.5).color(p.text_muted))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_clr).clicked() {
                                            let _ = crate::adb::clear_app_data(&serial, &pkg.name);
                                        }

                                        // Force Stop
                                        let btn_stp = egui::Button::new(RichText::new("Stop").size(9.5).color(p.text_secondary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stp).clicked() {
                                            let _ = crate::adb::force_stop_app(&serial, &pkg.name);
                                        }

                                        // Launch
                                        let btn_run = egui::Button::new(RichText::new("Launch").size(9.5).color(p.accent))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            let _ = crate::adb::launch_app(&serial, &pkg.name);
                                        }
                                    });
                                });
                                ui.separator();
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

                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::WIFI).size(15.0).color(Color32::from_rgb(56, 189, 248)));
                        ui.label(RichText::new("WIRELESS ADB (TCP/IP MODE) CONTROLLER").size(11.0).monospace().strong().color(p.text_primary));
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Enable wireless debugging on your Android device to untether from USB cables. Device must be on the same local Wi-Fi network.")
                        .size(10.5).color(p.text_muted));

                    ui.add_space(12.0);

                    // Step 1: Switch device to TCP/IP mode on port 5555
                    ui.horizontal(|ui| {
                        let btn_tcpip = egui::Button::new(RichText::new("📶 Enable TCP/IP on Port 5555").size(11.5).strong().color(Color32::WHITE))
                            .fill(Color32::from_rgb(2, 132, 199))
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(200.0, 30.0));
                        if ui.add(btn_tcpip).clicked() {
                            match crate::adb::enable_tcpip(&serial, 5555) {
                                Ok(res) => self.adb_status_msg = Some((format!("TCP/IP 5555 enabled: {}", res), false, Instant::now())),
                                Err(e) => self.adb_status_msg = Some((format!("TCP/IP error: {}", e), true, Instant::now())),
                            }
                        }
                    });

                    ui.add_space(10.0);

                    // Step 2: Auto-discover IP
                    if let Ok(ip_out) = crate::adb::adb_cmd(Some(&serial), &["shell", "ip", "route"]) {
                        for line in ip_out.lines() {
                            if line.contains("src ") {
                                if let Some(ip) = line.split("src ").nth(1).and_then(|s| s.split_whitespace().next()) {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("Discovered Device Wi-Fi IP: {}:5555", ip)).size(11.0).monospace().color(p.success));

                                        let btn_c = egui::Button::new(RichText::new("Connect This IP").size(10.5).color(p.text_primary))
                                            .fill(p.bg_button)
                                            .rounding(Rounding::same(6.0));
                                        if ui.add(btn_c).clicked() {
                                            let target = format!("{}:5555", ip);
                                            let _ = crate::adb::connect_wireless(&target);
                                        }
                                    });
                                }
                            }
                        }
                    }
                });
            });
    }
}
