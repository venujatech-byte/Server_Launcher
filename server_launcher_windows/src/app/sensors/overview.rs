use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Overview Dashboard (Matching media_1792121759405_52cfc968.png)
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_overview_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        ScrollArea::vertical()
            .id_salt("sensors_overview_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let mut assigned_mounts = std::collections::HashSet::new();
                let mut storage: Vec<_> = snap.physical_disks.iter().enumerate().map(|(index, disk)| {
                    let mounts: Vec<_> = snap.disks.iter().enumerate().filter_map(|(mount_index, mount)| {
                        (disk_owns_mount(&disk.name, &disk.partitions, &mount.name) && assigned_mounts.insert(mount_index))
                            .then(|| mount.clone())
                    }).collect();
                    let (total, used, _) = mounted_storage_totals(&mounts);
                    (format!("DISK {}", index + 1), disk.name.clone(), total, used)
                }).collect();
                let mut seen_mounts = std::collections::HashSet::new();
                for (index, mount) in snap.disks.iter().enumerate() {
                    if !assigned_mounts.contains(&index) && seen_mounts.insert(&mount.name) {
                        storage.push(("MOUNTED STORAGE".to_owned(), mount.mount_point.clone(), mount.total_bytes, mount.used_bytes));
                    }
                }
                if storage.is_empty() {
                    storage.push(("MOUNTED STORAGE".to_owned(), "No volumes reported".to_owned(), 0, 0));
                }

                sensor_card_columns(ui, 3 + storage.len(), |ui, column| match column {
                    0 => {
                        let mut details = Vec::new();
                        if snap.cpu_cores_physical > 0 {
                            details.push(format!("{} cores", snap.cpu_cores_physical));
                        }
                        if snap.cpu_frequency_mhz > 0 {
                            details.push(format!("{:.2} GHz", snap.cpu_frequency_mhz as f64 / 1000.0));
                        }
                        let detail = if details.is_empty() { "CPU details unavailable".into() } else { details.join(" · ") };
                        let load = snap.cpu_usage_available.then_some(snap.cpu_global_usage);
                        let history: Vec<_> = self.cpu_history.iter().copied().collect();
                        overview_metric(ui, &p, accent, crate::lucide::ACTIVITY, "CPU", load, "%", &detail, &history, 100.0);
                    }
                    1 => {
                        let load = (snap.mem_total_bytes > 0).then_some(snap.mem_usage_pct);
                        let detail = if load.is_some() {
                            format!("{} / {}", crate::sensors::format_bytes(snap.mem_used_bytes), crate::sensors::format_bytes(snap.mem_total_bytes))
                        } else {
                            "Memory telemetry unavailable".into()
                        };
                        let history: Vec<_> = self.ram_history.iter().map(|(ram, _)| *ram).collect();
                        overview_metric(ui, &p, accent, crate::lucide::RAM, "RAM", load, "%", &detail, &history, 100.0);
                    }
                    2 => {
                        let gpu = snap.gpus.get(self.selected_gpu_idx).or_else(|| snap.gpus.first());
                        let (load, engine_peak) = gpu_display_load(gpu);
                        let history = sensor_metric_history(ui, host_id, &format!("overview:gpu:{gpu_index}:{device}:{engine_peak}", gpu_index = self.selected_gpu_idx, device = gpu.map_or("", |gpu| if gpu.device_path.is_empty() { &gpu.name } else { &gpu.device_path })), snap.hardware_history_timestamp(), load);
                        overview_metric(ui, &p, accent, crate::lucide::GPU, "GPU", load, "%", gpu.map_or("Adapter unavailable", |gpu| gpu.name.as_str()), &history, 100.0);
                    }
                    index => {
                        let (title, detail, total, used) = &storage[index - 3];
                        let usage = (*total > 0).then(|| (*used as f64 / *total as f64 * 100.0) as f32);
                        let history = sensor_metric_history(ui, host_id, &format!("overview:storage:{title}:{detail}"), snap.timestamp, usage);
                        let capacity = if *total > 0 {
                            format!("{} / {}", crate::sensors::format_bytes(*used), crate::sensors::format_bytes(*total))
                        } else {
                            "Mounted usage unavailable".to_owned()
                        };
                        overview_metric(ui, &p, accent, crate::lucide::STORAGE, title, usage, "%", &format!("{capacity} · {detail}"), &history, 100.0);
                    }
                });

                ui.add_space(10.0);
                sensor_card_columns(ui, 3, |ui, column| match column {
                    0 => {
                        let battery = snap.power.total_power_watts.filter(|watts| watts.is_finite() && *watts >= 0.0);
                        let (power, source) = if battery.is_some() {
                            (battery, "Battery power")
                        } else {
                            (snap.cpu_power_watts.filter(|watts| watts.is_finite() && *watts >= 0.0), "CPU package power")
                        };
                        let history = sensor_metric_history(ui, host_id, source, snap.timestamp, power);
                        let scale = history.iter().copied().fold(1.0_f32, f32::max);
                        overview_metric(ui, &p, accent, crate::lucide::ZAP, "POWER", power, "W", source, &history, scale);
                    }
                    1 => {
                        let temperature = snap.temp_sensors.iter().map(|sensor| sensor.temp)
                            .chain([snap.cpu_temp, snap.ram_temp, snap.storage_temp].into_iter().flatten())
                            .chain(snap.gpus.iter().filter_map(|gpu| gpu.temp))
                            .filter(|value| value.is_finite()).reduce(f32::max);
                        let history = sensor_metric_history(ui, host_id, "overview:temperature", snap.timestamp, temperature);
                        overview_metric(ui, &p, accent, crate::lucide::THERMOMETER, "OVERALL TEMPERATURE", temperature, "°C", "Hottest reported sensor", &history, 120.0);
                    }
                    _ => {
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(18.0, 15.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new("NETWORK SPEED").size(11.0).monospace().strong().color(p.text_muted));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("↑ {} up", sensor_network_rate(snap, snap.total_tx_rate_bps)));
                                    ui.label(format!("↓ {} down", sensor_network_rate(snap, snap.total_rx_rate_bps)));
                                });
                                ui.add_space(6.0);
                                draw_dual_network_chart(ui, 74.0, &self.net_tx_history, &self.net_rx_history, self.net_peak_mb, accent);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(format!("{} sent", sensor_network_bytes(snap, snap.total_tx_bytes))).size(10.0).color(p.text_muted));
                                    ui.label(RichText::new(format!("{} received", sensor_network_bytes(snap, snap.total_rx_bytes))).size(10.0).color(p.text_muted));
                                });
                            });
                    }
                });
                ui.add_space(10.0);
            });
    }
}

