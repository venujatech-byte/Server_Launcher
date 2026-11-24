use super::*;

mod processes;
mod overview;
mod cpu;
mod ram;
mod gpu;
mod storage;
mod wifi;
mod data_usage;
mod power;
mod bluetooth;
mod temperature;

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: System Brief Info Panel (Persistent Top Bar Next to Tabs)
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_system_brief_info_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let snap = if let Some(hid) = host_id {
            self.remote_sensors_snapshots.lock().unwrap().get(hid).cloned().unwrap_or_else(|| Arc::new(crate::sensors::SystemSensorsSnapshot::default()))
        } else {
            Arc::clone(&self.sensors_snapshot)
        };

        Frame::none()
            .fill(p.bg_card)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
            .show(ui, |ui| {
                let total_avail_w = ui.available_width();
                let right_w = total_avail_w.min(108.0);
                let left_w = (total_avail_w - right_w - 8.0).max(0.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    if left_w >= 34.0 {
                        ui.allocate_ui_with_layout(egui::vec2(left_w, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.set_max_width(left_w);
                            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                            ui.spacing_mut().item_spacing.x = 8.0;

                            // Left Icon + Host / System Name
                            ui.label(RichText::new(crate::lucide::ACTIVITY).size(12.0).color(accent));
                            let host_name = self.host_display_name(host_id);
                            ui.add(egui::Label::new(RichText::new(&host_name).size(11.5).strong().color(p.text_primary)).truncate())
                                .on_hover_text(host_name);

                            // CPU brief info (clean up "(R)", "(TM)", "CPU" to keep it concise)
                            if ui.available_width() > 140.0 {
                                ui.label(RichText::new("│").size(10.0).color(p.border));

                                let cpu_clean = if !snap.cpu_brand.is_empty() {
                                    let b = snap.cpu_brand
                                        .replace("(R)", "")
                                        .replace("(TM)", "")
                                        .replace("CPU", "");
                                    let first_part = b.split('@').next().unwrap_or(&b).trim();
                                    first_part.split_whitespace().collect::<Vec<_>>().join(" ")
                                } else if host_id.is_some() {
                                    "Remote CPU".to_string()
                                } else {
                                    "Local CPU".to_string()
                                };

                                let cpu_disp = if ui.available_width() < 240.0 && cpu_clean.len() > 18 {
                                    format!("{}…", cpu_clean.chars().take(17).collect::<String>())
                                } else {
                                    cpu_clean
                                };

                                let cores_label = format!("{}C/{}T", if snap.cpu_cores_physical > 0 { snap.cpu_cores_physical.to_string() } else { "—".into() }, if snap.cpu_cores_logical > 0 { snap.cpu_cores_logical.to_string() } else { "—".into() });
                                ui.label(RichText::new(format!("{} {}", crate::lucide::CPU, cpu_disp)).size(10.0).color(p.text_secondary));
                                ui.label(RichText::new(cores_label).size(9.5).monospace().color(p.text_muted));
                            }

                            // RAM brief info
                            if ui.available_width() > 65.0 {
                                ui.label(RichText::new("│").size(10.0).color(p.border));
                                let ram_gb = (snap.mem_total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)).round() as u64;
                                let ram_label = if ram_gb > 0 { format!("{}GB RAM", ram_gb) } else { "RAM —".to_string() };
                                ui.label(RichText::new(format!("{} {}", crate::lucide::RAM, ram_label)).size(10.0).color(p.text_secondary));
                            }

                            // Storage brief info
                            let phys_disk: u64 = snap.physical_disks.iter().map(|d| d.total_bytes).sum();
                            let total_disk = if phys_disk > 0 { phys_disk } else { mounted_storage_totals(&snap.disks).0 };
                            if total_disk > 0 && ui.available_width() > 70.0 {
                                ui.label(RichText::new("│").size(10.0).color(p.border));
                                ui.label(RichText::new(format!("{} {}", crate::lucide::STORAGE, crate::sensors::format_bytes(total_disk))).size(10.0).color(p.text_secondary))
                                    .on_hover_text(if phys_disk > 0 { "Installed physical storage capacity" } else { "Mounted filesystem capacity" });
                            }

                            // System Uptime
                            if ui.available_width() > 75.0 {
                                let up_secs = if snap.uptime_secs > 0 { snap.uptime_secs } else if host_id.is_none() { get_system_uptime() } else { 0 };
                                ui.label(RichText::new("│").size(10.0).color(p.border));
                                ui.label(RichText::new(format!("Up {}", format_uptime(up_secs))).size(9.5).monospace().color(p.text_muted));
                            }
                        });

                    }

                    ui.allocate_ui_with_layout(egui::vec2(right_w, 24.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;

                        let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(10.5).color(p.text_secondary))
                            .fill(p.bg_button)
                            .rounding(Rounding::same(7.0))
                            .min_size(egui::vec2(26.0, 24.0));
                        if ui.add(btn_ref).on_hover_text("Refresh system telemetry now").clicked() {
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    self.trigger_remote_sensors_query(host);
                                }
                            } else {
                                self.trigger_local_sensors_query(false, Some(ui.ctx().clone()));
                                self.last_sensors_refresh = Instant::now();
                            }
                        }

                        let failed = host_id.is_some_and(|id| self.remote_sensors_errors.lock().unwrap().contains_key(id));
                        let age = snap.timestamp.map(|timestamp| timestamp.elapsed());
                        let state = sensor_freshness(age, self.sensors_refresh_interval, failed);
                        let state_color = if state == "LIVE" { accent } else { p.text_muted };
                        Frame::none()
                            .fill(accent_with_alpha(state_color, 25))
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(7.0, 2.5))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(RichText::new("●").size(7.5).color(state_color));
                                    ui.label(RichText::new(state).size(9.5).monospace().strong().color(state_color));
                                });
                            }).response.on_hover_text(age.map(|age| format!("Last updated {}s ago", age.as_secs())).unwrap_or_else(|| "No sensor snapshot received yet".into()));
                    });
                });
            });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: Left Sidebar (Quick Overview Cards & Category Filters)
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_sensors_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        ui.vertical(|ui| {
            // Match the compact Sensors workspace column.
            let navigation_width = ui.available_width().min(SENSORS_SIDEBAR_WIDTH);
            ui.set_width(navigation_width);
            ui.set_max_width(navigation_width);
            ui.add_space(8.0);

            ScrollArea::vertical()
                .id_salt("sensors_left_sidebar_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_width(ui.available_width());

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 26.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().interact_size.y = 26.0;
                                ui.label(RichText::new("HARDWARE ENGINES").size(11.0).strong().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_ref = egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button)
                                        .rounding(Rounding::same(7.0))
                                        .min_size(egui::vec2(28.0, 26.0));
                                    if ui.add(btn_ref).on_hover_text("Refresh all sensors now").clicked() {
                                        if let Some(hid) = host_id {
                                            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                                self.trigger_remote_sensors_query(host);
                                            }
                                        } else {
                                            let include_top = (self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited)
                                                || self.sensors_subtab == SensorsSubTab::Ram;
                                            self.trigger_local_sensors_query(include_top, Some(ui.ctx().clone()));
                                            self.last_sensors_refresh = Instant::now();
                                        }
                                    }

                                    // Auto-refresh interval dropdown
                                    let interval_label = match self.sensors_refresh_interval {
                                        0 => format!("{} Off", crate::lucide::CLOCK),
                                        s => format!("{} {}s", crate::lucide::CLOCK, s),
                                    };
                                    egui::ComboBox::from_id_salt("sensors_refresh_interval_combo")
                                        .selected_text(RichText::new(interval_label).size(10.0).color(p.text_primary))
                                        .width(62.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.sensors_refresh_interval, 0, "Off (Manual)");
                                            ui.selectable_value(&mut self.sensors_refresh_interval, 1, "1 second");
                                            ui.selectable_value(&mut self.sensors_refresh_interval, 2, "2 seconds");
                                            ui.selectable_value(&mut self.sensors_refresh_interval, 3, "3 seconds");
                                            ui.selectable_value(&mut self.sensors_refresh_interval, 5, "5 seconds");
                                        });
                                });
                            },
                        );

                        ui.add_space(6.0);

                        // Left Navigation Menu: 11 Subtabs
                        let items: [(SensorsSubTab, &str, &str); 11] = [
                            (SensorsSubTab::Overview, "Overview", crate::lucide::ACTIVITY),
                            (SensorsSubTab::TopApps, "Top Apps", crate::lucide::LAYOUT_GRID),
                            (SensorsSubTab::Cpu, "CPU", crate::lucide::CPU),
                            (SensorsSubTab::Ram, "RAM", crate::lucide::RAM),
                            (SensorsSubTab::Gpu, "GPU", crate::lucide::GPU),
                            (SensorsSubTab::Storage, "Storage", crate::lucide::STORAGE),
                            (SensorsSubTab::Wifi, "Wi-Fi", crate::lucide::WIFI),
                            (SensorsSubTab::DataUsage, "Data Usage", crate::lucide::NETWORK),
                            (SensorsSubTab::Power, "Power", crate::lucide::ZAP),
                            (SensorsSubTab::Bluetooth, "Bluetooth", crate::lucide::BLUETOOTH),
                            (SensorsSubTab::Temperature, "Temperature", crate::lucide::THERMOMETER),
                        ];

                        for (tab, label, icon) in items {
                            let is_active = self.sensors_subtab == tab;
                            let bg = if is_active {
                                accent
                            } else {
                                Color32::TRANSPARENT
                            };
                            let fg = if is_active {
                                p.accent_text
                            } else {
                                p.text_muted // Slate gray text
                            };

                            let rt = if is_active {
                                RichText::new(format!("{}  {}", icon, label)).size(12.0).strong().color(fg)
                            } else {
                                RichText::new(format!("{}  {}", icon, label)).size(12.0).color(fg)
                            };

                            let btn = egui::Button::new(rt)
                                .fill(bg)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .min_size(egui::vec2(ui.available_width(), 30.0));

                            if ui.add(btn).clicked() {
                                self.sensors_subtab = tab;
                                if tab == SensorsSubTab::TopApps {
                                    self.sensors_top_apps_visited = true;
                                    if self.selected_machine == "local" && self.sensors_snapshot.top_apps.is_empty() {
                                        self.trigger_local_sensors_query(true, Some(ui.ctx().clone()));
                                        self.last_sensors_refresh = Instant::now();
                                    } else if let Some(hid) = host_id {
                                        if let Some(host) = self.ssh_hosts.iter().find(|host| host.id == hid).cloned() {
                                            self.trigger_remote_sensors_query(host);
                                        }
                                    }
                                } else if tab == SensorsSubTab::Ram && host_id.is_some() {
                                    if let Some(host) = self.ssh_hosts.iter().find(|host| Some(host.id.as_str()) == host_id).cloned() {
                                        self.trigger_remote_sensors_query(host);
                                    }
                                } else if tab == SensorsSubTab::DataUsage && host_id.is_none() && self.sensors_snapshot.networks.is_empty() {
                                    self.trigger_local_sensors_query(false, Some(ui.ctx().clone()));
                                    self.last_sensors_refresh = Instant::now();
                                } else if tab == SensorsSubTab::Wifi && host_id.is_some() {
                                    if let Some(host) = self.ssh_hosts.iter().find(|host| Some(host.id.as_str()) == host_id).cloned() {
                                        self.trigger_remote_sensors_query(host);
                                    }
                                }
                            }
                            ui.add_space(2.0);
                        }
                        ui.add_space(4.0);

                    });
                });
        });
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SENSORS TAB: Right Main Area (Full Sensors & Hardware Monitor Viewer)
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_sensors_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);

        let is_fetching = if let Some(hid) = host_id {
            self.remote_sensors_in_flight.lock().unwrap().contains(hid)
        } else {
            false
        };

        // Auto-refresh timer logic (asynchronous for SSH hosts, never blocking UI thread)
        let snap = if let Some(hid) = host_id {
            let last_fetch = self.remote_sensors_last_fetch.lock().unwrap().get(hid).copied();
            let failed = self.remote_sensors_errors.lock().unwrap().contains_key(hid);
            if remote_sensor_due(last_fetch.map(|time| time.elapsed()), self.sensors_refresh_interval, failed) && !is_fetching {
                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                    self.trigger_remote_sensors_query(host);
                }
            }
            self.remote_sensors_snapshots.lock().unwrap().get(hid).cloned().unwrap_or_else(|| Arc::new(crate::sensors::SystemSensorsSnapshot::default()))
        } else {
            if self.sensors_refresh_interval > 0 && self.last_sensors_refresh.elapsed() >= Duration::from_secs(self.sensors_refresh_interval as u64) {
                let include_top = (self.sensors_subtab == SensorsSubTab::TopApps && self.sensors_top_apps_visited)
                    || self.sensors_subtab == SensorsSubTab::Ram;
                self.trigger_local_sensors_query(include_top, Some(ui.ctx().clone()));
                self.last_sensors_refresh = Instant::now();
            }
            Arc::clone(&self.sensors_snapshot)
        };
        if let Some(hid) = host_id {
            let needs_processes = matches!(self.sensors_subtab, SensorsSubTab::TopApps | SensorsSubTab::Ram);
            let needs_network = self.sensors_subtab == SensorsSubTab::TopApps && snap.process_network_supported;
            let needs_network_details = self.sensors_subtab == SensorsSubTab::Wifi;
            if snap.timestamp.is_some() && !is_fetching
                && ((needs_processes && !snap.processes_collected) || (needs_network && !snap.process_network_collected) || (needs_network_details && !snap.network_details_collected))
                && !self.remote_sensors_errors.lock().unwrap().contains_key(hid)
            {
                if let Some(host) = self.ssh_hosts.iter().find(|host| host.id == hid).cloned() {
                    self.trigger_remote_sensors_query(host);
                }
            }
        }
        // Reset telemetry histories when switching between local host and remote SSH targets
        let cur_host_str = host_id.map(|s| s.to_string());
        if self.current_sensors_host != cur_host_str {
            self.sensors_top_apps_selected_pid = None;
            self.current_sensors_host = cur_host_str.clone();
            self.clear_sensor_telemetry_history();
            self.cpu_core_histories = (0..snap.cpu_core_usages.len())
                .map(|_| VecDeque::with_capacity(30)).collect();
        }
        if host_id.is_some() && snap.timestamp.is_none() {
            self.clear_sensor_telemetry_history();
        }

        // Keep overview histories near a 60-second window at the selected poll interval.
        let history_capacity = telemetry_capacity(self.sensors_refresh_interval);
        if snap.timestamp.is_some_and(|timestamp| self.last_telemetry_sample.is_none_or(|last| timestamp > last)) {
            self.last_telemetry_sample = snap.timestamp;
            if snap.cpu_usage_available {
                push_telemetry(&mut self.cpu_history, snap.cpu_global_usage, history_capacity);
                self.cpu_peak_usage = self.cpu_history.iter().copied().fold(0.0_f32, f32::max);
            }
            if snap.mem_total_bytes > 0 {
                push_telemetry(&mut self.ram_history, (snap.mem_usage_pct, reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct).unwrap_or(f32::NAN)), history_capacity);
            } else {
                self.ram_history.clear();
            }

            if snap.total_disk_rates_available {
                push_telemetry(&mut self.disk_write_history, (snap.total_disk_write_rate_bps / 1_000_000.0) as f32, history_capacity);
                push_telemetry(&mut self.disk_read_history, (snap.total_disk_read_rate_bps / 1_000_000.0) as f32, history_capacity);
            } else {
                self.disk_write_history.clear();
                self.disk_read_history.clear();
            }

            for disk in &snap.physical_disks {
                if disk.disk_rates_available {
                    push_telemetry(self.per_disk_write_history.entry(disk.name.clone()).or_default(), (disk.write_rate_bps / 1_000_000.0) as f32, history_capacity);
                    push_telemetry(self.per_disk_read_history.entry(disk.name.clone()).or_default(), (disk.read_rate_bps / 1_000_000.0) as f32, history_capacity);
                } else {
                    self.per_disk_write_history.remove(&disk.name);
                    self.per_disk_read_history.remove(&disk.name);
                }
            }

            if snap.physical_disks.is_empty() {
                for disk in &snap.disks {
                    let name = disk.name.trim_start_matches("/dev/").to_string();
                    if disk.disk_rates_available {
                        push_telemetry(self.per_disk_write_history.entry(name.clone()).or_default(), (disk.write_rate_bps / 1_000_000.0) as f32, history_capacity);
                        push_telemetry(self.per_disk_read_history.entry(name).or_default(), (disk.read_rate_bps / 1_000_000.0) as f32, history_capacity);
                    } else {
                        self.per_disk_write_history.remove(&name);
                        self.per_disk_read_history.remove(&name);
                    }
                }
            }

            if snap.network_rates_available {
                push_telemetry(&mut self.net_tx_history, (snap.total_tx_rate_bps / 1_000_000.0) as f32, history_capacity);
                push_telemetry(&mut self.net_rx_history, (snap.total_rx_rate_bps / 1_000_000.0) as f32, history_capacity);
            } else {
                self.net_tx_history.clear();
                self.net_rx_history.clear();
            }

            self.net_peak_mb = self.net_tx_history.iter().chain(&self.net_rx_history)
                .copied().fold(0.0_f32, f32::max);


            let num_cores = snap.cpu_core_usages.len();
            if self.cpu_core_histories.len() != num_cores {
                self.cpu_core_histories = (0..num_cores)
                    .map(|_| VecDeque::with_capacity(30))
                    .collect();
            }
            for (history, value) in self.cpu_core_histories.iter_mut().zip(&snap.cpu_core_usages) {
                push_telemetry(history, value.filter(|value| value.is_finite()).unwrap_or(f32::NAN), history_capacity);
            }


        }

        ui.vertical(|ui| {
            #[cfg(windows)]
            if host_id.is_none() {
                let sample_age = snap.windows_hardware_sampled_at
                    .map(|sampled_at| format!("{}s ago", sampled_at.elapsed().as_secs()))
                    .unwrap_or_else(|| "awaiting first sample".into());
                let inventory_age = snap.windows_inventory_sampled_at
                    .map(|sampled_at| format!("{}s ago", sampled_at.elapsed().as_secs()))
                    .unwrap_or_else(|| "awaiting complete scan".into());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("Hardware sample {sample_age}")).size(10.5).color(p.text_secondary));
                    ui.label(RichText::new(format!("Last complete inventory {inventory_age}")).size(10.5).color(p.text_muted));
                });
                ui.label(RichText::new("Device disk throughput, SMART, socket counts and per-process network counters are unavailable; their zero fields do not indicate measured inactivity.")
                    .size(10.5).color(p.text_muted));
            }

            #[cfg(windows)]
            if host_id.is_none() {
                if let Some(error) = &snap.windows_telemetry_error {
                    ui.label(RichText::new(error).size(10.5).color(p.text_secondary));
                    ui.add_space(4.0);
                }
            }

            // Remote host error notification (if query failed)
            if let Some(hid) = host_id {
                if let Some(err_msg) = self.remote_sensors_errors.lock().unwrap().get(hid) {
                    ui.add_space(4.0);
                    Frame::none()
                        .fill(accent_with_alpha(accent, 25))
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::ALERT_TRIANGLE).size(11.5).color(accent));
                                ui.label(RichText::new("Remote sensor query notice:").size(11.5).strong().color(p.text_primary));
                                ui.label(RichText::new(err_msg).size(11.0).color(p.text_secondary));
                            });
                        });
                }
            }

            ui.add_space(6.0);



            ui.add_space(4.0);

            match self.sensors_subtab {
                SensorsSubTab::Overview => self.render_sensors_overview_page(ui, &snap, host_id),
                SensorsSubTab::TopApps => self.render_sensors_top_apps_tab(ui, &snap, host_id),
                SensorsSubTab::Cpu => self.render_sensors_cpu_page(ui, &snap, host_id),
                SensorsSubTab::Ram => self.render_sensors_ram_page(ui, &snap, host_id),
                SensorsSubTab::Gpu => self.render_sensors_gpu_page(ui, &snap, host_id),
                SensorsSubTab::Storage => self.render_sensors_storage_page(ui, &snap, host_id),
                SensorsSubTab::Wifi => self.render_sensors_wifi_page(ui, &snap, host_id),
                SensorsSubTab::DataUsage => self.render_sensors_data_usage_page(ui, &snap, host_id),
                SensorsSubTab::Power => self.render_sensors_power_page(ui, &snap, host_id),
                SensorsSubTab::Bluetooth => self.render_sensors_bluetooth_page(ui, &snap, host_id),
                SensorsSubTab::Temperature => self.render_sensors_temperature_page(ui, &snap, host_id),
            }
        });
    }

    pub(in crate::app) fn clear_sensor_telemetry_history(&mut self) {
        self.last_telemetry_sample = None;
        self.cpu_history.clear();
        self.cpu_peak_usage = 0.0;
        self.ram_history.clear();
        self.disk_write_history.clear();
        self.disk_read_history.clear();
        self.per_disk_write_history.clear();
        self.per_disk_read_history.clear();
        self.net_tx_history.clear();
        self.net_rx_history.clear();
        self.net_peak_mb = 0.0;
        self.cpu_core_histories.clear();
    }

    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: CPU Dashboard (Matching media_1792113265094_cd2e3f19.png)
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensor_hardware_access(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        if host_id.is_some() || !cfg!(target_os = "linux") { return; }
        let p = self.palette();
        let key = PendingSudoAction::UnlockSensorAccess.key(None);
        let busy = self.systemd_actions_in_flight.lock().unwrap().contains(&key);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!busy, egui::Button::new(if self.sensors_snapshot.hardware_access_unlocked { "Refresh protected sensors" } else { "Unlock hardware sensors" })).on_hover_text("Extra metrics only: CPU and RAM usage do not need sudo. This read-only probe reads protected CPU energy, Intel GPU counters and RAM module details; it changes no hardware settings.").clicked() {
                self.service_action_msg = None;
                self.queue_systemd_action(PendingSudoAction::UnlockSensorAccess, None, None);
            }
            if busy { ui.spinner(); }
            ui.label(RichText::new("Protected readings are snapshots. Refresh to take another sample.").size(11.0).color(p.text_muted));
        });
        if let Some(message) = self.service_action_msg.as_ref().filter(|message| !busy && (message.starts_with("Read-only hardware") || message.starts_with("Error: Hardware probe") || message.starts_with("Error: Could not start hardware"))) {
            if crate::systemd::needs_sudo_password(message) {
                ui.label(RichText::new("Unlock sensors to read protected metrics.").size(11.0).color(p.text_muted))
                    .on_hover_text(message);
            } else {
                ui.label(RichText::new(message).size(11.0).color(p.text_muted));
            }
        }
        if let Some(error) = &self.sensors_snapshot.hardware_access_error {
            ui.add(egui::Label::new(RichText::new(error).size(11.0).color(p.warning)).truncate()).on_hover_text(error);
        }
        ui.add_space(6.0);
    }
}
