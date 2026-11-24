use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_sensors_cpu_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        ScrollArea::vertical()
            .id_salt("sensors_cpu_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.render_sensor_hardware_access(ui, host_id);

                let brand = if !snap.cpu_brand.is_empty() {
                    &snap.cpu_brand
                } else {
                    "Processor model unavailable"
                };
                let phys = if snap.cpu_cores_physical > 0 { snap.cpu_cores_physical.to_string() } else { "—".into() };
                let log = if snap.cpu_cores_logical > 0 { snap.cpu_cores_logical.to_string() } else { "—".into() };

                // 1. Top Header Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical(|ui| {
                            // Row 1: CPU details on the left, reported limits on the right
                            ui.horizontal(|ui| {
                                let row_w = ui.available_width();
                                let right_w = 265.0_f32;
                                let left_w = (row_w - right_w - 8.0).max(80.0);

                                ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(RichText::new("CPU").size(15.0).strong().color(p.text_primary));
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    // Tjunction Max badge
                                    let tj_max = reported_sensor_value(snap.cpu_tjunction_max, 0);
                                    Frame::none()
                                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 8))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 3.5))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(format!("Tjunction Max: {}°C", tj_max)).size(9.5).monospace().color(p.text_muted));
                                        });

                                    ui.add_space(6.0);

                                    // Report only the power cap supplied by telemetry.
                                    Frame::none()
                                        .fill(accent_with_alpha(accent, 25))
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 3.5))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 5.0;
                                                let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(d_rect.center(), 2.8, accent);
                                                let pl1 = reported_sensor_value(snap.cpu_power_cap_watts, 1);
                                                ui.label(RichText::new(format!("Reported power cap: {} W", pl1)).size(9.5).monospace().strong().color(p.text_primary));
                                            });
                                        });
                                });
                            });

                            ui.add_space(3.0);

                            // Row 2: Subtitle spanning the full card width underneath Row 1 without overlap
                            let uptime_secs = if snap.uptime_secs > 0 { snap.uptime_secs } else if host_id.is_none() { get_system_uptime() } else { 0 };
                            let uptime_str = format_uptime_hms(uptime_secs);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(brand).size(10.0).monospace().color(p.text_muted));
                                ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                ui.label(RichText::new(format!("{} Cores / {} Threads", phys, log)).size(10.0).monospace().color(p.text_muted));
                                ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                ui.label(RichText::new(format!("Uptime {}", uptime_str)).size(10.0).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 2. Row of 4 Big Metric Cards (matching Overview page architecture)
                let hardware_timestamp = snap.hardware_history_timestamp();
                let readings = [
                    snap.cpu_usage_available.then_some(snap.cpu_global_usage).filter(|_| snap.timestamp.is_some()),
                    (snap.cpu_frequency_mhz > 0).then_some(snap.cpu_frequency_mhz as f32 / 1000.0),
                    snap.cpu_power_watts,
                    snap.cpu_temp,
                ];
                let labels = ["CPU LOAD", "CLOCK FREQUENCY", "PACKAGE POWER", "CPU TEMPERATURE"];
                let units = ["%", "GHz", "W", "°C"];
                let icons = [crate::lucide::ACTIVITY, crate::lucide::GAUGE, crate::lucide::ZAP, crate::lucide::THERMOMETER];
                let histories: Vec<_> = labels.iter().enumerate().map(|(index, label)|
                    sensor_metric_history(ui, host_id, label, if index >= 2 { hardware_timestamp } else { snap.timestamp }, readings[index])).collect();
                sensor_card_columns(ui, 4, |ui, column| {
                    let history = &histories[column];
                    let maximum = history.iter().copied().filter(|value| value.is_finite()).reduce(f32::max);
                    let subtitle = match column {
                        0 => format!("{} Cores / {} Threads", phys, log),
                        1 => "Reported clock".to_string(),
                        2 => snap.cpu_power_cap_watts.map(|value| format!("Reported power cap: {:.1} W", value)).unwrap_or_else(|| "Power cap unavailable".into()),
                        _ => snap.cpu_tjunction_max.map(|value| format!("Reported limit: {:.0}°C", value)).unwrap_or_else(|| "Temperature limit unavailable".into()),
                    };
                    let scale = if column == 0 { 100.0 } else { maximum.unwrap_or(1.0).max(1.0) * 1.1 };
                    super::overview::overview_metric(
                        ui, &p, accent, icons[column], labels[column], readings[column], units[column],
                        if readings[column].is_some() { &subtitle } else { "Unavailable from this collector" },
                        history, scale,
                    );
                });

                ui.add_space(8.0);

                // 3. Utilization History (60 Seconds) Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(crate::lucide::ACTIVITY).size(12.0).color(accent));
                            ui.label(RichText::new("Utilization").size(12.5).strong().color(p.text_primary));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                    ui.painter().circle_filled(d_rect.center(), 2.8, accent);
                                    ui.label(RichText::new("RECENT SAMPLES").size(10.0).monospace().strong().color(p.text_primary));
                                });
                            });
                        });
                        ui.add_space(8.0);
                        draw_waveform(ui, 140.0, &self.cpu_history, 100.0, accent, accent_with_alpha(accent, 45));

                        ui.add_space(4.0);
                        ui.columns(5, |cols| {
                            cols[0].label(RichText::new("Earlier").size(9.5).monospace().color(p.text_muted));
                            cols[1].centered_and_justified(|ui| { ui.label(RichText::new("").size(9.5).monospace().color(p.text_muted)); });
                            cols[2].centered_and_justified(|ui| { ui.label(RichText::new("").size(9.5).monospace().color(p.text_muted)); });
                            cols[3].centered_and_justified(|ui| { ui.label(RichText::new("").size(9.5).monospace().color(p.text_muted)); });
                            cols[4].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("Latest sample").size(9.5).monospace().strong().color(p.text_primary));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 4. Per-Core Breakdown Grid (4 Columns row matching Image)
                let num_cores = snap.cpu_core_usages.len();
                if num_cores == 0 { ui.label(RichText::new("Per-processor usage unavailable").size(11.0).color(p.text_muted)); }
                for chunk in (0..num_cores).collect::<Vec<_>>().chunks(4) {
                    ui.columns(chunk.len(), |cols| {
                        for (col_idx, &core_idx) in chunk.iter().enumerate() {
                            let col_ui = &mut cols[col_idx];
                            let usage = snap.cpu_core_usages[core_idx];
                            let freq_str = snap.cpu_frequencies_mhz.get(core_idx).filter(|value| **value > 0)
                                .map_or_else(|| "—".to_string(), |value| format!("{:.2}", *value as f64 / 1000.0));

                            let dot_color = if core_idx % 2 == 0 {
                                p.info // Soft Blue
                            } else {
                                accent // Cybernetic Cyan
                            };

                            Frame::none()
                                .fill(p.bg_card)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                .show(col_ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    // Header: Core X on left, Thread X on right
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        ui.label(RichText::new("●").size(8.0).color(dot_color));
                                        ui.label(RichText::new(format!("Logical CPU {}", core_idx)).size(11.5).strong().color(p.text_primary));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new("Processor").size(10.0).monospace().color(p.text_muted));
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // Clock GHz on left, Load % on right
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(&freq_str).size(26.0).strong().color(p.text_primary));
                                            ui.label(RichText::new("CLOCK (GHz)").size(9.0).monospace().color(p.text_muted));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                                ui.label(RichText::new(usage.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "—".into())).size(20.0).strong().color(p.text_primary));
                                                ui.label(RichText::new("LOAD").size(9.0).monospace().color(p.text_muted));
                                            });
                                        });
                                    });

                                    ui.add_space(6.0);

                                    // Smooth wave sparkline across bottom of card
                                    let hist = self.cpu_core_histories.get(core_idx).map(|q| q.iter().copied().collect::<Vec<_>>()).unwrap_or_default();

                                    let spark_w = ui.available_width();
                                    draw_core_waveform(ui, spark_w, 28.0, &hist, accent);
                                });
                        }
                    });
                    ui.add_space(6.0);
                }

                ui.add_space(4.0);

                // 5. Reported System Context
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(crate::lucide::SYSTEMD).size(11.0).color(accent));
                            ui.label(RichText::new("SYSTEM CONTEXT").size(10.5).monospace().strong().color(p.text_muted));
                        });

                        ui.add_space(8.0);

                        let process_count = snap.process_count.map(|count| count.to_string()).unwrap_or_else(|| "—".to_string());
                        let os_name = if snap.os_name.is_empty() { "—" } else { &snap.os_name };
                        let os_version = if snap.os_version.is_empty() { "Version unavailable" } else { &snap.os_version };
                        let uptime = if snap.uptime_secs > 0 { format_uptime(snap.uptime_secs) } else { "—".to_string() };
                        let values = [
                            ("PROCESSES", process_count.as_str(), "Visible processes"),
                            ("OPERATING SYSTEM", os_name, os_version),
                            ("UPTIME", uptime.as_str(), "Since last boot"),
                        ];
                        ui.columns(values.len(), |cols| {
                            for (column, (title, value, detail)) in cols.iter_mut().zip(values) {
                                column.vertical(|ui| {
                                    ui.label(RichText::new(title).size(9.5).monospace().strong().color(p.text_muted));
                                    ui.add_space(2.0);
                                    ui.label(RichText::new(value).size(15.0).strong().color(p.text_primary));
                                    ui.add_space(1.0);
                                    ui.label(RichText::new(detail).size(9.5).monospace().color(p.text_muted));
                                });
                            }
                        });
                    });

                ui.add_space(10.0);
            });
    }
}
