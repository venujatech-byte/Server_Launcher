use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Storage Dashboard (Matching media_1792116273204_464a968b.png)
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_storage_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();
        ScrollArea::vertical()
            .id_salt("sensors_storage_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Calculate storage metrics across physical disks and mounted filesystems
                let (total_bytes, used_bytes, free_bytes) = mounted_storage_totals(&snap.disks);

                let total_gb = total_bytes as f64 / 1e9;
                let used_gb = used_bytes as f64 / 1e9;
                let free_gb = free_bytes as f64 / 1e9;
                let drive_count = snap.physical_disks.len();
                let read_mb = snap.total_disk_read_rate_bps / 1_000_000.0;
                let write_mb = snap.total_disk_write_rate_bps / 1_000_000.0;
                let st_pct = (total_bytes > 0).then(|| (used_bytes as f64 / total_bytes as f64 * 100.0) as f32);

                // 1. Top Header Banner
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                let overline = if is_remote {
                                    format!("STORAGE • HOST '{}'", self.host_display_name(host_id))
                                } else {
                                    "STORAGE / I/O".to_string()
                                };
                                ui.label(RichText::new(overline).size(9.0).monospace().color(p.text_muted));
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(RichText::new("Storage & Disks").size(18.0).strong().color(p.text_primary));

                                    // "All Devices Synchronized" pill badge
                                    Frame::none()
                                        .fill(p.bg_subtle)
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 5.0;
                                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                                ui.painter().circle_filled(dot_rect.center(), 2.8, accent);
                                                ui.label(RichText::new(if drive_count > 0 { format!("{} Disks Detected", drive_count) } else { "Drive inventory unavailable".to_string() }).size(9.5).monospace().color(p.text_secondary));
                                            });
                                        });
                                });
                            });

                            // The SMART summary below reports actual health when the host supports it.
                        });
                    });

                ui.add_space(8.0);

                // 2. Top Row of 4 Big Stat Cards (matching Overview page architecture)
                let st_history = sensor_metric_history(ui, host_id, "storage:mounted_usage_pct", snap.timestamp, st_pct);
                let hardware_timestamp = snap.hardware_history_timestamp();
                let rd_history = sensor_metric_history(ui, host_id, "storage:read_rate", hardware_timestamp, snap.total_disk_rates_available.then_some(read_mb as f32));
                let wr_history = sensor_metric_history(ui, host_id, "storage:write_rate", hardware_timestamp, snap.total_disk_rates_available.then_some(write_mb as f32));
                let st_subtitle = st_pct.map(|pct| format!("{used_gb:.1} GB mounted ({pct:.1}%)"))
                    .unwrap_or_else(|| "Mounted usage unavailable".to_string());
                let free_stat = if total_bytes > 0 { format!("FREE {free_gb:.1} GB") } else { "Unavailable".to_string() };

                let ssd_count = snap.physical_disks.iter().filter(|d| d.is_ssd == Some(true)).count();
                let hdd_count = snap.physical_disks.iter().filter(|d| d.is_ssd == Some(false)).count();
                let unknown_count = snap.physical_disks.len().saturating_sub(ssd_count + hdd_count);
                let drives_sub = if !snap.physical_disks.is_empty() {
                    if unknown_count > 0 {
                        format!("{} SSD • {} HDD • {} unknown", ssd_count, hdd_count, unknown_count)
                    } else if ssd_count > 0 && hdd_count > 0 {
                        format!("{}x SSD • {}x HDD", ssd_count, hdd_count)
                    } else if ssd_count > 0 {
                        format!("{}x SSD Storage", ssd_count)
                    } else if hdd_count > 0 {
                        format!("{}x HDD Storage", hdd_count)
                    } else {
                        format!("{}x Block Storage Disks", drive_count)
                    }
                } else {
                    "Physical drive data unavailable".to_string()
                };

                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: TOTAL STORAGE
                    render_big_stat_card(
                        ui,
                        crate::lucide::STORAGE,
                        "MOUNTED STORAGE",
                        &if total_bytes > 0 { format!("{total_gb:.1}") } else { "—".to_string() },
                        "GB",
                        &st_subtitle,
                        "CAPACITY",
                        &free_stat,
                        &st_history,
                        100.0,
                        st_pct.map(|pct| pct / 100.0),
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    1 => {
                    // Card 2: PHYSICAL DRIVES
                    render_big_stat_card(
                        ui,
                        crate::lucide::HARD_DRIVE,
                        "PHYSICAL DRIVES",
                        &if drive_count > 0 { drive_count.to_string() } else { "—".to_string() },
                        "DISKS",
                        &drives_sub,
                        "ONLINE",
                        "Not queried",
                        &[],
                        (drive_count as f32 * 1.5).max(1.0),
                        (drive_count > 0).then_some(1.0),
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    2 => {
                    // Card 3: READ THROUGHPUT
                    render_big_stat_card(
                        ui,
                        crate::lucide::ARROW_DOWN,
                        "READ THROUGHPUT",
                        &snap.total_disk_rates_available.then(|| format!("{:.1}", read_mb)).unwrap_or_else(|| "—".to_string()),
                        "MB/s",
                        if snap.total_disk_rates_available { "Measured read rate" } else { "Rate unavailable" },
                        "",
                        "",
                        if snap.total_disk_rates_available { &rd_history } else { &[] },
                        150.0,
                        snap.total_disk_rates_available.then(|| (read_mb as f32 / 100.0).clamp(0.05, 1.0)),
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    3 => {
                    // Card 4: WRITE THROUGHPUT
                    render_big_stat_card(
                        ui,
                        crate::lucide::ARROW_UP,
                        "WRITE THROUGHPUT",
                        &snap.total_disk_rates_available.then(|| format!("{:.1}", write_mb)).unwrap_or_else(|| "—".to_string()),
                        "MB/s",
                        if snap.total_disk_rates_available { "Measured write rate" } else { "Rate unavailable" },
                        "",
                        "",
                        if snap.total_disk_rates_available { &wr_history } else { &[] },
                        100.0,
                        snap.total_disk_rates_available.then(|| (write_mb as f32 / 100.0).clamp(0.05, 1.0)),
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );
                    },
                    _ => unreachable!(),
                });

                ui.add_space(8.0);

                // 3. Drives Grid (Storage 1, Storage 2, Storage 3... in 2-column paired layout)
                let drives_list: Vec<(String, String, u64, Option<bool>, Option<usize>, f64, f64, bool)> = if !snap.physical_disks.is_empty() {
                    snap.physical_disks
                        .iter()
                        .map(|d| (d.name.clone(), d.model.clone(), d.total_bytes, d.is_ssd, d.partition_count, d.read_rate_bps, d.write_rate_bps, d.disk_rates_available))
                        .collect()
                } else if !snap.disks.is_empty() {
                    let mut seen = std::collections::HashSet::new();
                    let mut synthesized = Vec::new();
                    for d in &snap.disks {
                        let clean = if cfg!(windows) { d.mount_point.clone() } else { d.name.trim_start_matches("/dev/").to_string() };
                        if seen.insert(clean.clone()) {
                            let is_ssd = None;
                            let model = if !d.fs_type.is_empty() {
                                format!("{} Storage ({})", d.fs_type.to_uppercase(), d.mount_point)
                            } else {
                                format!("Volume ({})", d.mount_point)
                            };
                            synthesized.push((clean, model, d.total_bytes, is_ssd, None, d.read_rate_bps, d.write_rate_bps, d.disk_rates_available));
                        }
                    }
                    synthesized
                } else {
                    Vec::new()
                };

                if self.selected_disk_idx >= drives_list.len() {
                    self.selected_disk_idx = 0;
                }
                let d1_phys = snap.physical_disks.get(self.selected_disk_idx);
                let primary_drive_name = drives_list.first().map(|d| d.0.clone()).unwrap_or_else(|| "Unavailable".to_string());

                let current_selected_idx = self.selected_disk_idx;
                let mut clicked_drive_idx: Option<usize> = None;

                let render_drive_card = |ui: &mut Ui, idx: usize, d_name: &str, d_model: &str, d_bytes: u64, d_is_ssd: Option<bool>, d_part_count: Option<usize>, d_read_bps: f64, d_write_bps: f64, rates_available: bool, is_selected: bool, clicked_slot: &mut Option<usize>| {
                    let d_phys_gb = d_bytes as f64 / 1e9;
                    let physical_disk = snap.physical_disks.iter()
                        .find(|disk| disk.name.trim_start_matches("/dev/") == d_name.trim_start_matches("/dev/"));
                    let partitions = physical_disk.map(|disk| disk.partitions.as_slice()).unwrap_or(&[]);
                    let d_mount = snap.disks.iter().find(|disk| disk.mount_point == d_name || disk_owns_mount(d_name, partitions, &disk.name));

                    let d_read_str = rates_available.then(|| crate::sensors::format_rate(d_read_bps)).unwrap_or_else(|| "—".to_string());
                    let d_write_str = rates_available.then(|| crate::sensors::format_rate(d_write_bps)).unwrap_or_else(|| "—".to_string());
                    let disk_identity = if cfg!(windows) { d_name.to_owned() } else { format!("/dev/{d_name}") };

                    let (d_used, d_tot, d_pct, d_fs, is_mounted, mount_pt) = if let Some(m) = d_mount {
                        let u = m.used_bytes as f64 / 1e9;
                        let t = m.total_bytes as f64 / 1e9;
                        let pct = if t > 0.0 { (u / t * 100.0) as f32 } else { 0.0 };
                        let fs = if m.fs_type.is_empty() { "Unknown" } else { m.fs_type.as_str() };
                        (u, t, pct, fs, true, m.mount_point.as_str())
                    } else {
                        (0.0, d_phys_gb, 0.0, "Unknown", false, "")
                    };

                    let card_bg = if is_selected {
                        p.bg_card_active
                    } else {
                        p.bg_card
                    };
                    let card_stroke = if is_selected {
                        Stroke::new(1.0_f32, accent)
                    } else {
                        Stroke::NONE
                    };

                    let resp = Frame::none()
                        .fill(card_bg)
                        .stroke(card_stroke)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if d_is_ssd == Some(true) { crate::lucide::STORAGE } else { crate::lucide::HARD_DRIVE }).size(15.0).color(accent));
                                ui.add_space(4.0);

                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        ui.label(RichText::new(if physical_disk.is_some() { format!("Storage {} ({})", idx + 1, disk_identity) } else { format!("Volume {} ({})", idx + 1, d_name) }).size(13.0).strong().color(p.text_primary));
                                        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                        let dot_col = if is_selected { accent } else { p.text_muted };
                                        ui.painter().circle_filled(dot_rect.center(), 2.8, dot_col);
                                    });
                                    let media_label = if physical_disk.is_none() { "Volume" } else { match d_is_ssd { Some(true) => "SSD", Some(false) => "HDD", None => "Disk" } };
                                    ui.add(egui::Label::new(RichText::new(format!("{} • {} {:.1} GB", media_label, d_model, d_phys_gb)).size(9.5).monospace().color(p.text_muted)).truncate());
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.vertical(|ui| {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let (tag_lbl, tag_bg, tag_col) = if is_selected {
                                                ("ACTIVE MONITOR", accent_with_alpha(accent, 40), p.text_primary)
                                            } else {
                                                ("STORAGE", Color32::from_rgba_unmultiplied(148, 163, 184, 20), p.text_secondary)
                                            };
                                            Frame::none()
                                                .fill(tag_bg)
                                                .rounding(Rounding::same(4.0))
                                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(tag_lbl).size(8.5).monospace().strong().color(tag_col));
                                                });
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let partition_label = if physical_disk.is_none() { "VOLUME".to_string() } else {
                                                d_part_count.map_or_else(|| "PARTITIONS N/A".to_string(), |count| format!("{count} PARTITION{}", if count == 1 { "" } else { "S" }))
                                            };
                                            ui.label(RichText::new(partition_label).size(9.0).monospace().color(p.text_muted));
                                        });
                                    });
                                });
                            });

                            ui.add_space(8.0);
                            let total_w = ui.available_width();
                            let right_w = if is_mounted { 160.0 } else { 110.0 };
                            let left_w = (total_w - right_w - 8.0).max(80.0);

                            ui.horizontal(|ui| {
                                let display_mnt_text = if is_mounted {
                                    if mount_pt.len() > 22 {
                                        let short_path = if let Some(last_slash) = mount_pt.rfind('/') {
                                            format!("…{}", &mount_pt[last_slash..])
                                        } else {
                                            let count = mount_pt.chars().count();
                                            format!("…{}", mount_pt.chars().skip(count.saturating_sub(16)).collect::<String>())
                                        };
                                        format!("Mount: {} ({})", short_path, d_fs)
                                    } else {
                                        format!("Mount: {} ({})", mount_pt, d_fs)
                                    }
                                } else {
                                    "No mount reported".to_string()
                                };

                                ui.allocate_ui_with_layout(egui::vec2(left_w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    let mnt_lbl = ui.add(
                                        egui::Label::new(
                                            RichText::new(&display_mnt_text)
                                                .size(10.0)
                                                .monospace()
                                                .color(p.text_secondary),
                                        )
                                        .truncate(),
                                    );
                                    if is_mounted {
                                        mnt_lbl.on_hover_text(format!("Mount: {} ({})", mount_pt, d_fs));
                                    }
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if is_mounted {
                                        ui.label(RichText::new(format!("{:.1} GB / {:.1} GB ({:.0}%)", d_used, d_tot, d_pct)).size(10.0).monospace().color(p.text_secondary));
                                    } else {
                                        ui.label(RichText::new(format!("{:.1} GB (Ready)", d_phys_gb)).size(10.0).monospace().color(p.text_secondary));
                                    }
                                });
                            });

                            ui.add_space(4.0);
                            let (br, bp) = ui.allocate_painter(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                            bp.rect_filled(br.rect, Rounding::same(2.0), p.bg_subtle);
                            let fill_w = br.rect.width() * (d_pct / 100.0).clamp(0.02, 1.0);
                            bp.rect_filled(egui::Rect::from_min_size(br.rect.min, egui::vec2(fill_w, 4.0)), Rounding::same(2.0), accent);

                            ui.add_space(8.0);
                            ui.columns(3, |scols| {
                                scols[0].vertical(|ui| {
                                    ui.label(RichText::new("READ RATE").size(8.5).monospace().color(p.text_muted));
                                    ui.label(RichText::new(&d_read_str).size(12.5).monospace().strong().color(p.text_primary));
                                });
                                scols[1].vertical(|ui| {
                                    ui.label(RichText::new("WRITE RATE").size(8.5).monospace().color(p.text_muted));
                                    ui.label(RichText::new(&d_write_str).size(12.5).monospace().strong().color(p.text_primary));
                                });
                                scols[2].vertical(|ui| {
                                    ui.label(RichText::new("BUS INTERFACE").size(8.5).monospace().color(p.text_muted));
                                    let bus_str = physical_disk.map(|disk| disk.bus_interface.as_str()).filter(|bus| !bus.is_empty()).unwrap_or("Not reported");
                                    ui.add(egui::Label::new(RichText::new(bus_str).size(12.5).monospace().strong().color(p.text_primary)).truncate()).on_hover_text(bus_str);
                                });
                            });
                        });

                    let interact = ui.interact(resp.response.rect, ui.make_persistent_id(format!("drive_card_sel_{}", idx)), egui::Sense::click());
                    if interact.clicked() {
                        *clicked_slot = Some(idx);
                    }
                };

                let render_unpopulated_card = |ui: &mut Ui, slot_num: usize| {
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::HARD_DRIVE).size(15.0).color(p.text_muted));
                                ui.add_space(4.0);

                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        ui.label(RichText::new(format!("Storage {} — No data", slot_num)).size(13.0).strong().color(p.text_primary));
                                        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                        ui.painter().circle_filled(dot_rect.center(), 2.8, p.text_muted);
                                    });
                                    ui.label(RichText::new("No additional drive reported").size(9.5).monospace().color(p.text_muted));
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    Frame::none()
                                        .fill(p.bg_subtle)
                                        .rounding(Rounding::same(4.0))
                                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("STANDBY").size(8.5).monospace().color(p.text_muted));
                                        });
                                });
                            });

                            ui.add_space(8.0);
                            let total_w = ui.available_width();
                            let right_w = 110.0;
                            let left_w = (total_w - right_w - 8.0).max(80.0);
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(egui::vec2(left_w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.add(egui::Label::new(RichText::new("").size(10.0).monospace().color(p.text_muted)).truncate());
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("—").size(10.0).monospace().color(p.text_muted));
                                });
                            });

                            ui.add_space(4.0);
                            let (br, bp) = ui.allocate_painter(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                            bp.rect_filled(br.rect, Rounding::same(2.0), p.bg_subtle);

                            ui.add_space(8.0);
                            ui.columns(3, |scols| {
                                scols[0].vertical(|ui| {
                                    ui.label(RichText::new("READ RATE").size(8.5).monospace().color(p.text_muted));
                                    ui.label(RichText::new("—").size(12.5).monospace().color(p.text_muted));
                                });
                                scols[1].vertical(|ui| {
                                    ui.label(RichText::new("WRITE RATE").size(8.5).monospace().color(p.text_muted));
                                    ui.label(RichText::new("—").size(12.5).monospace().color(p.text_muted));
                                });
                                scols[2].vertical(|ui| {
                                    ui.label(RichText::new("BUS INTERFACE").size(8.5).monospace().color(p.text_muted));
                                    ui.label(RichText::new("—").size(12.5).monospace().color(p.text_muted));
                                });
                            });
                        });
                };

                for (chunk_idx, chunk) in drives_list.chunks(2).enumerate() {
                    ui.columns(2, |cols| {
                        let d1 = &chunk[0];
                        let idx1 = chunk_idx * 2;
                        render_drive_card(&mut cols[0], idx1, &d1.0, &d1.1, d1.2, d1.3, d1.4, d1.5, d1.6, d1.7, idx1 == current_selected_idx, &mut clicked_drive_idx);

                        if chunk.len() > 1 {
                            let d2 = &chunk[1];
                            let idx2 = chunk_idx * 2 + 1;
                            render_drive_card(&mut cols[1], idx2, &d2.0, &d2.1, d2.2, d2.3, d2.4, d2.5, d2.6, d2.7, idx2 == current_selected_idx, &mut clicked_drive_idx);
                        } else {
                            render_unpopulated_card(&mut cols[1], idx1 + 2);
                        }
                    });
                    ui.add_space(8.0);
                }

                if let Some(clicked) = clicked_drive_idx {
                    self.selected_disk_idx = clicked;
                }

                // 4. Live Throughput Activity (60s Window)
                let active_disk_idx = if drives_list.is_empty() { 0 } else { self.selected_disk_idx.min(drives_list.len() - 1) };
                let (active_disk_name, active_read_mb, active_write_mb, active_rates_available) = if let Some(d) = drives_list.get(active_disk_idx) {
                    (d.0.clone(), d.5 / 1_000_000.0, d.6 / 1_000_000.0, d.7)
                } else {
                    (primary_drive_name.clone(), read_mb, write_mb, snap.total_disk_rates_available)
                };

                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;


                            // Interactive Disk Selector Pills
                            if drives_list.len() > 1 {
                                for (i, d) in drives_list.iter().enumerate() {
                                    let is_active = i == active_disk_idx;
                                    let pill_txt = format!("Disk {} • {}", i + 1, d.0);
                                    let pill_bg = if is_active {
                                        accent_with_alpha(accent, 35)
                                    } else {
                                        p.bg_subtle
                                    };
                                    let pill_stroke = if is_active {
                                        Stroke::new(1.0_f32, accent)
                                    } else {
                                        Stroke::NONE
                                    };
                                    let text_col = if is_active { p.text_primary } else { p.text_muted };
                                    if ui.add(egui::Button::new(RichText::new(pill_txt).size(9.0).monospace().strong().color(text_col))
                                        .fill(pill_bg)
                                        .stroke(pill_stroke)
                                        .rounding(Rounding::same(4.0))).clicked() {
                                        self.selected_disk_idx = i;
                                    }
                                }
                            } else {
                                Frame::none()
                                    .fill(p.bg_subtle)
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("Disk 1 • {}", primary_drive_name)).size(9.0).monospace().color(p.text_muted));
                                    });
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;


                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let (sq_r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                        ui.painter().rect_filled(sq_r, Rounding::same(1.5), p.text_secondary);
                                        ui.label(RichText::new(active_rates_available.then(|| format!("Read: {:.1} MB/s", active_read_mb)).unwrap_or_else(|| "Read: —".to_string())).size(9.5).monospace().color(p.text_secondary));
                                    });

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let (sq_r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                        ui.painter().rect_filled(sq_r, Rounding::same(1.5), accent);
                                        ui.label(RichText::new(active_rates_available.then(|| format!("Write: {:.1} MB/s", active_write_mb)).unwrap_or_else(|| "Write: —".to_string())).size(9.5).monospace().strong().color(p.text_primary));
                                    });
                                });
                            });
                        });

                        ui.add_space(8.0);
                        let empty_history = VecDeque::new();
                        let (w_hist, r_hist) = if drives_list.is_empty() {
                            (&self.disk_write_history, &self.disk_read_history)
                        } else {
                            (
                                self.per_disk_write_history.get(&active_disk_name).unwrap_or(&empty_history),
                                self.per_disk_read_history.get(&active_disk_name).unwrap_or(&empty_history),
                            )
                        };
                        draw_disk_waveform(ui, 130.0, w_hist, r_hist, accent);

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                            ui.painter().circle_filled(dot_rect.center(), 2.5, accent);
                            let buf_label = if active_rates_available {
                                format!("Disk activity • {}", active_disk_name)
                            } else {
                                "Disk activity unavailable".to_string()
                            };
                            ui.label(RichText::new(buf_label).size(9.0).monospace().color(p.text_muted));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("Latency not measured").size(9.0).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);

                // 5. Row of 4 Telemetry Cards
                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: THERMAL CORE
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("DRIVE TEMPERATURE").size(11.0).monospace().strong().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(crate::lucide::THERMOMETER).size(11.0).color(accent));
                                });
                            });
                            ui.add_space(4.0);
                            let t = snap.storage_temp.unwrap_or(f32::NAN);
                            let limit = matching_temperature_limit(&snap.temp_sensors, crate::sensors::SensorCategory::Storage, t);
                            let (st_label, st_col) = match (t.is_finite(), limit) {
                                (false, _) => ("Unavailable", p.text_muted),
                                (true, Some(limit)) if t >= limit => ("At reported limit", p.danger),
                                (true, Some(_)) => ("Below reported limit", accent),
                                (true, None) => ("No limit reported", p.text_muted),
                            };

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(RichText::new(if t.is_finite() { format!("{:.1}", t) } else { "—".to_string() }).size(24.0).strong().color(p.text_primary));
                                ui.label(RichText::new("°C").size(12.0).color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_rect.center(), 2.5, st_col);
                                ui.label(RichText::new(st_label).size(9.0).monospace().strong().color(p.text_primary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("").size(9.0).monospace().color(p.text_muted));
                                });
                            });
                        });

                    },
                    1 => {
                    // Card 2: S.M.A.R.T. STATUS
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("S.M.A.R.T. STATUS").size(9.5).monospace().strong().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(crate::lucide::SHIELD).size(11.0).color(accent));
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let smart_status = d1_phys.map(|disk| disk.smart_status.as_str()).filter(|status| !status.is_empty()).unwrap_or("Unavailable");
                                let smart_color = match smart_status {
                                    "PASSED" => accent,
                                    "WARNING" => p.danger,
                                    _ => p.text_muted,
                                };
                                ui.label(RichText::new(smart_status).size(18.0).strong().color(smart_color));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("SMART self-test status").size(9.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("").size(9.0).monospace().strong().color(p.text_secondary));
                                });
                            });
                        });

                    },
                    2 => {
                    // Card 3: DRIVE BUS INTERFACE
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("DRIVE BUS INTERFACE").size(9.5).monospace().strong().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(crate::lucide::HARD_DRIVE).size(11.0).color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                let bus = d1_phys.map(|disk| disk.bus_interface.as_str()).filter(|bus| !bus.is_empty()).unwrap_or("—");
                                ui.label(RichText::new(bus).size(18.0).strong().color(p.text_primary));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Reported device bus").size(9.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("").size(9.0).monospace().color(p.text_muted));
                                });
                            });
                        });

                    },
                    3 => {
                    // Card 4: TRIM / DISCARD
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("TRIM / DISCARD").size(9.5).monospace().strong().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(crate::lucide::ACTIVITY).size(11.0).color(accent));
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let discard = d1_phys.and_then(|disk| disk.discard_supported);
                                let discard_label = match discard {
                                    Some(true) => "SUPPORTED",
                                    Some(false) => "UNSUPPORTED",
                                    None => "—",
                                };
                                let discard_color = if discard == Some(true) { accent } else { p.text_muted };
                                ui.label(RichText::new(discard_label).size(15.0).strong().color(discard_color));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Queue discard capability").size(9.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new("").size(9.0).monospace().strong().color(p.text_secondary));
                                });
                            });
                        });
                    },
                    _ => unreachable!(),
                });

                ui.add_space(8.0);

                // 6. Physical Partition Map & Topologies Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(crate::lucide::STORAGE).size(12.0).color(accent));
                            ui.label(RichText::new("Partition map").size(12.5).strong().color(p.text_primary));
                            if let Some(disk) = d1_phys {
                                ui.label(RichText::new(format!("{} • /dev/{}", disk.model, disk.name)).size(9.5).monospace().color(p.text_muted));
                            }
                        });

                        ui.add_space(8.0);

                        // Visual Partition Bar (24px tall)
                        let (part_res, part_p) = ui.allocate_painter(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
                        let pr = part_res.rect;
                        part_p.rect_filled(pr, Rounding::same(4.0), p.bg_input);

                        // Draw proportional partitions from real physical disk partitions
                        if let Some(primary) = d1_phys {
                            if !primary.partitions.is_empty() {
                                let total_part_bytes: u64 = primary.partitions.iter().map(|p| p.size_bytes).sum::<u64>().max(1);
                                let gap = 2.0_f32;
                                let n = primary.partitions.len();
                                let total_gaps = gap * (n.saturating_sub(1) as f32);
                                let usable_w = (pr.width() - total_gaps).max(10.0);

                                let mut cur_x = pr.min.x;
                                for (idx, part) in primary.partitions.iter().enumerate() {
                                    let fraction = (part.size_bytes as f32 / total_part_bytes as f32).max(0.01);
                                    let rem_x = (pr.max.x - cur_x).max(0.0);
                                    let w = if idx == n - 1 { rem_x } else { (fraction * usable_w).min(rem_x) };

                                    let seg_r = egui::Rect::from_min_size(egui::pos2(cur_x, pr.min.y), egui::vec2(w.max(6.0), pr.height()));

                                    // Determine partition role styling
                                    let (fill_col, text_col, label) = (
                                        accent.gamma_multiply(0.65),
                                        p.text_primary,
                                        format!("P{}", part.partition_num),
                                    );

                                    part_p.rect_filled(seg_r, Rounding::same(3.0), fill_col);

                                    // Render label if wide enough
                                    if w > 65.0 {
                                        let gb = part.size_bytes as f64 / 1e9;
                                        part_p.text(seg_r.center(), egui::Align2::CENTER_CENTER, format!("{} ({:.1} GB)", label, gb), egui::FontId::monospace(8.5), text_col);
                                    } else if w > 24.0 {
                                        part_p.text(seg_r.center(), egui::Align2::CENTER_CENTER, &label, egui::FontId::monospace(8.0), text_col);
                                    }

                                    cur_x += w + gap;
                                    if cur_x >= pr.max.x {
                                        break;
                                    }
                                }
                            }
                        }
                        if d1_phys.map_or(true, |disk| disk.partitions.is_empty()) {
                            part_p.text(pr.center(), egui::Align2::CENTER_CENTER, "Partition information unavailable", egui::FontId::monospace(9.0), p.text_muted);
                        }

                        ui.add_space(8.0);

                        ui.columns(4, |cols| {
                            let mut mounts = snap.disks.iter().filter(|mount| {
                                if let Some(disk) = d1_phys {
                                    disk_owns_mount(&disk.name, &disk.partitions, &mount.name)
                                } else {
                                    drives_list.get(self.selected_disk_idx).is_some_and(|disk| disk_owns_mount(&disk.0, &[], &mount.name))
                                }
                            });
                            for col in cols.iter_mut() {
                                if let Some(mount) = mounts.next() {
                                    col.add(egui::Label::new(RichText::new(format!("MOUNT: {}", mount.mount_point)).size(9.0).monospace().color(p.text_muted)).truncate())
                                        .on_hover_text(&mount.mount_point);
                                    col.add_space(2.0);
                                    col.label(RichText::new(format!("{:.1} GB {}", mount.total_bytes as f64 / 1e9, mount.fs_type)).size(13.0).strong().color(p.text_primary));
                                    col.add(egui::Label::new(RichText::new(format!("{} • {:.1} GB used", mount.name, mount.used_bytes as f64 / 1e9)).size(9.0).monospace().color(p.text_muted)).truncate())
                                        .on_hover_text(format!("{} • {:.1} GB used", mount.name, mount.used_bytes as f64 / 1e9));
                                } else {
                                    col.label(RichText::new("—").size(13.0).color(p.text_muted));
                                    col.label(RichText::new("No mount reported").size(9.0).color(p.text_muted));
                                }
                            }
                        });
                    });

                ui.add_space(10.0);
            });
    }
}
