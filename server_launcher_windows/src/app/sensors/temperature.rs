use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Temperature Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_temperature_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();

        let cpu_temp = snap.cpu_temp
            .or_else(|| hottest_temperature_for_category(&snap.temp_sensors, crate::sensors::SensorCategory::Cpu))
            .unwrap_or(f32::NAN);

        let selected_gpu_temp = snap.gpus.get(self.selected_gpu_idx).and_then(|gpu| gpu.temp);
        let gpu_temp = selected_gpu_temp.or_else(|| {
            (snap.gpus.len() == 1).then(|| hottest_temperature_for_category(&snap.temp_sensors, crate::sensors::SensorCategory::Gpu)).flatten()
        }).unwrap_or(f32::NAN);

        let ram_temp = snap.ram_temp
            .or_else(|| hottest_temperature_for_category(&snap.temp_sensors, crate::sensors::SensorCategory::Memory))
            .unwrap_or(f32::NAN);

        let nvme_temp = snap.storage_temp
            .or_else(|| hottest_temperature_for_category(&snap.temp_sensors, crate::sensors::SensorCategory::Storage))
            .unwrap_or(f32::NAN);

        let vrm_temp = snap.temp_sensors.iter()
            .find(|s| s.name.to_lowercase().contains("vrm") || s.name.to_lowercase().contains("motherboard") || s.name.to_lowercase().contains("pch"))
            .map(|s| s.temp)
            .unwrap_or(f32::NAN);

        let temperature_text = |value: f32| if value.is_finite() { format!("{value:.1}") } else { "—".to_string() };
        let selected_gpu_identity = snap.gpus.get(self.selected_gpu_idx).map(|gpu| {
            if gpu.device_path.is_empty() { gpu.name.as_str() } else { gpu.device_path.as_str() }
        }).unwrap_or("unavailable");
        let history_id = egui::Id::new(("thermal_measured_history", host_id, self.selected_gpu_idx, selected_gpu_identity));
        let hardware_timestamp = snap.hardware_history_timestamp();
        let history = ui.ctx().data_mut(|data| {
            let history = data.get_temp_mut_or_default::<VecDeque<(Instant, f32, f32)>>(history_id);
            if let Some(timestamp) = hardware_timestamp {
                if history.back().map(|sample| sample.0) != Some(timestamp) {
                    history.push_back((timestamp, cpu_temp, gpu_temp));
                }
                while history.front().is_some_and(|sample| timestamp.saturating_duration_since(sample.0).as_secs_f32() > 60.0) || history.len() > 120 {
                    history.pop_front();
                }
            }
            debug_assert!(history.len() <= 120);
            history.clone()
        });
        let cpu_peak = history.iter().map(|sample| sample.1).filter(|value| value.is_finite()).reduce(f32::max);
        let gpu_peak = history.iter().map(|sample| sample.2).filter(|value| value.is_finite()).reduce(f32::max);
        let sensor_limit = |category, value| matching_temperature_limit(&snap.temp_sensors, category, value).unwrap_or(f32::NAN);

        ScrollArea::vertical()
            .id_salt("sensors_temp_scroll_v2")
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

                        let tag_str = if is_remote {
                            "THERMAL SUBSYSTEM • REMOTE"
                        } else {
                            "THERMAL SUBSYSTEM"
                        };
                        let title_str = if is_remote {
                            format!("Hardware Thermal Sensors & Cooling • Host '{}'", self.host_display_name(host_id))
                        } else {
                            "Hardware Thermal Sensors".to_string()
                        };
                        let sub_str = if is_remote {
                            "Reported hardware temperatures • cooling controls unavailable"
                        } else {
                            "Reported hardware temperatures • cooling controls unavailable"
                        };

                        ui.vertical(|ui| {
                            // Row 1: Overline / Subsystem Tag
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(crate::lucide::THERMOMETER).size(15.0).color(accent));
                                ui.label(RichText::new(tag_str).size(10.5).monospace().strong().color(p.text_muted));
                            });

                            ui.add_space(2.0);

                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.label(RichText::new(&title_str).size(18.0).strong().color(p.text_primary));
                                Frame::none()
                                    .fill(accent_with_alpha(accent, 25))
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        let badge = if [cpu_temp, gpu_temp, ram_temp, nvme_temp, vrm_temp].iter().any(|value| value.is_finite()) { "SENSORS" } else { "NO READINGS" };
                                        ui.label(RichText::new(badge).size(8.5).monospace().strong().color(p.text_primary));
                                    });
                            });

                            ui.add_space(4.0);

                            // Row 3: Full-width Subtitle spanning card width underneath Row 2 (Never collides or overlaps)
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(sub_str).size(10.5).color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 2. Row of 5 Sensor Cards (CPU, GPU/Ambient, RAM, Storage, VRM)
                let has_gpu = gpu_temp.is_finite();
                use crate::sensors::SensorCategory;
                let sensor_cards = [
                    ("CPU TEMPERATURE", cpu_temp, sensor_limit(SensorCategory::Cpu, cpu_temp), "", accent),
                    ("GPU TEMPERATURE", gpu_temp, if snap.gpus.get(self.selected_gpu_idx).and_then(|gpu| gpu.temp).is_some() { f32::NAN } else { sensor_limit(SensorCategory::Gpu, gpu_temp) }, "", p.info),
                    ("MEMORY", ram_temp, sensor_limit(SensorCategory::Memory, ram_temp), "", accent),
                    ("STORAGE", nvme_temp, sensor_limit(SensorCategory::Storage, nvme_temp), "", p.info),
                    ("BOARD SENSOR", vrm_temp, sensor_limit(SensorCategory::System, vrm_temp), "", p.text_secondary),
                ];

                let render_card = |ui: &mut Ui, i: usize| {
                    let (name, val, limit, _delta, accent) = &sensor_cards[i];
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(*name).size(9.5).monospace().strong().color(p.text_muted));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(temperature_text(*val)).size(30.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("°C").size(14.0).color(p.text_muted));

                                });
                                ui.add_space(6.0);
                                if val.is_finite() && limit.is_finite() && *limit > 0.0 {
                                    let pct = (val / limit).clamp(0.0, 1.0);
                                    let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.5), egui::Sense::hover());
                                    ui.painter().rect_filled(bar_rect, Rounding::same(2.0), p.bg_button);
                                    let mut filled_rect = bar_rect;
                                    filled_rect.set_width(bar_rect.width() * pct);
                                    ui.painter().rect_filled(filled_rect, Rounding::same(2.0), *accent);
                                }
                                ui.label(RichText::new(if limit.is_finite() { format!("LIMIT: {limit:.1}°C") } else { "Limit unavailable".to_string() }).size(8.5).monospace().color(p.text_muted));
                            });
                };
                let count = sensor_cards.len();
                if count <= 5 && sensor_grid_columns(ui.available_width(), ui.spacing().item_spacing.x, count) == count {
                    ui.columns(count, |columns| {
                        for (index, column) in columns.iter_mut().enumerate() {
                            render_card(column, index);
                        }
                    });
                } else {
                    sensor_card_columns(ui, count, render_card);
                }

                ui.add_space(8.0);

                // 3. 60-Second Thermal History Waveform Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Temperature history").size(14.0).strong().color(p.text_primary));
                                let sub_trace = if !has_gpu {
                                    "CPU temperature • GPU unavailable".to_string()
                                } else {
                                    "CPU and GPU temperatures • measured samples".to_string()
                                };
                                ui.label(RichText::new(sub_trace).size(9.5).monospace().color(p.text_muted));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 14.0;
                                    // Second channel Indicator
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                        ui.painter().circle_filled(d_rect.center(), 3.5, p.text_muted);
                                        let second_lbl = if !has_gpu {
                                            "GPU —".to_string()
                                        } else {
                                            format!("GPU peak {}°C", temperature_text(gpu_peak.unwrap_or(f32::NAN)))
                                        };
                                        ui.label(RichText::new(second_lbl).size(10.0).monospace().color(p.text_muted));
                                    });

                                    // CPU Indicator
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let (d_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                        ui.painter().circle_filled(d_rect.center(), 3.5, accent);
                                        ui.label(RichText::new(format!("CPU peak {}°C", temperature_text(cpu_peak.unwrap_or(f32::NAN)))).size(10.0).monospace().color(p.text_secondary));
                                    });
                                });
                            });
                        });

                        ui.add_space(8.0);
                        draw_thermal_dual_waveform(ui, 110.0, &history, accent, self.sensors_refresh_interval);
                    });

                ui.add_space(8.0);

                // Fan RPM is reported when available; acoustic readings and controls are not collected.
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Cooling telemetry").size(14.0).strong().color(p.text_primary));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let status = if snap.fan_sensors.is_empty() { "RPM UNAVAILABLE".to_string() } else { format!("{} FANS", snap.fan_sensors.len()) };
                                ui.label(RichText::new(status).size(8.5).monospace().strong().color(p.text_muted));
                            });
                        });
                        if snap.fan_sensors.is_empty() {
                            ui.label(RichText::new("No fan RPM sensors were reported. Cooling controls remain unavailable.")
                                .size(10.0).color(p.text_muted));
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                for fan in &snap.fan_sensors {
                                    let label = if fan.chip.is_empty() { fan.name.clone() } else { format!("{} · {}", fan.chip, fan.name) };
                                    ui.label(RichText::new(format!("{label}: {:.0} RPM", fan.rpm)).size(10.0).monospace().color(p.text_secondary));
                                }
                            });
                            ui.label(RichText::new("Cooling controls and acoustic readings are unavailable.").size(9.0).color(p.text_muted));
                        }
                    });

                ui.add_space(8.0);

                                // 5. Bottom Section: Thermal Sensor Telemetry Matrix
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                let matrix_title = "Individual sensors";
                                ui.label(RichText::new(matrix_title).size(14.0).strong().color(p.text_primary));
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                Frame::none()
                                    .fill(accent_with_alpha(accent, 16))
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(7.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            ui.label(RichText::new("●").size(6.0).color(accent));
                                            ui.label(RichText::new(if self.sensors_refresh_interval == 0 { "PAUSED".to_string() } else { format!("POLL: {}s", self.sensors_refresh_interval) }).size(8.5).monospace().strong().color(p.text_primary));
                                        });
                                    });
                            });
                        });

                        ui.add_space(10.0);

                        let thermistors: Vec<(String, f32, f32, String, &'static str, Color32)> = snap.temp_sensors.iter().filter(|sensor| sensor.temp.is_finite()).map(|sensor| {
                            let limit = reported_temperature_limit(sensor.crit, sensor.max);
                            let status = if limit.is_some_and(|limit| sensor.temp >= limit) { "LIMIT" } else { "READING" };
                            let note = limit.map(|limit| format!("Limit {limit:.0}°C")).unwrap_or_else(|| "Limit unavailable".to_string());
                            (sensor.name.clone(), sensor.temp, limit.unwrap_or(f32::NAN), note, status, accent)
                        }).collect();
                        if thermistors.is_empty() {
                            ui.label(RichText::new("No additional thermal sensors reported").size(11.0).color(p.text_muted));
                        }

                        sensor_card_columns(ui, thermistors.len(), |ui, index| {
                            let (name, val, max_t, sub_note, status, accent) = &thermistors[index];
                            Frame::none()
                                .fill(p.bg_input)
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Label::new(RichText::new(name).size(10.0).monospace().strong().color(p.text_secondary)).truncate()).on_hover_text(name);
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(*status).size(7.5).monospace().strong().color(p.text_secondary));
                                        });
                                    });

                                    ui.add_space(3.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(temperature_text(*val)).size(22.0).strong().color(p.text_primary));
                                        ui.label(RichText::new("°C").size(10.0).color(p.text_muted));
                                    });

                                    ui.label(RichText::new(sub_note).size(10.0).monospace().color(p.text_muted));
                                    ui.add_space(4.0);
                                    let pct = if max_t.is_finite() { (val / max_t).clamp(0.0, 1.0) } else { 0.0 };
                                    let (b_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
                                    ui.painter().rect_filled(b_rect, Rounding::same(1.5), p.bg_button);
                                    let mut f_rect = b_rect;
                                    f_rect.set_width(b_rect.width() * pct);
                                    ui.painter().rect_filled(f_rect, Rounding::same(1.5), *accent);
                                });
                        });
                    });

                ui.add_space(10.0);
            });
    }
}
