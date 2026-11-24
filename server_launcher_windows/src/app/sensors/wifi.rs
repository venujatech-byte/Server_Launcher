use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Wi-Fi & Network Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_wifi_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();

        if !is_remote {
            let results: Vec<_> = self.wifi_action_results.lock().unwrap().drain(..).collect();
            for (message, error, refresh) in results {
                self.wifi_status_msg = Some((message, error, Instant::now()));
                if refresh {
                    self.wifi_last_query = Instant::now() - Duration::from_secs(10);
                    self.trigger_wifi_query(Some(ui.ctx().clone()));
                }
            }
        }

        // Only query local Wi-Fi telemetry when viewing the local PC (10s throttle prevents high CPU from external commands)
        if !is_remote && self.wifi_last_query.elapsed().as_secs() >= 10 {
            self.trigger_wifi_query(Some(ui.ctx().clone()));
        }

        let wifi_snap = if !is_remote {
            if let Ok(lock) = self.wifi_snapshot.try_lock() {
                lock.clone()
            } else {
                crate::wifi::WifiSnapshot::default()
            }
        } else {
            crate::wifi::WifiSnapshot::default()
        };

        let remote_interface_id = egui::Id::new(("remote_wifi_interface", host_id));
        let saved_interface = ui.ctx().data(|data| data.get_temp::<String>(remote_interface_id));
        let primary_net = if is_remote {
            select_remote_interface(&snap.networks, saved_interface.as_deref())
        } else {
            snap.networks.iter().find(|n| n.name == wifi_snap.interface)
        };
        let iface_name = primary_net.map(|n| n.name.as_str()).unwrap_or("Unavailable");
        let is_wireless = if is_remote { primary_net.and_then(|n| n.is_wireless) } else { (!wifi_snap.interface.is_empty()).then_some(true) };
        let remote_wifi_state = primary_net.and_then(|interface| interface.wifi_state.as_deref());
        let remote_state = remote_wifi_state.or_else(|| primary_net.and_then(|interface| interface.link_state.as_deref())).unwrap_or("Unavailable");
        let signal_pct = if is_remote { primary_net.and_then(|interface| interface.wifi_signal_pct) } else { wifi_snap.signal_pct };
        let is_connected = if is_remote { remote_wifi_state.map_or_else(|| remote_state.eq_ignore_ascii_case("up"), |state| state.eq_ignore_ascii_case("connected")) } else { wifi_snap.is_connected };

        ScrollArea::vertical()
            .id_salt("sensors_wifi_scroll_v2")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // 1. Header Banner
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.vertical(|ui| {
                            // Row 1: Overline / Subsystem Tag
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new("●").size(7.0).color(accent));
                                let overline = if is_remote {
                                    format!("WI-FI & NETWORK • HOST '{}'", self.host_display_name(host_id).to_uppercase())
                                } else {
                                    format!("WI-FI & NETWORK • {}", wifi_snap.interface.to_uppercase())
                                };
                                ui.label(RichText::new(overline).size(9.0).monospace().color(p.text_muted));
                            });

                            ui.add_space(3.0);

                            // Row 2: Title & Status Badge on Left, Action / Badges on Right
                            let avail_w = ui.available_width();
                            let is_narrow = avail_w < 700.0;

                            let title = match is_wireless {
                                Some(false) if is_remote => "Network Subsystem & Host Interface",
                                None if is_remote => "Network Interface",
                                _ => "Wireless Network & Wi-Fi",
                            };
                            let (status_text, dot_col) = if is_remote {
                                match remote_wifi_state {
                                    Some(state) if state.eq_ignore_ascii_case("connected") => ("CONNECTED", accent),
                                    Some(state) if state.eq_ignore_ascii_case("disconnected") => ("DISCONNECTED", p.text_muted),
                                    _ if primary_net.is_some() => ("INTERFACE FOUND", accent),
                                    _ => ("UNAVAILABLE", p.text_muted),
                                }
                            } else if wifi_snap.interface.is_empty() {
                                ("UNAVAILABLE", p.text_muted)
                            } else if is_connected {
                                ("CONNECTED", accent)
                            } else if wifi_snap.state == "DISCONNECTED" {
                                ("DISCONNECTED", p.text_muted)
                            } else {
                                ("STATE UNKNOWN", p.text_muted)
                            };
                            let status_text_col = if is_connected { p.text_primary } else { p.text_muted };

                            let render_title_and_status = |ui: &mut Ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(RichText::new(title).size(16.5).strong().color(p.text_primary));
                                    Frame::none()
                                        .fill(dot_col.gamma_multiply(0.14))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.5))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(d_rect.center(), 2.8, dot_col);
                                                ui.label(RichText::new(status_text).size(8.5).monospace().strong().color(status_text_col));
                                            });
                                        });
                                });
                            };

                            if is_narrow {
                                render_title_and_status(ui);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    if !is_remote {
                                        // Re-Scan Button (borderless tonal pill)
                                        let (rescan_rect, rescan_resp) = ui.allocate_exact_size(egui::vec2(86.0, 26.0), egui::Sense::click());
                                        let bg = if rescan_resp.hovered() { accent_with_alpha(accent, 32) } else { accent_with_alpha(accent, 18) };
                                        ui.painter().rect_filled(rescan_rect, Rounding::same(6.0), bg);
                                        ui.painter().rect_stroke(rescan_rect, Rounding::same(6.0), Stroke::new(1.0_f32, accent_with_alpha(accent, 40)));
                                        ui.painter().text(rescan_rect.center(), egui::Align2::CENTER_CENTER, format!("{} Re-Scan", crate::lucide::REFRESH_CW), egui::FontId::monospace(9.0), p.text_primary);
                                        if rescan_resp.clicked() && !wifi_snap.interface.is_empty() {
                                            self.queue_wifi_action(Some(wifi_snap.interface.clone()), ui.ctx().clone());
                                        }

                                        // Badges
                                        if !wifi_snap.security.is_empty() {
                                            Frame::none()
                                                .fill(p.bg_input)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(&wifi_snap.security).size(9.0).monospace().color(p.text_secondary));
                                                });
                                        }

                                        let ch_text = wifi_channel_label(wifi_snap.channel, wifi_snap.frequency_mhz);
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(ch_text).size(9.0).monospace().strong().color(p.text_secondary));
                                            });
                                    } else {
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(format!("NIC: {}", iface_name)).size(9.0).monospace().strong().color(p.text_secondary));
                                            });

                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(if snap.socket_stats_available { format!("Sockets: {}", snap.socket_total) } else { "Sockets: —".to_string() }).size(9.0).monospace().color(p.text_secondary));
                                            });
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let right_est = 310.0_f32;
                                    let left_w = (avail_w - right_est - 16.0).max(100.0);
                                    ui.allocate_ui_with_layout(egui::vec2(left_w, 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        render_title_and_status(ui);
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        if !is_remote {
                                            // Re-Scan Button (borderless tonal pill)
                                            let (rescan_rect, rescan_resp) = ui.allocate_exact_size(egui::vec2(86.0, 26.0), egui::Sense::click());
                                            let bg = if rescan_resp.hovered() { accent_with_alpha(accent, 32) } else { accent_with_alpha(accent, 18) };
                                            ui.painter().rect_filled(rescan_rect, Rounding::same(6.0), bg);
                                            ui.painter().rect_stroke(rescan_rect, Rounding::same(6.0), Stroke::new(1.0_f32, accent_with_alpha(accent, 40)));
                                            ui.painter().text(rescan_rect.center(), egui::Align2::CENTER_CENTER, format!("{} Re-Scan", crate::lucide::REFRESH_CW), egui::FontId::monospace(9.0), p.text_primary);
                                            if rescan_resp.clicked() && !wifi_snap.interface.is_empty() {
                                                self.queue_wifi_action(Some(wifi_snap.interface.clone()), ui.ctx().clone());
                                            }

                                            // Security Badge
                                            if !wifi_snap.security.is_empty() {
                                                Frame::none()
                                                    .fill(p.bg_input)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(&wifi_snap.security).size(9.0).monospace().color(p.text_secondary));
                                                    });
                                            }

                                            // Channel Badge
                                            let ch_text = wifi_channel_label(wifi_snap.channel, wifi_snap.frequency_mhz);
                                            Frame::none()
                                                .fill(p.bg_input)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(ch_text).size(9.0).monospace().strong().color(p.text_secondary));
                                                });
                                        } else {
                                            Frame::none()
                                                .fill(p.bg_input)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(format!("NIC: {}", iface_name)).size(9.0).monospace().strong().color(p.text_secondary));
                                                });

                                            Frame::none()
                                                .fill(p.bg_input)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(if snap.socket_stats_available { format!("Sockets: {}", snap.socket_total) } else { "Sockets: —".to_string() }).size(9.0).monospace().color(p.text_secondary));
                                                });
                                        }
                                    });
                                });
                            }

                            ui.add_space(4.0);

                            // Row 3: Subtitle spanning the full card width underneath (wrapped, never overlaps)
                            ui.horizontal_wrapped(|ui| {
                                if is_remote {
                                    ui.label(RichText::new(format!("SSH link to host '{}'", self.host_display_name(host_id))).size(10.0).color(p.text_muted));
                                } else {
                                    ui.label(RichText::new("Wireless interface telemetry").size(10.0).color(p.text_muted));
                                }
                                #[cfg(windows)]
                                if !is_remote && ui.small_button("Windows Network Settings").clicked() {
                                    let _ = std::process::Command::new("cmd.exe").args(["/D", "/C", "start", "", "ms-settings:network-wifi"]).spawn();
                                }
                            });

                            if is_remote && snap.networks.iter().filter(|interface| interface.name != "lo").count() > 1 {
                                let current = iface_name.to_string();
                                let mut selected = current.clone();
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Interface").size(10.0).color(p.text_muted));
                                    egui::ComboBox::from_id_salt(remote_interface_id)
                                        .selected_text(&selected).show_ui(ui, |ui| {
                                            for interface in snap.networks.iter().filter(|interface| interface.name != "lo") {
                                                ui.selectable_value(&mut selected, interface.name.clone(), &interface.name);
                                            }
                                        });
                                });
                                if selected != current {
                                    ui.ctx().data_mut(|data| data.insert_temp(remote_interface_id, selected));
                                }
                            }
                            if !is_remote && wifi_snap.interfaces.len() > 1 {
                                let current = self.wifi_selected_interface.as_ref().filter(|name| wifi_snap.interfaces.contains(name)).cloned().unwrap_or_else(|| wifi_snap.interface.clone());
                                let mut selected = current.clone();
                                egui::ComboBox::from_id_salt("wifi_interface_selector")
                                    .selected_text(&selected).show_ui(ui, |ui| {
                                        for name in &wifi_snap.interfaces { ui.selectable_value(&mut selected, name.clone(), name); }
                                    });
                                if selected != current {
                                    self.wifi_selected_interface = Some(selected);
                                    self.wifi_last_query = Instant::now() - Duration::from_secs(10);
                                    self.trigger_wifi_query(Some(ui.ctx().clone()));
                                }
                            }
                            ui.add_space(2.0);

                            // Row 4: Device and hardware metadata spanning the full card width underneath (wrapped, never overlaps)
                            ui.horizontal_wrapped(|ui| {
                                if is_remote {
                                    ui.label(RichText::new(if snap.network_stats_available { format!("All interfaces: {:.2} GB In • {:.2} GB Out", snap.total_rx_bytes as f64 / 1e9, snap.total_tx_bytes as f64 / 1e9) } else { "Network counters unavailable".to_string() }).size(8.5).monospace().color(p.text_muted));
                                } else {
                                    ui.label(RichText::new(format!("{} • MAC: {}", wifi_snap.product, wifi_snap.mac_address)).size(8.5).monospace().color(p.text_muted));
                                }
                            });
                        });
                    });

                // Status message toast banner
                if let Some((ref msg, is_err, time)) = self.wifi_status_msg {
                    if time.elapsed().as_secs() < 6 {
                        ui.add_space(6.0);
                        let toast_col = if is_err { p.text_secondary } else { accent };
                        Frame::none()
                            .fill(toast_col.gamma_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, toast_col.gamma_multiply(0.35)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let icon = if is_err { crate::lucide::ALERT_TRIANGLE } else { crate::lucide::CHECK };
                                    ui.label(RichText::new(icon).size(11.0).color(toast_col));
                                    ui.label(RichText::new(msg).size(9.0).monospace().strong().color(p.text_primary));
                                });
                            });
                    }
                }

                ui.add_space(8.0);

                // 2. Top Two Columns: 60-Second Realtime Waveform vs Access Point / Link Infrastructure
                let total_w = ui.available_width();
                let gap = 10.0_f32;
                let side_by_side = total_w >= 600.0;
                let left_w = if side_by_side { (total_w - gap) * 0.58 } else { total_w };
                let right_w = if side_by_side { total_w - left_w - gap } else { total_w };

                ui.allocate_ui_with_layout(egui::vec2(total_w, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Left Column: 60-Second Realtime Waveform
                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let (disp_rx_bps, disp_tx_bps) = (snap.total_rx_rate_bps, snap.total_tx_rate_bps);

                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("Live Waveform").size(12.5).strong().color(p.text_primary));
                                        ui.label(RichText::new("All interfaces • RX / TX").size(9.0).monospace().color(p.text_muted));
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;
                                            // TX indicator
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(d_rect.center(), 2.8, p.text_muted);
                                                ui.label(RichText::new(format!("TX: {}", sensor_network_rate(snap, disp_tx_bps))).size(9.5).monospace().color(p.text_secondary));
                                            });

                                            // RX indicator
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(d_rect.center(), 2.8, accent);
                                                ui.label(RichText::new(format!("RX: {}", sensor_network_rate(snap, disp_rx_bps))).size(9.5).monospace().color(p.text_primary));
                                            });
                                        });
                                    });
                                });

                                ui.add_space(8.0);
                                draw_wifi_realtime_waveform(ui, 115.0, &self.net_rx_history, &self.net_tx_history, accent);

                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let rx_rate = sensor_network_rate(snap, disp_rx_bps);
                                    let tx_rate = sensor_network_rate(snap, disp_tx_bps);
                                    let badges = [
                                        (format!("RX: {}", rx_rate), p.text_primary),
                                        (format!("TX: {}", tx_rate), p.text_secondary),
                                        (if snap.network_rates_available { format!("Peak: {:.2} MB/s", self.net_peak_mb.max((disp_rx_bps / 1_000_000.0) as f32)) } else if snap.network_stats_available { "Network rates collecting".to_string() } else { "Network counters unavailable".to_string() }, p.text_secondary),
                                    ];
                                    for (txt, col) in badges {
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(7.0, 3.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(txt).size(8.5).monospace().strong().color(col));
                                            });
                                    }
                                });
                            });
                    });

                    ui.add_space(gap);

                    // Right Column: Access Point (Local) or Network Interface Topology (Remote)
                    ui.allocate_ui_with_layout(egui::vec2(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        let remote_wifi = is_remote && is_wireless == Some(true);
                                        let r_title = if is_remote && !remote_wifi { "TOPOLOGY" } else { "ACCESS POINT" };
                                        let r_sub = if is_remote && !remote_wifi { "Host NIC" } else { "Connected AP" };
                                        ui.label(RichText::new(r_title).size(12.5).strong().color(p.text_primary));
                                        ui.label(RichText::new(r_sub).size(9.0).monospace().color(p.text_muted));
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let qual_str = if let Some(signal) = signal_pct {
                                            let quality = if signal >= 80 { "EXC" } else if signal >= 50 { "GOOD" } else { "FAIR" };
                                            format!("{quality} ({signal}%)")
                                        } else if is_remote && primary_net.is_some() { "INTERFACE FOUND".to_string() } else { "SIGNAL UNAVAILABLE".to_string() };
                                        Frame::none()
                                            .fill(accent_with_alpha(accent, 16))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 2.5))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(qual_str).size(8.5).monospace().strong().color(p.text_primary));
                                            });
                                    });
                                });

                                ui.add_space(8.0);
                                Frame::none()
                                    .fill(p.bg_input)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let icon = if is_remote && is_wireless != Some(true) { crate::lucide::SERVER } else { crate::lucide::WIFI };
                                            ui.label(RichText::new(icon).size(20.0).color(accent));
                                            ui.add_space(6.0);
                                            ui.vertical(|ui| {
                                                let main_lbl = if is_remote { primary_net.and_then(|interface| interface.wifi_ssid.clone()).unwrap_or_else(|| format!("Interface {}", iface_name)) } else if wifi_snap.ssid.is_empty() { "Access point unavailable".to_string() } else { wifi_snap.ssid.clone() };
                                                ui.label(RichText::new(main_lbl).size(13.0).strong().color(p.text_primary));
                                                let band_str = if is_remote && is_wireless == Some(true) {
                                                    let channel = primary_net.and_then(|interface| interface.wifi_channel).map(|channel| wifi_channel_label(channel, 0)).unwrap_or_else(|| "Channel unavailable".into());
                                                    let security = primary_net.and_then(|interface| interface.wifi_security.as_deref()).unwrap_or("Security unavailable");
                                                    format!("{} • {}", channel, security)
                                                } else if is_remote {
                                                    format!("Interface state: {}", remote_state)
                                                } else {
                                                    let frequency = if wifi_snap.frequency_mhz > 0 { format!("{:.1} GHz", wifi_snap.frequency_mhz as f32 / 1000.0) } else { "Frequency unavailable".into() };
                                                    format!("{} • {} ({})", frequency, if wifi_snap.security.is_empty() { "Security unavailable" } else { &wifi_snap.security }, if wifi_snap.driver.is_empty() { "Driver unavailable" } else { &wifi_snap.driver })
                                                };
                                                ui.label(RichText::new(band_str).size(9.0).monospace().color(p.text_muted));
                                            });

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 6.0;
                                                    if is_remote && signal_pct.is_none() {
                                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                                        ui.painter().circle_filled(d_rect.center(), 3.5, accent);
                                                        ui.label(RichText::new("—").size(12.0).monospace().strong().color(p.text_primary));
                                                    } else {
                                                        // 5 Signal Bars
                                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 18.0), egui::Sense::hover());
                                                        let bar_w = 3.5_f32;
                                                        let gap = 2.5_f32;
                                                        let active_bars = ((signal_pct.unwrap_or(0) as f32 / 20.0).round() as usize).clamp(0, 5);
                                                        for i in 0..5 {
                                                            let h = 4.0_f32 + (i as f32 + 1.0) * 2.8_f32;
                                                            let x = rect.left() + i as f32 * (bar_w + gap);
                                                            let y = rect.bottom() - h;
                                                            let b_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(bar_w, h));
                                                            let col = if i < active_bars { accent } else { p.bg_button_hover };
                                                            ui.painter().rect_filled(b_rect, Rounding::same(1.0), col);
                                                        }
                                                        ui.label(RichText::new(signal_pct.map_or_else(|| "—".to_string(), |signal| format!("{signal}%"))).size(12.0).monospace().strong().color(p.text_primary));
                                                    }
                                                });
                                            });
                                        });
                                    });

                                ui.add_space(8.0);
                                ui.columns(3, |cols| {
                                    let ap_chips: [(&str, &str); 3] = if is_remote && is_wireless == Some(true) {
                                        [
                                            ("RATE", primary_net.and_then(|interface| interface.wifi_rate.as_deref()).unwrap_or("Unavailable")),
                                            ("SECURITY", primary_net.and_then(|interface| interface.wifi_security.as_deref()).unwrap_or("Unavailable")),
                                            ("BSSID", primary_net.and_then(|interface| interface.wifi_bssid.as_deref()).unwrap_or("Unavailable")),
                                        ]
                                    } else if is_remote {
                                        [
                                            ("TYPE", match is_wireless { Some(true) => "Wi-Fi", Some(false) => "Network", None => "Unknown" }),
                                            ("STATE", remote_state),
                                            ("SOCKETS", "Host-wide"),
                                        ]
                                    } else {
                                        [
                                            ("TX LINK", wifi_snap.rate.as_str()),
                                            ("RX LINK", wifi_snap.rate.as_str()),
                                            ("BSSID", wifi_snap.bssid.as_str()),
                                        ]
                                    };
                                    for (j, (k, v)) in ap_chips.iter().enumerate() {
                                        let c = &mut cols[j];
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 5.0))
                                            .show(c, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(RichText::new(*k).size(7.5).monospace().color(p.text_muted));
                                                ui.add_space(1.0);
                                                ui.label(RichText::new(*v).size(9.0).monospace().strong().color(p.text_primary));
                                            });
                                    }
                                });
                            });
                    });
                });

                ui.add_space(8.0);

                // 3. Middle 4 Big Stat Cards (Bytes Sent, Bytes Received, Gateway Latency / Socket Count, Packet Loss)
                let tx_gb = snap.total_tx_bytes as f32 / 1e9;
                let rx_gb = snap.total_rx_bytes as f32 / 1e9;
                let tx_slice: Vec<f32> = self.net_tx_history.iter().copied().collect();
                let rx_slice: Vec<f32> = self.net_rx_history.iter().copied().collect();
                let lat_slice: [f32; 0] = [];
                let sock_slice: [f32; 0] = [];
                let loss_slice: [f32; 0] = [];

                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: BYTES SENT
                    let egress_sub = if snap.network_stats_available { "All interfaces • since boot" } else { "Network counters unavailable" };
                    render_big_stat_card(
                        ui,
                        crate::lucide::UPLOAD,
                        "BYTES SENT",
                        &if snap.network_stats_available { format!("{:.2}", tx_gb) } else { "—".to_string() },
                        if snap.network_stats_available { "GB" } else { "" },
                        &egress_sub,
                        "",
                        "",
                        &tx_slice,
                        self.net_peak_mb.max(0.001),
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    1 => {
                    // Card 2: BYTES RECV
                    let ingress_sub = if snap.network_stats_available { "All interfaces • since boot" } else { "Network counters unavailable" };
                    render_big_stat_card(
                        ui,
                        crate::lucide::DOWNLOAD,
                        "BYTES RECV",
                        &if snap.network_stats_available { format!("{:.2}", rx_gb) } else { "—".to_string() },
                        if snap.network_stats_available { "GB" } else { "" },
                        &ingress_sub,
                        "",
                        "",
                        &rx_slice,
                        self.net_peak_mb.max(0.001),
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    2 => {
                    // Card 3: GATEWAY LATENCY (Local) or ACTIVE SOCKETS (Remote)
                    if is_remote {
                        render_big_stat_card(
                            ui,
                            crate::lucide::ACTIVITY,
                            "SOCKETS",
                            &if snap.socket_stats_available { snap.socket_total.to_string() } else { "—".to_string() },
                            "TOTAL",
                            &if snap.socket_stats_available { format!("{} TCP • {} UDP", snap.socket_tcp, snap.socket_udp) } else { "Socket counts unavailable".to_string() },
                            "",
                            "",
                            &sock_slice,
                            if snap.socket_stats_available { (snap.socket_total as f32 * 1.5).max(20.0) } else { 20.0 },
                            None,
                            p.info,
                            p.bg_card,
                            p.text_muted,
                        );
                    } else {
                        let gw_sub = format!("To {}", wifi_snap.gateway);
                        render_big_stat_card(
                            ui,
                            crate::lucide::ACTIVITY,
                            "LATENCY",
                            "—",
                            "",
                            &gw_sub,
                            "Not measured",
                            "",
                            &lat_slice,
                            10.0,
                            None,
                            p.info,
                            p.bg_card,
                            p.text_muted,
                        );
                    }

                    },
                    3 => {
                    // Card 4: LOSS
                    render_big_stat_card(
                        ui,
                        crate::lucide::CHECK,
                        "LOSS",
                        "—",
                        "",
                        "Not measured",
                        "",
                        "",
                        &loss_slice,
                        10.0,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );
                    },
                    _ => unreachable!(),
                });

                ui.add_space(8.0);

                // 4. Top Link State & Telemetry Bar
                let iface_display = if is_remote { iface_name } else { wifi_snap.interface.as_str() };
                let link_rate_str = if is_remote { primary_net.and_then(|interface| interface.wifi_rate.as_deref()).unwrap_or("Unavailable") } else { wifi_snap.rate.as_str() };
                let tunnel_id_str = if is_remote { "SSH telemetry" } else { "Local telemetry" };

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(d_rect.center(), 2.8, accent);
                    let link_status = if is_remote { format!("STATE: {}", remote_state.to_uppercase()) } else if is_connected { "LINK: UP".to_string() } else if wifi_snap.state == "DISCONNECTED" { "LINK: DOWN".to_string() } else { "LINK: UNKNOWN".to_string() };
                    ui.label(RichText::new(link_status).size(9.5).monospace().strong().color(p.text_muted));
                    ui.label(RichText::new("/").size(9.5).monospace().color(p.text_muted));
                    ui.label(RichText::new(iface_display).size(9.5).monospace().strong().color(p.text_primary));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;

                        // Tunnel ID pill
                        Frame::none()
                            .fill(p.bg_subtle)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(tunnel_id_str).size(8.5).monospace().color(p.text_muted));
                            });

                        // Jitter
                        ui.label(RichText::new("Latency unavailable").size(9.5).monospace().color(p.text_muted));

                        // Throughput Rate
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(crate::lucide::ACTIVITY).size(12.0).color(accent));
                            ui.label(RichText::new(link_rate_str).size(9.5).monospace().strong().color(p.text_secondary));
                        });
                    });
                });

                ui.add_space(8.0);

                // 5. Bottom Two Columns: IP Configuration Details vs Active Sockets & Daemons
                let half_w = (total_w - gap) * 0.50;

                // Prepare Data for Left & Right Cards
                let raw_ip = if is_remote { primary_net.and_then(|interface| interface.ipv4.clone()).unwrap_or_else(|| "Unavailable".to_string()) } else { wifi_snap.ip_v4.clone() };
                let (ip_without_cidr, cidr_str) = raw_ip.split_once('/').map(|(ip, _)| (ip, &raw_ip[ip.len()..])).unwrap_or((raw_ip.as_str(), ""));
                let subnet_mask_str = if is_remote { "Unavailable" } else { wifi_snap.subnet_mask.as_str() };
                let gateway_str = if is_remote { "Unavailable" } else { wifi_snap.gateway.as_str() };
                let dns_resolvers_str = if is_remote { "Unavailable" } else { wifi_snap.dns_resolvers.as_str() };
                let ip_v6_str = if is_remote { primary_net.and_then(|interface| interface.ipv6.as_deref()).unwrap_or("Unavailable") } else { wifi_snap.ip_v6.as_str() };
                let mac_str = if is_remote { primary_net.and_then(|interface| interface.mac_address.as_deref()).unwrap_or("Unavailable") } else { wifi_snap.mac_address.as_str() };
                let driver_str = if is_remote { "Unavailable" } else { wifi_snap.driver.as_str() };
                let pci_str = if is_remote { "—" } else { wifi_snap.pci_slot.as_str() };
                let mtu_val = if is_remote { primary_net.and_then(|interface| interface.mtu).map(|mtu| mtu.to_string()).unwrap_or_else(|| "—".to_string()) } else if wifi_snap.mtu == 0 { "—".to_string() } else { wifi_snap.mtu.to_string() };
                let tx_bytes_val = if is_remote { primary_net.map(|interface| interface.tx_bytes) } else { Some(wifi_snap.tx_bytes) };
                let lease_time_str = if is_remote { "Unavailable" } else { wifi_snap.lease_time_remaining.as_str() };
                let _kernel_str = if is_remote { snap.os_version.as_str() } else { wifi_snap.kernel_version.as_str() };

                ui.horizontal(|ui| {
                    // LEFT COLUMN: IP Configuration
                    ui.allocate_ui_with_layout(egui::vec2(half_w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                // Card Header
                                let ip_right_w = 70.0;
                                let ip_avail_w = ui.available_width();
                                let ip_left_w = (ip_avail_w - ip_right_w - 8.0).max(100.0);

                                ui.horizontal(|ui| {
                                    ui.set_width(ip_avail_w);

                                    ui.allocate_ui_with_layout(egui::vec2(ip_left_w, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                                        ui.painter().rect_filled(icon_rect, Rounding::same(6.0), p.bg_subtle);
                                        let icon_char = if is_remote && is_wireless != Some(true) { crate::lucide::SERVER } else { crate::lucide::LAYERS };
                                        ui.painter().text(icon_rect.center(), egui::Align2::CENTER_CENTER, icon_char, egui::FontId::proportional(14.0), accent);

                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("IP Config").size(13.5).strong().color(p.text_primary));
                                                let badge_txt = "NETWORK";
                                                Frame::none()
                                                    .fill(accent_with_alpha(accent, 20))
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(badge_txt).size(8.0).monospace().strong().color(p.text_secondary));
                                                    });
                                            });
                                            let sub = format!("Interface {}", iface_display);
                                            ui.label(RichText::new(sub).size(8.5).monospace().color(p.text_muted));
                                        });
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(format!("MTU {}", mtu_val)).size(8.5).monospace().color(p.text_muted));
                                            let tx_gb_fmt = tx_bytes_val.map(|bytes| format!("TX: {:.1} GB", bytes as f64 / 1e9)).unwrap_or_else(|| "TX: —".to_string());
                                            ui.label(RichText::new(tx_gb_fmt).size(8.5).monospace().color(p.text_muted));
                                        });
                                    });
                                });

                                ui.add_space(10.0);

                                // IPV4 (Prominent Display on a single row with Copy Button)
                                ui.label(RichText::new("IPV4 ADDRESS").size(8.5).monospace().color(p.text_muted));
                                ui.add_space(2.0);

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.label(RichText::new(ip_without_cidr).size(18.0).monospace().strong().color(p.text_primary));
                                    ui.label(RichText::new(cidr_str).size(13.5).monospace().color(p.text_muted));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let can_copy = ip_without_cidr.parse::<std::net::Ipv4Addr>().is_ok();
                                        let (copy_rect, copy_resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), if can_copy { egui::Sense::click() } else { egui::Sense::hover() });
                                        let copy_col = if can_copy && copy_resp.hovered() { accent } else { p.text_muted };
                                        if can_copy && copy_resp.hovered() {
                                            ui.painter().rect_filled(copy_rect, Rounding::same(4.0), accent_with_alpha(accent, 20));
                                        }
                                        ui.painter().text(copy_rect.center(), egui::Align2::CENTER_CENTER, crate::lucide::COPY, egui::FontId::proportional(12.5), copy_col);
                                        let copy_resp = copy_resp.on_hover_text(if can_copy { "Copy IPv4 address to clipboard" } else { "IPv4 address unavailable" });
                                        if copy_resp.clicked() {
                                            let text_to_copy = format!("{}{}", ip_without_cidr, cidr_str);
                                            ui.output_mut(|o| o.copied_text = text_to_copy.clone());
                                            self.wifi_status_msg = Some((format!("Copied IPv4 address '{}' to clipboard", text_to_copy), false, Instant::now()));
                                        }
                                    });
                                });

                                ui.add_space(8.0);

                                // Key-Value Rows
                                // 1. SUBNET
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("SUBNET").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(subnet_mask_str).size(9.5).monospace().color(p.text_primary));
                                    });
                                });
                                ui.add_space(4.0);

                                // 2. GATEWAY
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("GATEWAY").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(gateway_str).size(9.5).monospace().strong().color(p.text_primary));
                                    });
                                });
                                ui.add_space(4.0);

                                // 3. DNS
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("DNS").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;

                                        if !is_remote {
                                            let (flush_rect, flush_resp) = ui.allocate_exact_size(egui::vec2(48.0, 18.0), egui::Sense::click());
                                            let f_bg = if flush_resp.hovered() { p.bg_button_hover } else { p.bg_subtle };
                                            ui.painter().rect_filled(flush_rect, Rounding::same(3.0), f_bg);
                                            ui.painter().text(flush_rect.center(), egui::Align2::CENTER_CENTER, "Flush", egui::FontId::monospace(8.0), p.text_secondary);
                                            if flush_resp.clicked() {
                                                self.queue_wifi_action(None, ui.ctx().clone());
                                            }
                                            flush_resp.on_hover_text("Flush system DNS cache");
                                        }
                                        let clean_dns = dns_resolvers_str.split(',').next().unwrap_or(dns_resolvers_str).trim();
                                        ui.label(RichText::new(clean_dns).size(9.5).monospace().color(p.text_primary));
                                    });
                                });
                                ui.add_space(4.0);

                                // 4. IPV6 (with Copy Button on single line)
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("IPV6").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        let can_copy = ip_v6_str.split('/').next().unwrap_or("").split('%').next().unwrap_or("").parse::<std::net::Ipv6Addr>().is_ok();
                                        let (copy_v6_rect, copy_v6_resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), if can_copy { egui::Sense::click() } else { egui::Sense::hover() });
                                        let copy_v6_col = if can_copy && copy_v6_resp.hovered() { accent } else { p.text_muted };
                                        if can_copy && copy_v6_resp.hovered() {
                                            ui.painter().rect_filled(copy_v6_rect, Rounding::same(3.0), accent_with_alpha(accent, 20));
                                        }
                                        ui.painter().text(copy_v6_rect.center(), egui::Align2::CENTER_CENTER, crate::lucide::COPY, egui::FontId::proportional(11.0), copy_v6_col);
                                        let copy_v6_resp = copy_v6_resp.on_hover_text(if can_copy { "Copy IPv6 address to clipboard" } else { "IPv6 address unavailable" });
                                        if copy_v6_resp.clicked() {
                                            ui.output_mut(|o| o.copied_text = ip_v6_str.to_string());
                                            self.wifi_status_msg = Some(("Copied IPv6 address to clipboard".to_string(), false, Instant::now()));
                                        }

                                        ui.label(RichText::new(ip_v6_str).size(9.0).monospace().color(p.text_secondary));
                                    });
                                });
                                ui.add_space(4.0);

                                // 5. MAC
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("MAC").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(mac_str).size(9.5).monospace().color(p.text_primary));
                                    });
                                });
                                ui.add_space(4.0);

                                // 6. DRIVER
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("DRIVER").size(9.0).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        Frame::none()
                                            .fill(p.bg_subtle)
                                            .rounding(Rounding::same(3.0))
                                            .inner_margin(egui::Margin::symmetric(5.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(pci_str).size(8.0).monospace().color(p.text_muted));
                                            });
                                        ui.label(RichText::new(driver_str).size(9.5).monospace().color(p.text_primary));
                                    });
                                });

                                ui.add_space(8.0);

                                // Sub-pill: DHCP Lease
                                Frame::none()
                                    .fill(p.bg_input)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                ui.label(RichText::new(crate::lucide::SHIELD).size(10.5).color(accent));
                                                ui.label(RichText::new("DHCP Lease").size(9.0).monospace().color(p.text_secondary));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(lease_time_str).size(9.5).monospace().strong().color(p.text_primary));
                                            });
                                        });
                                    });
                            });

                        // Footer below Left Card

                    });

                    ui.add_space(gap);

                    // RIGHT COLUMN: Active Sockets & Daemons
                    ui.allocate_ui_with_layout(egui::vec2(half_w, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                        Frame::none()
                            .fill(p.bg_card)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                // Sockets to display
                                let endpoint_count = if is_remote {
                                    snap.active_sockets.len()
                                } else {
                                    wifi_snap.active_sockets.len()
                                };
                                let ep_badge_str = if is_remote && !snap.active_sockets_available {
                                    "Unavailable".to_string()
                                } else {
                                    format!("{} Endpoints", endpoint_count)
                                };

                                // Card Header
                                let right_btn_w = 72.0;
                                let header_avail_w = ui.available_width();
                                let left_w = (header_avail_w - right_btn_w - 8.0).max(100.0);

                                ui.horizontal(|ui| {
                                    ui.set_width(header_avail_w);

                                    ui.allocate_ui_with_layout(egui::vec2(left_w, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                                        ui.painter().rect_filled(icon_rect, Rounding::same(6.0), p.bg_subtle);
                                        ui.painter().text(icon_rect.center(), egui::Align2::CENTER_CENTER, crate::lucide::RADIO, egui::FontId::monospace(14.0), accent);

                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Sockets").size(13.5).strong().color(p.text_primary));
                                                Frame::none()
                                                    .fill(p.bg_subtle)
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(ep_badge_str).size(8.0).monospace().color(p.text_muted));
                                                    });
                                            });
                                            ui.label(RichText::new("Listening ports").size(8.5).monospace().color(p.text_muted));
                                        });
                                    });

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        // Socket Monitor button (solid electric cyan fill with dark text)
                                        let (mon_rect, mon_resp) = ui.allocate_exact_size(egui::vec2(right_btn_w, 22.0), egui::Sense::click());
                                        let m_bg = if mon_resp.hovered() { p.accent_hover } else { accent };
                                        ui.painter().rect_filled(mon_rect, Rounding::same(5.0), m_bg);
                                        ui.painter().text(mon_rect.center(), egui::Align2::CENTER_CENTER, "Monitor", egui::FontId::monospace(8.5), p.accent_text);
                                        if mon_resp.clicked() {
                                            if !is_remote {
                                                self.trigger_wifi_query(Some(ui.ctx().clone()));
                                                self.wifi_status_msg = Some(("Active sockets refreshed from system kernel".to_string(), false, Instant::now()));
                                            } else {
                                                self.wifi_status_msg = Some(("Remote socket telemetry streaming in realtime via SSH".to_string(), false, Instant::now()));
                                            }
                                        }
                                        mon_resp.on_hover_text("Refresh active listening sockets");
                                    });
                                });

                                ui.add_space(8.0);

                                // Sockets Table
                                egui::Grid::new("wifi_active_sockets_grid")
                                    .num_columns(4)
                                    .striped(false)
                                    .spacing(egui::vec2(12.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("PROCESS").size(8.5).monospace().strong().color(p.text_muted));
                                        ui.label(RichText::new("PROTO").size(8.5).monospace().strong().color(p.text_muted));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new("PORT").size(8.5).monospace().strong().color(p.text_muted));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new("STATUS").size(8.5).monospace().strong().color(p.text_muted));
                                        });
                                        ui.end_row();

                                        if is_remote {
                                            if !snap.active_sockets_available {
                                                ui.label(RichText::new("Remote socket details unavailable (ss is missing or inaccessible)").size(9.0).color(p.text_muted));
                                                ui.label(""); ui.label(""); ui.label(""); ui.end_row();
                                            }
                                            for sock in &snap.active_sockets {
                                                let (proc_name, proto, port, status) = (sock.process.as_str(), sock.proto.as_str(), sock.port.as_str(), sock.status.as_str());
                                                // Col 1: Cyan dot + Process name + muted (parenthesis)
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    ui.label(RichText::new("•").size(9.0).monospace().color(accent));
                                                    if let Some(idx) = proc_name.find('(') {
                                                        let main_part = proc_name[..idx].trim();
                                                        let rest_part = proc_name[idx..].trim();
                                                        ui.label(RichText::new(main_part).size(9.0).monospace().strong().color(p.text_primary));
                                                        ui.label(RichText::new(rest_part).size(8.5).monospace().color(p.text_muted));
                                                    } else {
                                                        ui.label(RichText::new(proc_name).size(9.0).monospace().strong().color(p.text_primary));
                                                    }
                                                });

                                                // Col 2: PROTO
                                                ui.label(RichText::new(proto).size(8.5).monospace().color(p.text_muted));

                                                // Col 3: LOCAL PORT (bold grey/white)
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.label(RichText::new(port).size(9.0).monospace().strong().color(p.text_primary));
                                                });

                                                // Col 4: STATUS (Pill badge)
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let (st_rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 18.0), egui::Sense::hover());
                                                    ui.painter().rect_filled(st_rect, Rounding::same(3.0), accent_with_alpha(accent, 16));
                                                    ui.painter().text(st_rect.center(), egui::Align2::CENTER_CENTER, status, egui::FontId::monospace(8.0), p.text_secondary);
                                                });

                                                ui.end_row();
                                            }
                                        } else {
                                            for sock in &wifi_snap.active_sockets {
                                                // Col 1: Cyan dot + Process name + muted (parenthesis)
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    ui.label(RichText::new("•").size(9.0).monospace().color(accent));
                                                    if let Some(idx) = sock.process.find('(') {
                                                        let main_part = sock.process[..idx].trim();
                                                        let rest_part = sock.process[idx..].trim();
                                                        ui.label(RichText::new(main_part).size(9.0).monospace().strong().color(p.text_primary));
                                                        ui.label(RichText::new(rest_part).size(8.5).monospace().color(p.text_muted));
                                                    } else {
                                                        ui.label(RichText::new(&sock.process).size(9.0).monospace().strong().color(p.text_primary));
                                                    }
                                                });

                                                // Col 2: PROTO
                                                ui.label(RichText::new(&sock.proto).size(8.5).monospace().color(p.text_muted));

                                                // Col 3: LOCAL PORT (bold grey/white)
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.label(RichText::new(&sock.port).size(9.0).monospace().strong().color(p.text_primary));
                                                });

                                                // Col 4: STATUS (Pill badge)
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    let (st_rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 18.0), egui::Sense::hover());
                                                    ui.painter().rect_filled(st_rect, Rounding::same(3.0), accent_with_alpha(accent, 16));
                                                    ui.painter().text(st_rect.center(), egui::Align2::CENTER_CENTER, &sock.status, egui::FontId::monospace(8.0), p.text_secondary);
                                                });

                                                ui.end_row();
                                            }
                                        }
                                    });

                                ui.add_space(8.0);

                                // Inner Bottom Bar inside Right Card
                                ui.horizontal(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
                                        ui.painter().circle_filled(d_rect.center(), 2.0, accent);
                                        ui.label(RichText::new("Filter: TCP Listen").size(8.5).monospace().color(p.text_muted));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("Queue unavailable").size(8.5).monospace().color(p.text_muted));
                                    });
                                });
                            });


                    });
                });

                ui.add_space(10.0);

            });
    }
}
