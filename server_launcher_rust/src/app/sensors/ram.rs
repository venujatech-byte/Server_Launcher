use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: RAM Dashboard (Matching media_1792116273219_0ccb7bbe.png)
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_ram_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        ScrollArea::vertical()
            .id_salt("sensors_ram_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.render_sensor_hardware_access(ui, host_id);

                let total_ram_gb = snap.mem_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                let used_ram_gb = snap.mem_used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                let available_ram_gb = snap.mem_available_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                let memory_available = snap.mem_total_bytes > 0;
                let cache_bytes = memory_available.then(|| memory_reclaimable_estimate(snap.mem_available_bytes, snap.mem_free_bytes)).flatten();
                let cache_gb = cache_bytes.map(|bytes| bytes as f64 / (1024.0 * 1024.0 * 1024.0));

                let total_ram_str = if memory_available { format!("{:.1} GB", total_ram_gb) } else { "Unavailable".to_string() };
                let used_gb_val = used_ram_gb;
                let cached_gb_val = cache_gb;
                let used_pct = if memory_available { snap.mem_usage_pct.clamp(0.0, 100.0) } else { f32::NAN };
                let avail_pct = memory_available.then(|| (snap.mem_available_bytes as f32 / snap.mem_total_bytes as f32 * 100.0).clamp(0.0, 100.0));

                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::RAM).size(15.0).color(accent));
                            ui.label(RichText::new("MEMORY").size(10.5).monospace().strong().color(p.text_muted));
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new("System memory & swap").size(18.0).strong().color(p.text_primary));
                        ui.add_space(6.0);
                        let capacity = format!("{total_ram_str} usable");
                        let swap = (snap.swap_total_bytes > 0).then(|| format!("{} swap", crate::sensors::format_bytes(snap.swap_total_bytes)));
                        ui.horizontal_wrapped(|ui| {
                            for label in std::iter::once(capacity.as_str())
                                .chain(swap.as_deref())
                                .chain(snap.memory_hw.iter().flat_map(|hw| [hw.memory_type.as_str(), hw.form_factor.as_str(), hw.slot_summary.as_str(), hw.topology.as_str()]))
                                .filter(|label| !label.is_empty() && !label.to_ascii_lowercase().contains("unavailable"))
                            {
                                Frame::none()
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(label).size(10.5).monospace().color(p.text_secondary));
                                    });
                            }
                        });
                        let modules = snap.memory_hw.as_ref().map(|hw| hw.module_spec.as_str())
                            .filter(|spec| !spec.is_empty()).unwrap_or("Module details unavailable");
                        ui.add_space(4.0);
                        ui.label(RichText::new(modules).size(10.5).monospace().color(p.text_muted));
                    });

                ui.add_space(8.0);

                let used = memory_available.then_some(used_ram_gb as f32);
                let available = memory_available.then_some(available_ram_gb as f32);
                let reclaimable = cached_gb_val.map(|value| value as f32);
                let swap = reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct);
                let used_detail = used.map_or_else(|| "Memory telemetry unavailable".into(), |_| format!("{used_pct:.1}% of {total_ram_str}"));
                let available_detail = avail_pct.map_or_else(|| "Memory telemetry unavailable".into(), |pct| format!("{pct:.1}% of {total_ram_str}"));
                let swap_detail = swap.map_or_else(|| "Swap telemetry unavailable".into(), |_| {
                    format!("{} / {}", crate::sensors::format_bytes(snap.swap_used_bytes), crate::sensors::format_bytes(snap.swap_total_bytes))
                });
                let swap_detail = if snap.swap_total_bytes == 0 { "No swap reported" } else { &swap_detail };
                let cards = [
                    (crate::lucide::RAM, "MEMORY IN USE", used, "GB", used_detail.as_str()),
                    (crate::lucide::CHECK, "AVAILABLE MEMORY", available, "GB", available_detail.as_str()),
                    (crate::lucide::FILE_TEXT, "RECLAIMABLE ESTIMATE", reclaimable, "GB", "Available − free"),
                    (crate::lucide::REFRESH_CW, "SWAP ALLOCATION", swap, "%", swap_detail),
                ];
                sensor_card_columns(ui, cards.len(), |ui, column| {
                    let (icon, title, value, unit, detail) = cards[column];
                    let history = if column == 3 {
                        self.ram_history.iter().map(|(_, swap)| *swap).collect()
                    } else {
                        sensor_metric_history(ui, host_id, title, snap.timestamp, value)
                    };
                    let scale = if column == 3 { 100.0 } else { (total_ram_gb as f32).max(1.0) };
                    super::overview::overview_metric(ui, &p, accent, icon, title, value, unit, detail, &history, scale);
                });

                ui.add_space(8.0);

                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::lucide::ACTIVITY).size(15.0).color(accent));
                            ui.label(RichText::new("Memory usage history").size(14.0).strong().color(p.text_primary));
                        });
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            let ram = if memory_available { format!("RAM {used_pct:.1}%") } else { "RAM —".into() };
                            let swap = swap.map_or_else(|| "Swap —".into(), |value| format!("Swap {value:.1}%"));
                            ui.label(RichText::new(format!("━━ {ram}")).size(11.0).monospace().strong().color(accent));
                            ui.label(RichText::new(format!("┄┄ {swap}")).size(11.0).monospace().color(Color32::from_rgb(96, 165, 250)));
                            ui.label(RichText::new(format!("{} samples", self.ram_history.len())).size(10.5).monospace().color(p.text_muted));
                        });
                        ui.add_space(10.0);
                        draw_ram_waveform(ui, 120.0, &self.ram_history, &total_ram_str, accent);
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            cols[0].label(RichText::new("Older samples").size(10.0).monospace().color(p.text_muted));
                            cols[1].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("Latest sample").size(10.0).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 4. Physical Memory Overview Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(crate::lucide::LAYERS).size(12.0).color(accent));
                            ui.label(RichText::new("Physical Memory Overview").size(12.5).strong().color(p.text_primary));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let pool_mb = snap.mem_total_bytes / (1024 * 1024);
                                ui.label(RichText::new(if memory_available { format!("{} MB TOTAL PHYSICAL MEMORY", pool_mb) } else { "PHYSICAL MEMORY UNAVAILABLE".to_string() }).size(10.0).monospace().color(p.text_muted));
                            });
                        });

                        ui.add_space(8.0);

                        // Only reported in-use and its remainder are disjoint; other counters below can overlap.
                        let used_frac = (used_pct / 100.0).clamp(0.0, 1.0);
                        let remaining_frac = 1.0 - used_frac;

                        let (bar_res, bar_p) = ui.allocate_painter(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
                        let br = bar_res.rect;
                        bar_res.on_hover_text("In use versus the remainder of reported total memory. Free, dirty and reclaimable values below are separate counters and may overlap.");
                        bar_p.rect_filled(br, Rounding::same(4.0), p.bg_input);

                        if memory_available {
                            let w1 = br.width() * used_frac;
                            let w2 = br.width() * remaining_frac;
                            bar_p.rect_filled(egui::Rect::from_min_size(br.min, egui::vec2(w1, br.height())), Rounding::same(3.0), p.info);
                            bar_p.rect_filled(egui::Rect::from_min_size(egui::pos2(br.min.x + w1, br.min.y), egui::vec2(w2, br.height())), Rounding::same(3.0), p.bg_button);
                        } else {
                            bar_p.text(br.center(), egui::Align2::CENTER_CENTER, "Memory telemetry unavailable", egui::FontId::proportional(10.0), p.text_muted);
                        }

                        ui.add_space(8.0);
                        ui.label(RichText::new("Additional counters below can overlap and are not stacked.").size(9.0).monospace().color(p.text_muted));
                        ui.add_space(5.0);

                        // 4-column legend breakdown
                        ui.columns(4, |cols| {
                            cols[0].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.info);
                                    ui.label(RichText::new("IN USE").size(9.5).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new(reported_sensor_value(memory_available.then_some(used_gb_val as f32), 1) + if memory_available { " GB" } else { "" }).size(14.0).strong().color(p.text_primary));
                                ui.label(RichText::new("Working set").size(9.0).monospace().color(p.text_muted));
                            });

                            cols[1].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.text_muted);
                                    ui.label(RichText::new("MODIFIED").size(9.5).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                let dirty_str = if !memory_available { "—".to_string() } else if let Some(dirty) = snap.mem_dirty_bytes {
                                    crate::sensors::format_bytes(dirty)
                                } else {
                                    "—".to_string()
                                };
                                ui.label(RichText::new(&dirty_str).size(14.0).strong().color(p.text_primary));
                                ui.label(RichText::new("Dirty pages · may overlap").size(9.0).monospace().color(p.text_muted));
                            });

                            cols[2].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.info);
                                    ui.label(RichText::new("EST. RECLAIMABLE").size(9.5).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new(cached_gb_val.map_or_else(|| "—".into(), |value| format!("{value:.1} GB"))).size(14.0).strong().color(p.text_primary));
                                ui.label(RichText::new("Available beyond free").size(9.0).monospace().color(p.text_muted));
                            });

                            cols[3].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.bg_button);
                                    ui.label(RichText::new(if snap.mem_free_bytes.is_some() { "FREE POOL" } else { "AVAILABLE" }).size(9.5).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                let free_label = if memory_available { format!("{} MB", snap.mem_free_bytes.unwrap_or(snap.mem_available_bytes) / (1024 * 1024)) } else { "—".to_string() };
                                ui.label(RichText::new(free_label).size(14.0).strong().color(p.text_primary));
                                ui.label(RichText::new(if snap.mem_free_bytes.is_some() { "Unallocated" } else { "Available for reuse" }).size(9.0).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 5. Top Memory Consumers
                ui.vertical(|ui| {
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 11.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::LAYERS).size(12.0).color(accent));
                                ui.label(RichText::new("Top Memory Consumers").size(12.5).strong().color(p.text_primary));
                            });

                            ui.add_space(2.0);


                            ui.add_space(8.0);

                            let mut apps: Vec<_> = snap.top_apps.iter().collect();
                            apps.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));

                            let total_mem = snap.mem_total_bytes.max(1);

                            let get_app_icon = |name: &str| -> &'static str {
                                let lower = name.to_lowercase();
                                if lower.contains("browser") || lower.contains("chrome") || lower.contains("firefox") || lower.contains("brave") {
                                    crate::lucide::MONITOR
                                } else if lower.contains("rust") || lower.contains("code") || lower.contains("cargo") || lower.contains("python") || lower.contains("node") {
                                    crate::lucide::FILE_CODE
                                } else if lower.contains("gnome") || lower.contains("shell") || lower.contains("wayland") || lower.contains("mutter") || lower.contains("xorg") {
                                    crate::lucide::LAYERS
                                } else if lower.contains("container") || lower.contains("docker") || lower.contains("shim") || lower.contains("podman") {
                                    crate::lucide::BOX
                                } else {
                                    crate::lucide::TERMINAL
                                }
                            };

                            let render_proc_row = |ui: &mut egui::Ui, name: &str, pid: u32, desc: &str, mem_bytes: u64, mem_str: &str, ico: &'static str| {
                                let frac = (mem_bytes as f32 / total_mem as f32).clamp(0.0, 1.0);
                                let pct_val = frac * 100.0;
                                let short_name = if name.chars().count() > 22 { format!("{}…", name.chars().take(20).collect::<String>()) } else { name.to_string() };

                                Frame::none()
                                    .fill(p.bg_input)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(5.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.5))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            ui.label(RichText::new(ico).size(11.0).color(accent));
                                            ui.label(RichText::new(&short_name).size(10.5).strong().color(p.text_primary));

                                            Frame::none()
                                                .fill(accent_with_alpha(accent, 18))
                                                .rounding(Rounding::same(3.0))
                                                .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(format!("PID {}", pid)).size(8.0).monospace().color(p.text_secondary));
                                                });

                                            ui.label(RichText::new(desc).size(8.5).monospace().color(p.text_muted));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    ui.label(RichText::new(format!("{:.1}%", pct_val)).size(9.0).monospace().color(p.text_secondary));
                                                    ui.label(RichText::new(mem_str).size(10.5).monospace().strong().color(p.text_primary));
                                                });
                                            });
                                        });

                                        ui.add_space(3.0);
                                        let (mr, mp) = ui.allocate_painter(egui::vec2(ui.available_width(), 2.5), egui::Sense::hover());
                                        mp.rect_filled(mr.rect, Rounding::same(1.5), p.bg_input);
                                        let mw = mr.rect.width() * frac;
                                        mp.rect_filled(egui::Rect::from_min_size(mr.rect.min, egui::vec2(mw, 2.5)), Rounding::same(1.5), accent);
                                    })
                                    .response
                                    .on_hover_ui(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(ico).size(12.0).color(accent));
                                            ui.label(RichText::new(name).size(11.0).strong().color(Color32::WHITE));
                                        });
                                        ui.add_space(2.0);
                                        ui.label(RichText::new(format!("PID: {}\nResident Memory (RSS): {} ({:.1}% of system RAM)\nDescription: {}", pid, mem_str, pct_val, desc)).size(9.5).monospace().color(p.text_secondary));
                                    });
                            };

                            if apps.is_empty() {
                                ui.label(RichText::new("Waiting for process telemetry…").size(10.5).color(p.text_muted));
                            } else {
                                for app in apps.iter().take(4) {
                                    let mem_str = crate::sensors::format_bytes(app.memory_bytes);
                                    let ico = get_app_icon(&app.name);
                                    let desc = if app.cpu_usage_available { format!("{}% CPU", (app.cpu_usage).clamp(0.0, 100.0).round()) } else { "CPU unavailable".into() };
                                    render_proc_row(ui, &app.name, app.pid, &desc, app.memory_bytes, &mem_str, ico);
                                    ui.add_space(3.0);
                                }
                            }

                            ui.add_space(4.0);
                            ui.horizontal(|ui| {

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn = egui::Button::new(RichText::new("Full Process Tree →").size(9.5).monospace().strong().color(p.text_secondary))
                                        .fill(accent_with_alpha(accent, 18))
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(0.0, 18.0));
                                    if ui.add(btn).on_hover_text("Open Top Apps & Process Tree subtab").clicked() {
                                        self.sensors_subtab = SensorsSubTab::TopApps;
                                        self.sensors_top_apps_visited = true;
                                    }
                                });
                            });
                        });
                });

                ui.add_space(10.0);
            });
    }
}
