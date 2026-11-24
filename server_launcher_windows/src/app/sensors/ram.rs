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
                let avail_gb_val = available_ram_gb;
                let cached_gb_val = cache_gb;
                let used_pct = if memory_available { snap.mem_usage_pct.clamp(0.0, 100.0) } else { f32::NAN };
                let avail_pct = memory_available.then(|| (snap.mem_available_bytes as f32 / snap.mem_total_bytes as f32 * 100.0).clamp(0.0, 100.0));

                let (slot_label, topol_label, mem_type_str, ram_mod_str) = if let Some(ref hw) = snap.memory_hw {
                    let slot = if host_id.is_some() { "REMOTE : ACTIVE".to_string() } else { if hw.slot_summary.is_empty() { "SLOTS UNAVAILABLE".into() } else { hw.slot_summary.clone() } };
                    let topol = if host_id.is_some() { "HOST MEMORY POOL".to_string() } else { if hw.topology.is_empty() { "TOPOLOGY UNAVAILABLE".into() } else { hw.topology.clone() } };
                    let mtype = format!("{} {} {}", total_ram_str, hw.memory_type, hw.form_factor).trim().to_string();
                    let mspec = hw.module_spec.clone();
                    (slot, topol, mtype, mspec)
                } else {
                    let slot = if host_id.is_some() { "REMOTE : ACTIVE".to_string() } else { "SLOTS UNAVAILABLE".to_string() };
                    let topol = if host_id.is_some() { "HOST MEMORY POOL".to_string() } else { "TOPOLOGY UNAVAILABLE".to_string() };
                    let mtype = total_ram_str.clone();
                    let mspec = if host_id.is_some() { "Module details unavailable".to_string() } else { "Module details unavailable".to_string() };
                    (slot, topol, mtype, mspec)
                };

                // 1. Top Header Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical(|ui| {
                            // Row 1: Overline
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                Frame::none()
                                    .fill(p.bg_subtle)
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&slot_label).size(9.0).monospace().strong().color(p.text_primary));
                                    });
                                ui.label(RichText::new(&topol_label).size(9.0).monospace().color(p.text_muted));
                            });

                            ui.add_space(3.0);

                            ui.label(RichText::new("System Memory (RAM) & Swap").size(16.0).strong().color(p.text_primary));
                            ui.add_space(3.0);

                            // Row 3: Subtitle spanning the full card width underneath Row 2
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(&mem_type_str).size(10.0).monospace().color(p.text_muted));
                                ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                ui.label(RichText::new(&ram_mod_str).size(10.0).monospace().color(p.text_muted));
                                ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                ui.label(RichText::new("Memory telemetry").size(10.0).monospace().color(p.text_muted));
                                ui.label(RichText::new("•").size(9.0).color(p.text_muted));
                                ui.label(RichText::new("Voltage unavailable").size(10.0).monospace().color(p.text_secondary));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 2. Row of 4 Big Metric Cards (matching Overview page architecture)
                let ram_slice: Vec<f32> = self.ram_history.iter().map(|(r, _)| *r).collect();
                let swap_slice: Vec<f32> = self.ram_history.iter().map(|(_, s)| *s).collect();
                let ram_avg = (!self.ram_history.is_empty()).then(|| self.ram_history.iter().map(|(r, _)| *r).sum::<f32>() / self.ram_history.len() as f32);
                let ram_peak = self.ram_history.iter().map(|(r, _)| *r).reduce(f32::max);
                let swap_values: Vec<f32> = self.ram_history.iter().map(|(_, s)| *s).filter(|value| value.is_finite()).collect();
                let swap_avg = if swap_values.is_empty() { reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct) } else { Some(swap_values.iter().sum::<f32>() / swap_values.len() as f32) };
                let swap_peak = swap_values.iter().copied().reduce(f32::max).or_else(|| reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct));

                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: MEMORY IN USE
                    render_big_stat_card(
                        ui,
                        crate::lucide::RAM,
                        "MEMORY IN USE",
                        &reported_sensor_value(memory_available.then_some(used_gb_val as f32), 1),
                        "GB",
                        &if memory_available { format!("Allocation {:.1}% of total", used_pct) } else { "Memory telemetry unavailable".to_string() },
                        &format!("AVG {}%", reported_sensor_value(ram_avg, 0)),
                        &format!("PEAK {}%", reported_sensor_value(ram_peak, 0)),
                        &ram_slice,
                        100.0,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    1 => {
                    // Card 2: AVAILABLE MEMORY
                    let avail_slice: Vec<f32> = ram_slice.iter().map(|&u| (100.0 - u).max(0.0)).collect();
                    render_big_stat_card(
                        ui,
                        crate::lucide::CHECK,
                        "AVAILABLE MEMORY",
                        &reported_sensor_value(memory_available.then_some(avail_gb_val as f32), 1),
                        "GB",
                        &format!("Available {}%", reported_sensor_value(avail_pct, 1)),
                        &format!("TOTAL {}", total_ram_str),
                        "",
                        &avail_slice,
                        100.0,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    2 => {
                    // Card 3: CACHED & BUFFERS
                    let cache_slice: Vec<f32> = Vec::new();
                    render_big_stat_card(
                        ui,
                        crate::lucide::FILE_TEXT,
                        "RECLAIMABLE ESTIMATE",
                        &cached_gb_val.map_or_else(|| "—".into(), |value| format!("{value:.1}")),
                        "GB",
                        "Available − free",
                        &cache_bytes.map_or_else(|| "FREE MEMORY UNAVAILABLE".into(), |value| format!("EST. {} MB", value / (1024 * 1024))),
                        "ESTIMATE",
                        &cache_slice,
                        (cached_gb_val.unwrap_or(0.0) as f32 * 1.5).max(4.0),
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    3 => {
                    // Card 4: SWAP ALLOCATION
                    let swap_usage = reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct);
                    let swap_value = reported_sensor_value(swap_usage, 1);
                    let swap_subtitle = if swap_usage.is_some() { format!("{} / {}", crate::sensors::format_bytes(snap.swap_used_bytes), crate::sensors::format_bytes(snap.swap_total_bytes)) } else { "No swap configured".to_string() };
                    let swap_avg_label = format!("AVG {}{}", reported_sensor_value(swap_avg, 1), if swap_avg.is_some() { "%" } else { "" });
                    let swap_peak_label = format!("PEAK {}{}", reported_sensor_value(swap_peak, 1), if swap_peak.is_some() { "%" } else { "" });
                    render_big_stat_card(
                        ui,
                        crate::lucide::REFRESH_CW,
                        "SWAP ALLOCATION",
                        &swap_value,
                        if swap_usage.is_some() { "%" } else { "" },
                        &swap_subtitle,
                        &swap_avg_label,
                        &swap_peak_label,
                        &swap_slice,
                        100.0,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );
                    },
                    _ => unreachable!(),
                });

                ui.add_space(8.0);

                // 3. Real-Time Allocation Waveform Card (60 SECONDS HISTORY)
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                            ui.painter().circle_filled(d_rect.center(), 2.8, accent);
                            ui.label(RichText::new("Memory Usage History").size(12.5).strong().color(p.text_primary));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("RAM in use · Swap allocation").size(9.0).monospace().color(p.text_muted));
                            });
                        });

                        ui.add_space(8.0);
                        draw_ram_waveform(ui, 120.0, &self.ram_history, &total_ram_str, accent);

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Earlier").size(9.5).monospace().color(p.text_muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(format!("Latest ({}%)", reported_sensor_value(memory_available.then_some(used_pct), 0))).size(9.5).monospace().strong().color(p.text_primary));
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
