use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Bluetooth Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_bluetooth_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();

        if is_remote {
            let hid = host_id.unwrap_or_default();
            let linux_host = snap.os_name.to_ascii_lowercase().contains("linux");
            let windows_host = snap.os_name.to_ascii_lowercase().contains("windows");
            let last_fetch = self.remote_bluetooth_last_fetch.lock().unwrap().get(hid).copied();
            let in_flight = self.remote_bluetooth_in_flight.lock().unwrap().contains(hid);
            let poll_interval = if windows_host { 15 } else { 5 };
            if (linux_host || windows_host) && last_fetch.map_or(true, |time| time.elapsed() >= Duration::from_secs(poll_interval)) && !in_flight {
                if let Some(host) = self.ssh_hosts.iter().find(|host| host.id == hid).cloned() {
                    self.trigger_remote_bluetooth_query(host, windows_host);
                }
            }
            let remote_snapshot = self.remote_bluetooth_snapshots.lock().unwrap().get(hid).cloned();
            Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(ui, |ui| {
                    ui.label(RichText::new(format!("Bluetooth · {}", self.host_display_name(host_id))).size(18.0).strong().color(p.text_primary));
                    ui.add_space(6.0);
                    if snap.os_name.is_empty() {
                        ui.label(RichText::new("Waiting for remote operating system details…").size(11.0).color(p.text_secondary));
                    } else if !linux_host && !windows_host {
                        ui.label(RichText::new("Remote Bluetooth monitoring requires Linux with BlueZ or Windows Bluetooth support.").size(11.0).color(p.text_secondary));
                    } else if let Some(remote) = remote_snapshot {
                        if let Some((error, _, _)) = &remote.status_msg {
                            ui.label(RichText::new(error).size(11.0).color(p.text_secondary));
                        } else if windows_host {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("{} paired", remote.get_paired_count())).size(12.0).strong().color(p.text_primary));
                                ui.label(RichText::new("Windows reports device connection state; radio power is unavailable.").size(10.0).color(p.text_muted));
                            });
                            ui.add_space(8.0);
                            if remote.devices.is_empty() {
                                ui.label(RichText::new("No paired devices reported by Windows.").size(11.0).color(p.text_muted));
                            }
                            for device in remote.devices.iter().filter(|device| device.paired) {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&device.name).size(11.5).color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(if device.connected { "Connected" } else { "Paired" }).size(10.0).color(if device.connected { accent } else { p.text_muted }));
                                    });
                                });
                                ui.label(RichText::new(&device.mac).size(9.0).monospace().color(p.text_muted));
                            }
                        } else if let Some(controller) = &remote.controller {
                            ui.horizontal(|ui| {
                                let name = if controller.alias.is_empty() { &controller.name } else { &controller.alias };
                                let state = if controller.powered { "Radio on" } else { "Radio off" };
                                ui.label(RichText::new(format!("{} · {}", name, state)).size(12.0).strong().color(if controller.powered { accent } else { p.text_secondary }));
                                ui.label(RichText::new(format!("{} paired", remote.get_paired_count())).size(10.0).color(p.text_muted));
                            });
                            ui.add_space(8.0);
                            if remote.devices.is_empty() {
                                ui.label(RichText::new("No paired devices reported.").size(11.0).color(p.text_muted));
                            }
                            for device in remote.devices.iter().filter(|device| device.paired) {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&device.name).size(11.5).color(p.text_primary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(if device.connected { "Connected" } else { "Paired" }).size(10.0).color(if device.connected { accent } else { p.text_muted }));
                                        if let Some(battery) = device.battery_pct {
                                            ui.label(RichText::new(format!("{battery}%")).size(10.0).color(p.text_secondary));
                                        }
                                    });
                                });
                            }
                        } else {
                            ui.label(RichText::new("No Bluetooth adapter is currently reported by BlueZ.").size(11.0).color(p.text_secondary));
                        }
                    } else if in_flight {
                        ui.horizontal(|ui| { ui.spinner(); ui.label(RichText::new("Reading remote Bluetooth status…").size(11.0).color(p.text_secondary)); });
                    } else {
                        ui.label(RichText::new("Remote Bluetooth status is not available.").size(11.0).color(p.text_secondary));
                    }
                });
            return;
        }

        // Query Bluetooth status every 3 seconds
        if self.bluetooth_last_query.elapsed().as_secs() >= 3 {
            self.trigger_bluetooth_query(Some(ui.ctx().clone()));
        }

        let bt_snap = if let Ok(lock) = self.bluetooth_snapshot.try_lock() {
            lock.clone()
        } else {
            crate::bluetooth::BluetoothSnapshot::default()
        };

        if let Some(ref ctrl) = bt_snap.controller {
            self.bt_radio_on = ctrl.powered;
            self.bt_discoverable = ctrl.discoverable;
        }

        ScrollArea::vertical()
            .id_salt("sensors_bt_scroll_v2")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // 1. Header Banner
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.vertical(|ui| {
                            // Row 1: Overline / Subsystem Tag
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(crate::lucide::BLUETOOTH).size(15.0).color(accent));
                                ui.label(RichText::new("BLUETOOTH").size(10.5).monospace().strong().color(p.text_muted));
                            });

                            ui.add_space(2.0);

                            // Row 2: Title & Status Badge on Left, Radio State Card on Right
                            let avail_w = ui.available_width();
                            let right_card_w = 210.0_f32;
                            let is_narrow = avail_w < 680.0;

                            let (status_text, status_col) = if !bt_snap.available {
                                ("UNAVAILABLE", p.text_muted)
                            } else if self.bt_radio_on {
                                ("RADIO ON", accent)
                            } else {
                                ("RADIO OFF", p.text_muted)
                            };

                            if is_narrow {
                                // Narrow viewport: Title row with badge, then dedicated row for radio card
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(RichText::new("Bluetooth devices").size(18.0).strong().color(p.text_primary));
                                    Frame::none()
                                        .fill(status_col.gamma_multiply(0.14))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(d_rect.center(), 2.8, status_col);
                                                ui.label(RichText::new(status_text).size(8.5).monospace().strong().color(p.text_primary));
                                            });
                                        });
                                });

                                ui.add_space(6.0);

                                Frame::none()
                                    .fill(p.bg_input)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                ui.label(RichText::new("RADIO STATE").size(9.0).monospace().strong().color(p.text_muted));
                                                ui.label(RichText::new("Hardware RF Transceiver").size(8.5).color(p.text_muted));
                                            });
                                            ui.add_space(10.0);
                                            let mut power_toggle = self.bt_radio_on;
                                            if ui.add_enabled_ui(bt_snap.available, |ui| render_custom_toggle_switch(ui, &mut power_toggle, accent)).inner {
                                                let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                                let ctx_clone = ui.ctx().clone();
                                                std::thread::spawn(move || {
                                                    let result = crate::bluetooth::set_radio_power(power_toggle);
                                                    let mut updated = crate::bluetooth::query_bluetooth_status();
                                                    updated.status_msg = Some(match result { Ok(()) => ("Bluetooth radio updated".into(), false, Instant::now()), Err(error) => (format!("Radio update failed: {error}"), true, Instant::now()) });
                                                    if let Ok(mut lock) = snap_arc.lock() {
                                                        *lock = updated;
                                                    }
                                                    ctx_clone.request_repaint();
                                                });
                                            }
                                        });
                                    });
                            } else {
                                // Wide viewport: Title on Left, Radio State Card on Right
                                ui.horizontal(|ui| {
                                    let left_w = (avail_w - right_card_w - 16.0).max(100.0);
                                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        ui.label(RichText::new("Bluetooth devices").size(18.0).strong().color(p.text_primary));
                                        Frame::none()
                                            .fill(status_col.gamma_multiply(0.14))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 2.5))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                    ui.painter().circle_filled(d_rect.center(), 2.8, status_col);
                                                    ui.label(RichText::new(status_text).size(8.5).monospace().strong().color(p.text_primary));
                                                });
                                            });
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    ui.vertical(|ui| {
                                                        ui.spacing_mut().item_spacing.y = 1.0;
                                                        ui.label(RichText::new("RADIO STATE").size(9.0).monospace().strong().color(p.text_muted));
                                                        ui.label(RichText::new("Hardware RF Transceiver").size(8.5).color(p.text_muted));
                                                    });
                                                    ui.add_space(10.0);
                                                    let mut power_toggle = self.bt_radio_on;
                                                    if ui.add_enabled_ui(bt_snap.available, |ui| render_custom_toggle_switch(ui, &mut power_toggle, accent)).inner {
                                                        let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                                        let ctx_clone = ui.ctx().clone();
                                                        std::thread::spawn(move || {
                                                            let result = crate::bluetooth::set_radio_power(power_toggle);
                                                            let mut updated = crate::bluetooth::query_bluetooth_status();
                                                            updated.status_msg = Some(match result { Ok(()) => ("Bluetooth radio updated".into(), false, Instant::now()), Err(error) => (format!("Radio update failed: {error}"), true, Instant::now()) });
                                                            if let Ok(mut lock) = snap_arc.lock() {
                                                                *lock = updated;
                                                            }
                                                            ctx_clone.request_repaint();
                                                        });
                                                    }
                                                });
                                            });
                                    });
                                });
                            }

                            ui.add_space(4.0);

                            // Row 3: Subtitle spanning the full card width underneath (Never collides or overlaps)
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Bluetooth controller and device status reported by BlueZ").size(10.0).color(p.text_muted));
                            });

                            ui.add_space(2.0);

                            // Row 4: Controller desc spanning the full card width underneath (Never collides or overlaps)
                            ui.horizontal_wrapped(|ui| {
                                let ctrl_desc = if let Some(ref ctrl) = bt_snap.controller {
                                    format!("CONTROLLER: {} ({}) • BlueZ • MAC: {}", ctrl.name, ctrl.alias, ctrl.mac)
                                } else {
                                    "CONTROLLER: unavailable".to_string()
                                };
                                ui.label(RichText::new(ctrl_desc).size(8.5).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 2. Action Toolbar (Scan / Pair and Discoverable Toggle)
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // Scan / Pair button
                            let is_scanning = self.bluetooth_scanning.load(std::sync::atomic::Ordering::SeqCst);
                            let btn_label = if is_scanning { "Scanning Nearby..." } else { "+ Scan for Devices" };
                            let can_scan = !is_scanning && bt_snap.available && self.bt_radio_on;
                            let (pair_rect, pair_resp) = ui.allocate_exact_size(egui::vec2(140.0, 28.0), if can_scan { egui::Sense::click() } else { egui::Sense::hover() });
                            let pair_bg = if !can_scan {
                                accent_with_alpha(accent, 28)
                            } else if pair_resp.hovered() {
                                p.info
                            } else {
                                accent
                            };
                            let pair_text_col = if !can_scan { p.text_muted } else { p.accent_text };

                            ui.painter().rect_filled(pair_rect, Rounding::same(6.0), pair_bg);
                            ui.painter().text(pair_rect.center(), egui::Align2::CENTER_CENTER, btn_label, egui::FontId::monospace(10.0), pair_text_col);

                            if pair_resp.clicked() && can_scan {
                                crate::bluetooth::trigger_discovery_async(Arc::clone(&self.bluetooth_snapshot), Arc::clone(&self.bluetooth_scanning));
                            }

                            ui.add_space(16.0);

                            // Discoverable toggle
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new("Discoverable to nearby devices").size(10.5).color(p.text_primary));
                                let mut disc_toggle = self.bt_discoverable;
                                if ui.add_enabled_ui(bt_snap.controller.as_ref().is_some_and(|ctrl| ctrl.discoverable_reported && ctrl.powered), |ui| render_custom_toggle_switch(ui, &mut disc_toggle, accent)).inner {
                                    let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                    let ctx_clone = ui.ctx().clone();
                                    std::thread::spawn(move || {
                                        let result = crate::bluetooth::set_discoverable(disc_toggle);
                                        let mut updated = crate::bluetooth::query_bluetooth_status();
                                        updated.status_msg = Some(match result { Ok(()) => ("Bluetooth discoverability updated".into(), false, Instant::now()), Err(error) => (format!("Discoverability update failed: {error}"), true, Instant::now()) });
                                        if let Ok(mut lock) = snap_arc.lock() {
                                            *lock = updated;
                                        }
                                        ctx_clone.request_repaint();
                                    });
                                }
                            });

                            // Status message toast banner
                            if let Some((ref msg, is_err, time)) = bt_snap.status_msg {
                                if time.elapsed().as_secs() < 6 {
                                    ui.add_space(12.0);
                                    let toast_col = if is_err { p.text_secondary } else { accent };
                                    Frame::none()
                                        .fill(toast_col.gamma_multiply(0.12))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(msg).size(9.0).monospace().color(p.text_primary));
                                        });
                                }
                            }


                        });
                    });

                ui.add_space(8.0);

                // 3. Paired Devices Matrix (Live BlueZ devices)
                let connected_count = bt_snap.get_connected_count();
                let paired_count = bt_snap.get_paired_count();

                ui.horizontal(|ui| {
                    ui.label(RichText::new(if bt_snap.available { format!("{} CONNECTED, {} PAIRED", connected_count, paired_count) } else { "BLUETOOTH CONTROLLER UNAVAILABLE".to_string() }).size(10.0).monospace().strong().color(p.text_muted));
                });
                ui.add_space(4.0);

                if bt_snap.devices.is_empty() {
                    // Empty state card
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(24.0, 18.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(crate::lucide::BLUETOOTH).size(28.0).color(accent));
                                ui.add_space(4.0);
                                ui.label(RichText::new(if bt_snap.available { "No paired Bluetooth devices found" } else { "Bluetooth controller unavailable" }).size(13.0).strong().color(p.text_primary));
                                ui.add_space(2.0);
                                ui.label(RichText::new(if bt_snap.available { "Turn on Bluetooth and scan for devices to pair." } else { "Check that Bluetooth and BlueZ are available, then refresh." }).size(10.0).color(p.text_muted));
                            });
                        });
                } else {
                    sensor_card_columns(ui, bt_snap.devices.len(), |ui, i| {
                            let dev = &bt_snap.devices[i];
                            let card_bg = if dev.connected { p.bg_card_active } else { p.bg_card };
                            Frame::none()
                                .fill(card_bg)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    // Device Icon
                                    let icon = if dev.icon.contains("audio") || dev.icon.contains("head") {
                                        crate::lucide::HEADPHONES
                                    } else if dev.icon.contains("mouse") {
                                        crate::lucide::COMPASS
                                    } else if dev.icon.contains("keyboard") {
                                        crate::lucide::KEYBOARD
                                    } else if dev.icon.contains("gamepad") || dev.icon.contains("gaming") {
                                        crate::lucide::GPU
                                    } else {
                                        crate::lucide::BLUETOOTH
                                    };

                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(icon).size(16.0).color(accent));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Some(batt) = dev.battery_pct {
                                                Frame::none()
                                                    .fill(accent_with_alpha(accent, 20))
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(format!("{}% BATTERY", batt)).size(8.5).monospace().strong().color(p.text_primary));
                                                    });
                                            } else {
                                                let (tag_txt, tag_col) = if dev.connected {
                                                    ("CONNECTED", accent)
                                                } else {
                                                    ("PAIRED", p.text_muted)
                                                };
                                                Frame::none()
                                                    .fill(tag_col.gamma_multiply(0.12))
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(tag_txt).size(8.0).monospace().strong().color(p.text_primary));
                                                    });
                                            }
                                        });
                                    });

                                    ui.add_space(4.0);
                                    let clean_name = crate::bluetooth::clean_device_name(&dev.name);
                                    ui.add(egui::Label::new(RichText::new(&clean_name).size(14.0).strong().color(p.text_primary)).truncate()).on_hover_text(&clean_name);
                                    ui.label(RichText::new(&dev.mac).size(8.5).monospace().color(p.text_muted));

                                    ui.add_space(8.0);

                                    // Device metadata rows
                                    let dev_type = if !dev.icon.is_empty() { &dev.icon } else { "peripheral" };
                                    let status_str = if dev.connected { "ACTIVE LINK" } else { "STANDBY" };
                                    let stats = [
                                        ("TYPE", dev_type),
                                        ("BONDED", if dev.trusted { "TRUSTED" } else { "NO" }),
                                        ("STATE", status_str),
                                    ];

                                    for (k, v) in stats {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(k).size(8.5).monospace().color(p.text_muted));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(v).size(8.5).monospace().strong().color(p.text_secondary));
                                            });
                                        });
                                        ui.add_space(2.0);
                                    }

                                    ui.add_space(8.0);

                                    // Action buttons (Connect / Disconnect + Unpair)
                                    ui.horizontal(|ui| {
                                        let btn_w = (ui.available_width() - 26.0 - ui.spacing().item_spacing.x).max(0.0);
                                        if dev.connected {
                                            let (dis_rect, dis_resp) = ui.allocate_exact_size(egui::vec2(btn_w, 26.0), egui::Sense::click());
                                            let dis_bg = if dis_resp.hovered() { p.bg_button_hover } else { p.bg_subtle };
                                            ui.painter().rect_filled(dis_rect, Rounding::same(6.0), dis_bg);
                                            ui.painter().text(dis_rect.center(), egui::Align2::CENTER_CENTER, "Disconnect", egui::FontId::monospace(9.5), p.text_secondary);

                                            if dis_resp.clicked() {
                                                let mac_c = dev.mac.clone();
                                                let name_c = dev.name.clone();
                                                let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                                let ctx_c = ui.ctx().clone();
                                                std::thread::spawn(move || {
                                                    let res = crate::bluetooth::disconnect_device(&mac_c);
                                                    let updated = crate::bluetooth::query_bluetooth_status();
                                                    if let Ok(mut lock) = snap_arc.lock() {
                                                        *lock = updated;
                                                        match res {
                                                            Ok(_) => lock.status_msg = Some((format!("Disconnected {}", name_c), false, Instant::now())),
                                                            Err(e) => lock.status_msg = Some((format!("Failed to disconnect: {}", e), true, Instant::now())),
                                                        }
                                                    }
                                                    ctx_c.request_repaint();
                                                });
                                            }
                                        } else {
                                            let (conn_rect, conn_resp) = ui.allocate_exact_size(egui::vec2(btn_w, 26.0), egui::Sense::click());
                                            let conn_bg = if conn_resp.hovered() { accent_with_alpha(accent, 38) } else { accent_with_alpha(accent, 18) };
                                            ui.painter().rect_filled(conn_rect, Rounding::same(6.0), conn_bg);
                                            ui.painter().text(conn_rect.center(), egui::Align2::CENTER_CENTER, "Connect", egui::FontId::monospace(9.5), p.text_primary);

                                            if conn_resp.clicked() {
                                                let mac_c = dev.mac.clone();
                                                let name_c = dev.name.clone();
                                                let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                                let ctx_c = ui.ctx().clone();
                                                if let Ok(mut lock) = snap_arc.lock() {
                                                    lock.status_msg = Some((format!("Connecting to {}...", name_c), false, Instant::now()));
                                                }
                                                std::thread::spawn(move || {
                                                    let res = crate::bluetooth::connect_device(&mac_c);
                                                    let updated = crate::bluetooth::query_bluetooth_status();
                                                    if let Ok(mut lock) = snap_arc.lock() {
                                                        *lock = updated;
                                                        match res {
                                                            Ok(_) => lock.status_msg = Some((format!("Connected to {}", name_c), false, Instant::now())),
                                                            Err(e) => lock.status_msg = Some((format!("Connect failed: {}", e), true, Instant::now())),
                                                        }
                                                    }
                                                    ctx_c.request_repaint();
                                                });
                                            }
                                        }

                                        // Unpair button
                                        let (unpair_rect, unpair_resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                                        let u_bg = if unpair_resp.hovered() { p.bg_button_hover } else { p.bg_input };
                                        ui.painter().rect_filled(unpair_rect, Rounding::same(6.0), u_bg);
                                        ui.painter().text(unpair_rect.center(), egui::Align2::CENTER_CENTER, crate::lucide::TRASH, egui::FontId::monospace(10.0), p.text_muted);
                                        if unpair_resp.clicked() {
                                            let mac_c = dev.mac.clone();
                                            let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                            let ctx_c = ui.ctx().clone();
                                            std::thread::spawn(move || {
                                                let result = crate::bluetooth::remove_device(&mac_c);
                                                let updated = crate::bluetooth::query_bluetooth_status();
                                                if let Ok(mut lock) = snap_arc.lock() {
                                                    *lock = updated;
                                                    lock.status_msg = Some(match result { Ok(()) => ("Device removed from paired list".into(), false, Instant::now()), Err(error) => (format!("Device removal failed: {error}"), true, Instant::now()) });
                                                }
                                                ctx_c.request_repaint();
                                            });
                                        }
                                    });
                            });
                    });
                }

                // 4. Discovered Nearby Devices (if any found during scan)
                if !bt_snap.discovered_devices.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("DISCOVERED NEARBY DEVICES ({})", bt_snap.discovered_devices.len())).size(10.0).monospace().strong().color(p.text_secondary));
                    ui.add_space(4.0);

                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for disc_dev in &bt_snap.discovered_devices {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::lucide::BLUETOOTH).size(12.0).color(accent));
                                    ui.add_space(4.0);
                                    let clean_dname = crate::bluetooth::clean_device_name(&disc_dev.name);
                                    ui.label(RichText::new(&clean_dname).size(11.5).strong().color(p.text_primary));
                                    ui.label(RichText::new(&disc_dev.mac).size(9.0).monospace().color(p.text_muted));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let (p_rect, p_resp) = ui.allocate_exact_size(egui::vec2(96.0, 24.0), egui::Sense::click());
                                        let bg = if p_resp.hovered() { p.info } else { accent };
                                        ui.painter().rect_filled(p_rect, Rounding::same(6.0), bg);
                                        ui.painter().text(p_rect.center(), egui::Align2::CENTER_CENTER, "+ Pair & Trust", egui::FontId::monospace(8.5), p.accent_text);

                                        if p_resp.clicked() {
                                            let mac_c = disc_dev.mac.clone();
                                            let snap_arc = Arc::clone(&self.bluetooth_snapshot);
                                            let ctx_c = ui.ctx().clone();
                                            std::thread::spawn(move || {
                                                let result = crate::bluetooth::pair_and_trust(&mac_c);
                                                let updated = crate::bluetooth::query_bluetooth_status();
                                                if let Ok(mut lock) = snap_arc.lock() {
                                                    *lock = updated;
                                                    lock.status_msg = Some(match result { Ok(()) => ("Device paired and trusted".into(), false, Instant::now()), Err(error) => (format!("Pairing failed: {error}"), true, Instant::now()) });
                                                }
                                                ctx_c.request_repaint();
                                            });
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        });
                }

            });
    }
}