fn overview_metric(
    ui: &mut Ui, palette: &crate::theme::ThemePalette, accent: Color32,
    icon: &str, title: &str, value: Option<f32>, unit: &str, detail: &str,
    history: &[f32], scale: f32,
) {
    Frame::none()
        .fill(palette.bg_card)
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::symmetric(18.0, 15.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(15.0).color(accent));
                ui.add(egui::Label::new(RichText::new(title).size(11.0).monospace().strong().color(palette.text_muted)).truncate()).on_hover_text(title);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(reported_sensor_value(value, 1)).size(34.0).strong().color(palette.text_primary));
                if value.is_some() {
                    ui.label(RichText::new(unit).size(14.0).strong().color(palette.text_muted));
                }
            });
            ui.add(egui::Label::new(RichText::new(detail).size(11.0).monospace().color(palette.text_secondary)).truncate()).on_hover_text(detail);
            ui.add_space(5.0);
            let (sum, count, peak) = history.iter().copied().filter(|v| v.is_finite())
                .fold((0.0, 0, f32::NEG_INFINITY), |(sum, count, peak), v| (sum + v, count + 1, peak.max(v)));
            let stat = |label, value| {
                let reading = if count > 0 { format!("{value:.0}{unit}") } else { "—".into() };
                RichText::new(format!("{label} {reading}")).size(9.5).monospace().color(palette.text_muted)
            };
            ui.horizontal(|ui| {
                ui.label(stat("AVG", sum / count.max(1) as f32));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(stat("PEAK", peak));
                });
            });
            ui.add_space(5.0);
            draw_card_sparkline(ui, 44.0, history, scale, None, accent);
        });
}
