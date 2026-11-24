use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Power Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_power_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        // An initial empty snapshot is still loading, not evidence that the host has no battery.
        if snap.timestamp.is_none() {
            if host_id.is_none() {
                self.trigger_local_sensors_query(false, Some(ui.ctx().clone()));
            }
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading power and battery data…");
            });
            return;
        }
        #[cfg(windows)]
        if host_id.is_none() {
            let p = self.palette();
            let accent = self.sensors_accent_color(&p);
            ScrollArea::vertical().id_salt("windows_power_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.heading("Windows power and battery");
            if snap.power.batteries.is_empty() { ui.label("No battery telemetry is available."); }
            for (index, battery) in snap.power.batteries.iter().enumerate() {
                let duplicate_name = snap.power.batteries.iter().filter(|item| item.name == battery.name).count() > 1;
                let label = if duplicate_name { format!("{} · {}", battery.name, index + 1) } else { battery.name.clone() };
                let energy_label = if battery.energy_estimated { "Remaining energy (est.)" } else { "Remaining energy" };
                let metrics = [
                    ("Charge", battery.capacity_pct.map(|value| format!("{value:.0}%"))),
                    (energy_label, battery.energy_wh.map(|value| format!("{value:.1} Wh"))),
                    ("Full-charge capacity", battery.energy_full_wh.map(|value| format!("{value:.1} Wh"))),
                    ("Design capacity", battery.energy_design_wh.map(|value| format!("{value:.1} Wh"))),
                    ("Retention", battery_capacity_retention(battery).map(|value| format!("{value:.1}%"))),
                    ("Design voltage", battery.voltage_volts.map(|value| format!("{value:.2} V"))),
                ];
                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).size(12.0).strong().color(p.text_primary));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(&battery.status).size(10.0).color(p.text_secondary));
                                ui.label(RichText::new(crate::lucide::ZAP).size(15.0).color(accent));
                            });
                        });
                        ui.add_space(8.0);
                        sensor_card_columns(ui, metrics.len(), |ui, column| {
                            let (name, value) = &metrics[column];
                            ui.label(RichText::new(*name).size(9.5).color(p.text_muted));
                            ui.label(RichText::new(value.as_deref().unwrap_or("—")).size(13.0).monospace().color(p.text_primary));
                        });
                    });
                ui.add_space(6.0);
            }
            ui.label(RichText::new("Battery cycle count, charge limits, and measured package power are not reported by this Windows provider.").size(10.0).color(p.text_muted));
            render_animated_power_estimate(ui, snap, accent);
            if ui.button("Open Windows power settings").clicked() {
                let _ = std::process::Command::new("cmd.exe").args(["/D", "/C", "start", "", "ms-settings:powersleep"]).spawn();
            }
            });
            return;
        }

        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();

        ScrollArea::vertical()
            .id_salt("sensors_power_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let battery_id = egui::Id::new(("power_selected_battery", host_id));
                let mut selected = ui.ctx().data(|data| data.get_temp::<String>(battery_id))
                    .filter(|name| snap.power.batteries.iter().any(|battery| &battery.name == name))
                    .or_else(|| snap.power.batteries.first().map(|battery| battery.name.clone()));
                if snap.power.batteries.len() > 1 {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Battery").size(11.0).color(p.text_muted));
                        egui::ComboBox::from_id_salt(("power_battery_selector", host_id))
                            .selected_text(selected.as_deref().unwrap_or("Battery"))
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for battery in &snap.power.batteries {
                                    ui.selectable_value(&mut selected, Some(battery.name.clone()), &battery.name);
                                }
                            });
                    });
                    ui.add_space(6.0);
                }
                if let Some(name) = &selected { ui.ctx().data_mut(|data| data.insert_temp(battery_id, name.clone())); }
                let bat = selected.as_ref().and_then(|name| snap.power.batteries.iter().find(|battery| &battery.name == name));
                let has_battery = bat.is_some();
                let bat_pct = bat.and_then(|battery| battery.capacity_pct).unwrap_or(f32::NAN);
                let full_wh = bat.and_then(|battery| battery.energy_full_wh).unwrap_or(f32::NAN);
                let remaining_wh = bat.and_then(|battery| battery.energy_wh).unwrap_or(f32::NAN);
                let bat_health = bat.and_then(battery_capacity_retention).unwrap_or(f32::NAN);
                let reported_status = bat.map(|battery| battery.status.trim()).filter(|status| !status.is_empty()).unwrap_or("Battery state unavailable");
                let status_str = if has_battery { reported_status.to_string() } else {
                    match snap.power.ac_online { Some(true) => "External power connected", Some(false) => "External power disconnected", None => "Power-source status unavailable" }.to_string()
                };
                let est_time_str = if reported_status.eq_ignore_ascii_case("full") || reported_status.eq_ignore_ascii_case("fully charged") { "Fully charged" } else { "Unavailable" };

                // 1. Top Header Banner
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
                                ui.label(RichText::new("●").size(7.0).color(accent));
                                ui.label(RichText::new("POWER & BATTERY").size(10.5).monospace().strong().color(p.text_muted));
                            });

                            ui.add_space(2.0);

                            let title = if is_remote {
                                format!("Power & Battery · {}", self.host_display_name(host_id))
                            } else {
                                "Power & Battery".to_string()
                            };
                            ui.label(RichText::new(title).size(18.0).strong().color(p.text_primary));
                            ui.add_space(6.0);

                            // Row 3: Subtitle spanning the full card width underneath
                            ui.horizontal_wrapped(|ui| {
                                let sub_str = if !has_battery {
                                    if is_remote {
                                        format!("Reported power telemetry • Uptime {}", format_uptime(snap.uptime_secs))
                                    } else {
                                        "Reported power telemetry; battery information unavailable".to_string()
                                    }
                                } else {
                                    format!("Time remaining: {est_time_str}")
                                };
                                Frame::none().fill(p.bg_button).rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&status_str).size(10.5).monospace().color(p.text_secondary));
                                    });
                                Frame::none().fill(p.bg_button).rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(sub_str).size(10.5).monospace().color(p.text_secondary));
                                    });
                            });
                        });
                    });

                // Status message toast banner
                if let Some((ref msg, is_err, time)) = self.power_status_msg {
                    if time.elapsed().as_secs() < 6 {
                        ui.add_space(6.0);
                        let toast_col = if is_err { p.text_secondary } else { accent };
                        Frame::none()
                            .fill(toast_col.gamma_multiply(0.12))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let icon = if is_err { crate::lucide::ALERT_TRIANGLE } else { crate::lucide::CHECK };
                                    ui.label(RichText::new(icon).size(11.0).color(toast_col));
                                    ui.label(RichText::new(msg).size(9.0).monospace().strong().color(toast_col));
                                });
                            });
                    }
                }

                ui.add_space(8.0);

                // 2. Middle Row: Left Column (Power Source & Presets) + Right Column (Live Telemetry)
                let total_w = ui.available_width();
                let gap = 10.0_f32;
                let side_by_side = total_w >= 620.0;
                let left_w = if side_by_side { ((total_w - gap) * 0.64).floor() } else { total_w };
                let right_w = if side_by_side { (total_w - left_w - gap).max(0.0) } else { total_w };
                let panel_layout = if side_by_side { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };

                let num_cores = snap.cpu_cores_physical.max(snap.cpu_cores_logical).max(1);
                let profile = if is_remote {
                    crate::power::tdp_profile_from_nominal_or_cpu(snap.cpu_power_cap_watts, &snap.cpu_brand, num_cores)
                } else {
                    self.cpu_tdp_profile
                };
                // Desktop panels must share a row; wrapping can push the entire right panel below the tall left column.
                ui.allocate_ui_with_layout(egui::vec2(total_w, 0.0), panel_layout, |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Left Column
                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(left_w);
                        ui.set_max_width(left_w);
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        if has_battery {

                            // Primary Battery Telemetry Card (Local PC or Remote Laptop with detected battery)
                            Frame::none()
                                .fill(p.bg_card)
                                .rounding(Rounding::same(12.0))
                                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.label(RichText::new(crate::lucide::BATTERY).size(12.0).color(accent));
                                        let bat_name = bat.map(|b| b.name.as_str()).unwrap_or("Battery");
                                        let bat_mfg = bat.map(|b| b.manufacturer.trim()).filter(|s| !s.is_empty()).unwrap_or("Unknown manufacturer");
                                        let bat_model = bat.map(|b| b.model_name.trim()).filter(|s| !s.is_empty()).unwrap_or("Unknown model");
                                        let bat_hdr = format!("Battery · {} ({} {})", bat_name, bat_mfg, bat_model);
                                        ui.label(RichText::new(bat_hdr).size(10.0).monospace().strong().color(p.text_muted));

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            Frame::none()
                                                .fill(accent_with_alpha(accent, 20))
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(7.0, 2.5))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
                                                        ui.painter().circle_filled(d_rect.center(), 2.5, accent);
                                                        ui.label(RichText::new(if bat_health.is_finite() { format!("Capacity retained {}%", reported_sensor_value(Some(bat_health), 0)) } else { "Health unavailable".into() }).size(8.5).monospace().strong().color(p.text_primary));
                                                    });
                                                });
                                        });
                                    });

                                    ui.add_space(10.0);

                                    const BATTERY_TILE_HEIGHT: f32 = 132.0;

                                    let render_card_meta_row = |ui: &mut egui::Ui, label: &str, val: &str, val_col: Color32| {
                                        ui.horizontal(|ui| {
                                            let avail = ui.available_width();
                                            let left_w = (avail * 0.40).clamp(28.0, 55.0);
                                            let right_w = (avail - left_w - ui.spacing().item_spacing.x).max(0.0);
                                            ui.allocate_ui_with_layout(egui::vec2(left_w, 14.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(RichText::new(label).size(8.5).monospace().color(p.text_muted)).truncate());
                                            });
                                            ui.allocate_ui_with_layout(egui::vec2(right_w, 14.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.add(egui::Label::new(RichText::new(val).size(8.5).monospace().strong().color(val_col)).truncate());
                                            });
                                        });
                                    };

                                    ui.columns(3, |cols| {
                                        // Card 1: State of Charge
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                            .show(&mut cols[0], |ui| {
                                                ui.set_width(ui.available_width());

                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    ui.label(RichText::new(crate::lucide::BATTERY_CHARGING).size(11.0).color(accent));
                                                    ui.label(RichText::new("Charge").size(9.5).monospace().color(p.text_muted));
                                                });
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 2.0;
                                                    ui.label(RichText::new(format!("{}", reported_sensor_value(Some(bat_pct), 0))).size(28.0).strong().color(p.text_primary));
                                                    if bat_pct.is_finite() { ui.label(RichText::new("%").size(12.0).color(p.text_muted)); }
                                                });
                                                ui.add_space(8.0);
                                                let (bar_r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                                                ui.painter().rect_filled(bar_r, Rounding::same(2.0), p.bg_subtle);
                                                if bat_pct.is_finite() {
                                                    let fill_w = bar_r.width() * (bat_pct / 100.0).clamp(0.0, 1.0);
                                                    ui.painter().rect_filled(egui::Rect::from_min_size(bar_r.min, egui::vec2(fill_w, bar_r.height())), Rounding::same(2.0), accent);
                                                }
                                                ui.add_space(8.0);

                                                let delivery_str = reported_status;
                                                render_card_meta_row(ui, "Mode", delivery_str, p.text_secondary);
                                                ui.add_space(3.0);
                                                render_card_meta_row(ui, "Time", &est_time_str, accent);

                                                ui.set_min_height(BATTERY_TILE_HEIGHT);
                                            });

                                        // Card 2: Remaining vs Design
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                            .show(&mut cols[1], |ui| {
                                                ui.set_width(ui.available_width());

                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    ui.label(RichText::new(crate::lucide::ZAP).size(11.0).color(accent));
                                                    let estimated = bat.is_some_and(|battery| battery.energy_estimated);
                                                    ui.label(RichText::new(if estimated { "Energy (est.)" } else { "Battery energy" }).size(9.5).monospace().color(p.text_muted))
                                                        .on_hover_text(if estimated { "One or more energy values are estimated from reported charge and design voltage." } else { "Energy values reported by the battery, when available." });
                                                });
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 2.0;
                                                    ui.label(RichText::new(format!("{}", reported_sensor_value(Some(remaining_wh), 1))).size(28.0).strong().color(p.text_primary));
                                                    ui.label(RichText::new(format!("/ {} Wh", reported_sensor_value(Some(full_wh), 1))).size(11.0).color(p.text_muted));
                                                });
                                                ui.add_space(8.0);
                                                let (bar_r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                                                ui.painter().rect_filled(bar_r, Rounding::same(2.0), p.bg_subtle);
                                                if full_wh.is_finite() && full_wh > 0.0 && remaining_wh.is_finite() && remaining_wh >= 0.0 {
                                                    let cap_ratio = (remaining_wh / full_wh).clamp(0.0, 1.0);
                                                    ui.painter().rect_filled(egui::Rect::from_min_size(bar_r.min, egui::vec2(bar_r.width() * cap_ratio, bar_r.height())), Rounding::same(2.0), accent);
                                                }
                                                ui.add_space(8.0);

                                                let design_wh = bat.and_then(|b| b.energy_design_wh).unwrap_or(f32::NAN);
                                                render_card_meta_row(ui, "Retention", &format!("{}%", reported_sensor_value(Some(bat_health), 1)), p.text_secondary);
                                                ui.add_space(3.0);
                                                render_card_meta_row(ui, "Design", &format!("{} Wh", reported_sensor_value(Some(design_wh), 1)), p.text_muted);

                                                ui.set_min_height(BATTERY_TILE_HEIGHT);
                                            });

                                        // Card 3: Lifecycle Endurance
                                        Frame::none()
                                            .fill(p.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                            .show(&mut cols[2], |ui| {
                                                ui.set_width(ui.available_width());

                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    ui.label(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(accent));
                                                    ui.label(RichText::new("Charge cycles").size(9.5).monospace().color(p.text_muted));
                                                });
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 2.0;
                                                    if let Some(cycles) = bat.and_then(|battery| battery.cycle_count) {
                                                        ui.label(RichText::new(format!("{}", cycles)).size(28.0).strong().color(p.text_primary));
                                                        ui.label(RichText::new("cycles").size(11.0).color(p.text_muted));
                                                    } else {
                                                        ui.label(RichText::new("—").size(28.0).strong().color(p.text_primary));
                                                        ui.label(RichText::new("unavailable").size(11.0).color(p.text_muted));
                                                    }
                                                });
                                                let cond_label = bat.map(|battery| battery.status.as_str()).unwrap_or("Unavailable");
                                                render_card_meta_row(ui, "State", cond_label, p.text_muted);

                                                ui.set_min_height(BATTERY_TILE_HEIGHT);
                                            });
                                    });

                                    ui.add_space(8.0);

                                    // Row of 4 Sub-chips (Fits cleanly on 1 line without wrapping)
                                    ui.columns(4, |cols| {
                                        let volts_val = bat.and_then(|b| b.voltage_volts).unwrap_or(f32::NAN);
                                        let chem_str = bat.map(|b| b.technology.trim()).filter(|s| !s.is_empty()).unwrap_or("Unavailable");

                                        let ac_status = match snap.power.ac_online { Some(true) => "Connected", Some(false) => "Disconnected", None => "Unavailable" };
                                        let chips = [
                                            ("Voltage", format!("{} V", reported_sensor_value(Some(volts_val), 2)), p.text_primary),
                                            ("Battery chemistry", chem_str.to_string(), p.text_primary),
                                            ("CPU Temp", format!("{} °C", reported_sensor_value(snap.cpu_temp, 1)), p.text_secondary),
                                            ("AC adapter", ac_status.to_string(), p.text_secondary),
                                        ];
                                        for (i, (title, val, val_col)) in chips.into_iter().enumerate() {
                                            Frame::none()
                                                .fill(p.bg_input)
                                                .rounding(Rounding::same(6.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                                .show(&mut cols[i], |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.label(RichText::new(title).size(8.0).monospace().color(p.text_muted));
                                                    ui.add_space(2.0);
                                                    ui.label(RichText::new(val).size(9.5).monospace().strong().color(val_col));
                                                });
                                        }
                                    });
                                });
                                                } else {
                            ui.label(RichText::new("No battery information was reported by this host.").size(11.0).color(p.text_muted));
                        }
                        ui.add_space(8.0);


                    });

                    ui.add_space(gap);

                    let cpu_load = (snap.cpu_global_usage / 100.0).clamp(0.0, 1.0);
                    let raw_cpu = snap.cpu_power_watts.unwrap_or_else(|| {
                        let idle = (profile.nominal_watts * 0.12).clamp(1.5, 20.0);
                        idle + (profile.nominal_watts - idle) * (0.35 * cpu_load + 0.65 * cpu_load.powf(1.35))
                    });
                    // Right Column: ESTIMATED POWER
                    ui.allocate_ui_with_layout(egui::vec2(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(right_w);
                        ui.set_max_width(right_w);
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label(RichText::new(crate::lucide::ACTIVITY).size(12.0).color(accent));
                                    ui.label(RichText::new("ESTIMATED POWER").size(10.0).monospace().strong().color(p.text_muted));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                        ui.painter().circle_filled(d_rect.center(), 3.0, accent);
                                    });
                                });

                                ui.add_space(10.0);

                                // User-requested component power model; allocations are estimates.
                                let disk_mb_s = ((snap.total_disk_read_rate_bps + snap.total_disk_write_rate_bps) / (1024.0 * 1024.0)) as f32;
                                let net_mb_s = ((snap.total_rx_rate_bps + snap.total_tx_rate_bps) / (1024.0 * 1024.0)) as f32;
                                let mem_ratio = (snap.mem_usage_pct / 100.0).clamp(0.0, 1.0);

                                let is_discharging = all_batteries_have_status(&snap.power.batteries, "discharging");
                                let is_charging = all_batteries_have_status(&snap.power.batteries, "charging");

                                let (badge_str, badge_col) = ("Estimated model".to_string(), accent);

                                // GPU telemetry
                                let (raw_gpu, gpu_cap) = if !snap.gpus.is_empty() {
                                    let mut total_g_w = 0.0_f32;
                                    let mut total_g_cap = 0.0_f32;
                                    for g in &snap.gpus {
                                        let limit = g.power_limit_watts.unwrap_or(if is_remote { 75.0 } else { 35.0 });
                                        let g_load = (g.load_pct.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
                                        let w = g.power_watts.unwrap_or_else(|| {
                                            (limit * 0.08).max(0.5) + (limit * 0.92) * g_load
                                        });
                                        total_g_w += w;
                                        total_g_cap += limit.max(w * 1.25);
                                    }
                                    (total_g_w, total_g_cap.max(total_g_w * 1.25))
                                } else {
                                    let g_tdp = (profile.nominal_watts * 0.30).clamp(3.0, 35.0);
                                    // CPU utilization is not a proxy for integrated-GPU activity.
                                    let igpu_w = (g_tdp * 0.06).max(0.3);
                                    (igpu_w, g_tdp)
                                };

                                // Storage subsystem
                                let (num_rotational, num_ssd) = {
                                    let mut rot = 0;
                                    let mut ssd = 0;
                                    for d in &snap.physical_disks {
                                        match d.is_ssd { Some(true) => ssd += 1, Some(false) => rot += 1, None => {} }
                                    }
                                    (rot, ssd)
                                };
                                let raw_storage = (num_rotational as f32 * 2.2 + num_ssd as f32 * 0.5 + (disk_mb_s * 0.04).clamp(0.0, 4.0)).clamp(0.0, 50.0);
                                let storage_cap = (num_rotational as f32 * 7.0 + num_ssd as f32 * 3.5).max(raw_storage).clamp(0.1, 60.0);

                                // Motherboard chipset & memory
                                let mem_gb = (snap.mem_total_bytes as f32 / (1024.0 * 1024.0 * 1024.0)).max(4.0);
                                let ram_draw = (mem_gb / 8.0) * (0.35 + mem_ratio * 0.55);
                                let chipset_draw = if has_battery { 1.0 } else if num_cores >= 16 { 5.0 } else { 2.2 };
                                let raw_mb = (chipset_draw + ram_draw).clamp(1.2, 35.0);
                                let mb_cap = (chipset_draw * 2.0 + (mem_gb / 8.0) * 1.8).clamp(6.0, 40.0);

                                // Network PHY & bus
                                let raw_net = (0.4 + (net_mb_s * 0.06)).clamp(0.4, 20.0);
                                let net_cap = 15.0_f32;

                                // USB & Peripherals
                                let (base_usb_w, usb_device_count) = snap.power.usb_power_watts.unwrap_or((0.45, 0));
                                let usb_cap = (usb_device_count as f32 * 4.5 + 5.0).clamp(8.0, 30.0);

                                // CPU cap
                                let cpu_cap = profile.turbo_pl2_watts.max(raw_cpu * 1.25);

                                let (watts_val, components): (f32, Vec<(&'static str, f32, f32)>) = if has_battery {
                                    let br = snap.power.backlight_ratio.unwrap_or(0.40);
                                    let raw_display = 1.0 + br * 2.8;
                                    let disp_cap = 6.0_f32;
                                    let raw_kbd_lights = snap.power.kbd_lights_watts.unwrap_or(0.25);
                                    let kbd_cap = 4.0_f32;

                                    let bat_power = snap.power.total_power_watts.filter(|&w| w > 0.1);

                                    let (final_watts, c_cpu, c_gpu, c_disp, c_storage, c_usb, c_kbd_lights, c_mb) = if is_discharging && bat_power.is_some() {
                                        let total = bat_power.unwrap();
                                        let base_internal = raw_cpu + raw_gpu + raw_display + raw_storage + raw_kbd_lights + raw_mb + base_usb_w;
                                        let c_usb_raw = if total > base_internal {
                                            base_usb_w.max(total - base_internal)
                                        } else {
                                            base_usb_w
                                        };
                                        let non_cpu_sum = (raw_display + raw_storage + c_usb_raw + raw_kbd_lights + raw_mb).max(0.1);
                                        let target_non_cpu = (total - raw_cpu - raw_gpu).max(0.1);
                                        let scale = (target_non_cpu / non_cpu_sum).clamp(0.4, 1.8);

                                        (
                                            total,
                                            raw_cpu,
                                            raw_gpu,
                                            raw_display * scale,
                                            raw_storage * scale,
                                            c_usb_raw * scale,
                                            raw_kbd_lights * scale,
                                            raw_mb * scale,
                                        )
                                    } else if is_charging && bat_power.is_some() {
                                        let sys_draw = raw_cpu + raw_gpu + raw_display + raw_storage + base_usb_w + raw_kbd_lights + raw_mb;
                                        let total = sys_draw + bat_power.unwrap();
                                        (total, raw_cpu, raw_gpu, raw_display, raw_storage, base_usb_w, raw_kbd_lights, raw_mb)
                                    } else {
                                        let total = (raw_cpu + raw_gpu + raw_display + raw_storage + base_usb_w + raw_kbd_lights + raw_mb).max(2.5);
                                        (total, raw_cpu, raw_gpu, raw_display, raw_storage, base_usb_w, raw_kbd_lights, raw_mb)
                                    };

                                    let mut comps = vec![
                                        ("CPU Package (PL0/1)", c_cpu, cpu_cap),
                                    ];
                                    if !snap.gpus.is_empty() {
                                        comps.push(("Graphics Subsystem (GPU)", c_gpu, gpu_cap));
                                    } else {
                                        comps.push(("Integrated GPU (iGPU)", c_gpu, gpu_cap));
                                    }
                                    comps.push(("eDP Panel & Backlight", c_disp, disp_cap));
                                    comps.push(("Drives & Storage (HDD/NVMe)", c_storage, storage_cap));
                                    comps.push(("USB Ports & Peripherals", c_usb, usb_cap));
                                    comps.push(("Keyboard & System Lights", c_kbd_lights, kbd_cap));
                                    comps.push(("Motherboard Logic & RAM", c_mb, mb_cap));

                                    (final_watts, comps)
                                } else {
                                    let total = raw_cpu + (if !snap.gpus.is_empty() { raw_gpu } else { 0.0 }) + raw_storage + raw_net + raw_mb + base_usb_w;

                                    let mut comps = vec![
                                        ("CPU Package & Cores", raw_cpu, cpu_cap),
                                    ];
                                    if !snap.gpus.is_empty() {
                                        comps.push(("Graphics Subsystem (GPU)", raw_gpu, gpu_cap));
                                    }
                                    comps.push(("Storage Subsystem (HDD/NVMe)", raw_storage, storage_cap));
                                    comps.push(("Network PHY & Interface", raw_net, net_cap));
                                    comps.push(("Motherboard Logic & RAM", raw_mb, mb_cap));
                                    comps.push(("USB & System Peripherals", base_usb_w, usb_cap));

                                    (total, comps)
                                };

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("System draw").size(9.5).monospace().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        Frame::none()
                                            .fill(p.bg_subtle)
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(&badge_str).size(8.5).monospace().color(badge_col));
                                            });
                                    });
                                });

                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.label(RichText::new(format!("{:.1}", watts_val)).size(32.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("W").size(12.0).color(p.text_muted));
                                });

                                ui.add_space(8.0);

                                // Smooth realtime animated waveform canvas
                                draw_estimated_power_waveform(ui, 68.0, watts_val, accent);
                                ui.label(RichText::new("The animated chart and component values show model estimates.").size(11.0).color(p.text_muted));

                                ui.add_space(10.0);

                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Component estimates").size(12.0).strong().color(p.text_muted));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("ESTIMATED").size(7.5).monospace().color(p.text_muted));
                                    });
                                });
                                ui.add_space(4.0);

                                for &(c_name, c_watts, c_max) in &components {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(c_name).size(8.5).monospace().color(p.text_secondary));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(format!("{:.1} W", c_watts)).size(8.5).monospace().strong().color(p.text_primary));
                                        });
                                    });
                                    ui.add_space(2.0);
                                    let (br, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                                    ui.painter().rect_filled(br, Rounding::same(2.0), p.bg_input);
                                    let fill_w = br.width() * (c_watts / c_max).clamp(0.04, 1.0);
                                    ui.painter().rect_filled(egui::Rect::from_min_size(br.min, egui::vec2(fill_w, br.height())), Rounding::same(2.0), accent);
                                    ui.add_space(3.0);
                                }
                            });
                    });
                });

                ui.add_space(8.0);

                if has_battery {
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("BATTERY CHARGE LIMIT").size(10.0).monospace().strong().color(p.text_secondary));
                            ui.add_space(6.0);
                            if snap.power.batteries.len() != 1 {
                                ui.label(RichText::new("Charge control is unavailable for multiple batteries.").size(11.0).color(p.text_muted));
                            } else if let Some(limit) = snap.power.charge_limit {
                                let battery = &snap.power.batteries[0].name;
                                ui.label(RichText::new(format!("{} · Current limit: {}%", battery, limit)).size(11.0).color(p.text_secondary));
                                ui.horizontal(|ui| {
                                    ui.add(egui::Slider::new(&mut self.power_ceiling, 1.0..=100.0).integer().suffix("%"));
                                    if ui.button("Apply limit").clicked() {
                                        let host = host_id.and_then(|id| self.ssh_hosts.iter().find(|h| h.id == id)).cloned();
                                        if host_id.is_none() || host.is_some() {
                                            self.power_status_msg = Some(("Applying charge limit…".into(), false, Instant::now()));
                                            self.queue_systemd_action(PendingSudoAction::SetChargeLimit { battery: battery.clone(), limit: self.power_ceiling as u32 }, host.as_ref(), None);
                                        } else {
                                            self.power_status_msg = Some(("Selected host is unavailable".into(), true, Instant::now()));
                                        }
                                    }
                                });
                            } else {
                                ui.label(RichText::new("Charge-limit control is not reported by this battery.").size(11.0).color(p.text_muted));
                            }
                            ui.add_space(6.0);
                            ui.label(RichText::new("Rapid charging and USB standby controls are unavailable pending hardware support checks.").size(10.0).color(p.text_muted));
                        });
                }

                ui.add_space(10.0);
            });
    }
}
