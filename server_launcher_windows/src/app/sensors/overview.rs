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

                // 1. Top Row: 4 Metric Cards (CPU, RAM, SWAP, STORAGE)
                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: CPU
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::ACTIVITY).size(12.0).color(accent));
                                ui.label(RichText::new("CPU").size(9.5).monospace().strong().color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(RichText::new(reported_sensor_value(snap.cpu_usage_available.then_some(snap.cpu_global_usage), 1)).size(29.0).strong().color(p.text_primary));
                                ui.label(RichText::new("%").size(12.0).color(p.text_muted));
                            });
                            ui.add_space(2.0);
                            let freq = (snap.cpu_frequency_mhz > 0).then(|| format!("{:.2} GHz", snap.cpu_frequency_mhz as f64 / 1000.0));
                            let cores_str = [
                                (snap.cpu_cores_physical > 0).then(|| format!("{} cores", snap.cpu_cores_physical)),
                                freq,
                            ].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                            let cores_str = if cores_str.is_empty() { "—" } else { &cores_str };
                            ui.label(RichText::new(cores_str).size(9.5).monospace().color(p.text_muted));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if self.cpu_history.is_empty() { "AVG —".to_string() } else { format!("AVG {:.0}%", self.cpu_history.iter().sum::<f32>() / self.cpu_history.len() as f32) }).size(10.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(if self.cpu_history.is_empty() { "PEAK —".to_string() } else { format!("PEAK {:.0}%", self.cpu_peak_usage) }).size(10.0).monospace().color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            let cpu_slice: Vec<f32> = self.cpu_history.iter().copied().collect();
                            draw_card_sparkline(ui, 40.0, &cpu_slice, 100.0, Some(0.12), accent);
                        });

                    },
                    1 => {
                    // Card 2: RAM
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::RAM).size(12.0).color(accent));
                                ui.label(RichText::new("RAM").size(9.5).monospace().strong().color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(RichText::new(reported_sensor_value((snap.mem_total_bytes > 0).then_some(snap.mem_usage_pct), 1)).size(29.0).strong().color(p.text_primary));
                                if snap.mem_total_bytes > 0 { ui.label(RichText::new("%").size(12.0).color(p.text_muted)); }
                            });
                            ui.add_space(2.0);
                            let used_mb = snap.mem_used_bytes as f64 / (1024.0 * 1024.0);
                            let tot_mb = snap.mem_total_bytes as f64 / (1024.0 * 1024.0);
                            let ram_str = if snap.mem_total_bytes == 0 {
                                "Memory telemetry unavailable".to_string()
                            } else if tot_mb > 10240.0 {
                                format!("{:.2} GB / {:.2} GB", used_mb / 1024.0, tot_mb / 1024.0)
                            } else {
                                format!("{:.2} MB / {:.2} MB", used_mb, tot_mb)
                            };
                            ui.label(RichText::new(ram_str).size(9.5).monospace().color(p.text_muted));
                            ui.add_space(8.0);
                            let ram_avg = self.ram_history.iter().map(|(r, _)| *r).sum::<f32>() / self.ram_history.len().max(1) as f32;
                            let ram_peak = self.ram_history.iter().map(|(r, _)| *r).fold(snap.mem_usage_pct, f32::max);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if self.ram_history.is_empty() { "AVG —".to_string() } else { format!("AVG {:.0}%", ram_avg) }).size(10.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(if self.ram_history.is_empty() { "PEAK —".to_string() } else { format!("PEAK {:.0}%", ram_peak) }).size(10.0).monospace().color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            let ram_slice: Vec<f32> = self.ram_history.iter().map(|(r, _)| *r).collect();
                            draw_card_sparkline(ui, 40.0, &ram_slice, 100.0, Some(0.85), accent);
                        });

                    },
                    2 => {
                    // Card 3: SWAP
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::REFRESH_CW).size(12.0).color(accent));
                                ui.label(RichText::new("SWAP").size(9.5).monospace().strong().color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            let swap_usage = reported_swap_usage(snap.swap_total_bytes, snap.swap_usage_pct);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(RichText::new(reported_sensor_value(swap_usage, 1)).size(29.0).strong().color(p.text_primary));
                                if swap_usage.is_some() { ui.label(RichText::new("%").size(12.0).color(p.text_muted)); }
                            });
                            ui.add_space(2.0);
                            let swap_str = if swap_usage.is_some() { format!("{} / {}", crate::sensors::format_bytes(snap.swap_used_bytes), crate::sensors::format_bytes(snap.swap_total_bytes)) } else { "No swap configured".to_string() };
                            ui.label(RichText::new(swap_str).size(9.5).monospace().color(p.text_muted));
                            ui.add_space(8.0);
                            let swap_values: Vec<f32> = self.ram_history.iter().map(|(_, s)| *s).filter(|value| value.is_finite()).collect();
                            let swap_avg = (!swap_values.is_empty()).then(|| swap_values.iter().sum::<f32>() / swap_values.len() as f32);
                            let swap_peak = swap_values.iter().copied().reduce(f32::max);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("AVG {}{}", reported_sensor_value(swap_avg, 1), if swap_avg.is_some() { "%" } else { "" })).size(10.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("PEAK {}{}", reported_sensor_value(swap_peak, 1), if swap_peak.is_some() { "%" } else { "" })).size(10.0).monospace().color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            let swap_slice: Vec<f32> = self.ram_history.iter().map(|(_, s)| *s).collect();
                            draw_card_sparkline(ui, 40.0, &swap_slice, 100.0, Some(0.08), accent);
                        });

                    },
                    3 => {
                    // Card 4: STORAGE
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::STORAGE).size(12.0).color(accent));
                                ui.label(RichText::new("MOUNTED STORAGE").size(9.5).monospace().strong().color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            let (total_bytes, used_bytes, available_bytes) = mounted_storage_totals(&snap.disks);
                            let storage_pct = if total_bytes > 0 { Some((used_bytes as f64 / total_bytes as f64 * 100.0) as f32) } else { None };
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.label(RichText::new(storage_pct.map_or_else(|| "—".to_string(), |value| format!("{value:.1}"))).size(24.0).strong().color(p.text_primary));
                                ui.label(RichText::new("%").size(12.0).color(p.text_muted));
                            });
                            ui.add_space(2.0);
                            let used_gb = used_bytes as f64 / 1e9;
                            let tot_gb = total_bytes as f64 / 1e9;
                            ui.label(RichText::new(if total_bytes > 0 { format!("{used_gb:.2} / {tot_gb:.2} GB") } else { "—".to_string() }).size(9.5).monospace().color(p.text_muted));
                            ui.add_space(8.0);
                            let free_gb = available_bytes as f64 / 1e9;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(if total_bytes > 0 { format!("FREE {:.2} GB", free_gb) } else { "FREE —".to_string() }).size(10.0).monospace().color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let avg = (!self.storage_history.is_empty()).then(|| self.storage_history.iter().sum::<f32>() / self.storage_history.len() as f32);
                                    ui.label(RichText::new(avg.map_or_else(|| "AVG —".to_string(), |value| format!("AVG {value:.1}%"))).size(10.0).monospace().color(p.text_muted));
                                });
                            });
                            ui.add_space(4.0);
                            draw_card_sparkline(ui, 40.0, &self.storage_history.iter().copied().collect::<Vec<_>>(), 100.0, None, accent);
                        });
                    },
                    _ => unreachable!(),
                });

                ui.add_space(10.0);

                // 2. Bottom Row: 2 Cards (OVERALL SPEED & CONNECTION STATS)
                let total_w = ui.available_width();
                let gap = 10.0_f32;
                let side_by_side = total_w >= 560.0;
                let left_w = if side_by_side { (total_w - gap) * 0.64 } else { total_w };
                let right_w = if side_by_side { total_w - left_w - gap } else { total_w };

                ui.allocate_ui_with_layout(egui::vec2(total_w, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Left Card: OVERALL SPEED (~64%)
                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("OVERALL SPEED").size(11.0).monospace().strong().color(p.text_muted));
                                        ui.add_space(2.0);
                                        let scope = if cfg!(windows) && host_id.is_none() { "Reported interfaces" } else { "All interfaces" };
                                        let peak_str = if !snap.network_stats_available { "Network counters unavailable".to_string() } else if !snap.network_rates_available { "Network rates collecting".to_string() } else if self.net_tx_history.is_empty() { format!("{scope} · collecting") } else { format!("{scope} · peak {:.2} MB/s", self.net_peak_mb) };
                                        ui.label(RichText::new(peak_str).size(9.5).color(p.text_muted));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 12.0;
                                            let rx_str = sensor_network_rate(&snap, snap.total_rx_rate_bps);
                                            let tx_str = sensor_network_rate(&snap, snap.total_tx_rate_bps);
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(crate::lucide::ARROW_DOWN).size(11.0).color(p.text_muted));
                                                ui.label(RichText::new("Down").size(10.0).color(p.text_muted));
                                                ui.label(RichText::new(&rx_str).size(11.5).strong().color(p.text_primary));
                                            });
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(crate::lucide::ARROW_UP).size(11.0).color(accent));
                                                ui.label(RichText::new("Up").size(10.0).color(p.text_muted));
                                                ui.label(RichText::new(&tx_str).size(11.5).strong().color(p.text_primary));
                                            });
                                        });
                                    });
                                });

                                ui.add_space(10.0);
                                if !snap.network_rates_available {
                                    ui.label(RichText::new(if snap.network_stats_available { "Network rates collecting" } else { "Network counters unavailable" }).size(9.5).color(p.text_muted));
                                }
                                draw_dual_network_chart(ui, 130.0, &self.net_tx_history, &self.net_rx_history, self.net_peak_mb, accent);

                                ui.add_space(10.0);
                                let (sep_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                                ui.painter().hline(sep_rect.x_range(), sep_rect.center().y, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 8)));
                                ui.add_space(8.0);

                                ui.columns(3, |fcols| {
                                    fcols[0].vertical(|ui| {
                                        ui.label(RichText::new("SENT").size(10.0).monospace().color(p.text_muted));
                                        ui.add_space(2.0);
                                        let sent_str = sensor_network_bytes(&snap, snap.total_tx_bytes);
                                        ui.label(RichText::new(sent_str).size(15.0).strong().color(p.text_primary));
                                    });
                                    fcols[1].vertical(|ui| {
                                        ui.label(RichText::new("RECEIVED").size(10.0).monospace().color(p.text_muted));
                                        ui.add_space(2.0);
                                        let recv_str = sensor_network_bytes(&snap, snap.total_rx_bytes);
                                        ui.label(RichText::new(recv_str).size(15.0).strong().color(p.text_primary));
                                    });
                                    fcols[2].vertical(|ui| {
                                        ui.label(RichText::new("AVG OVER WINDOW").size(10.0).monospace().color(p.text_muted));
                                        ui.add_space(2.0);
                                        let avg_tx = (!self.net_tx_history.is_empty()).then(|| self.net_tx_history.iter().sum::<f32>() / self.net_tx_history.len() as f32);
                                        let avg_rx = (!self.net_rx_history.is_empty()).then(|| self.net_rx_history.iter().sum::<f32>() / self.net_rx_history.len() as f32);
                                        let rate_label = |arrow, avg: Option<f32>| avg.map_or_else(|| format!("{} —", arrow), |v| if v < 1.0 { format!("{} {:.2} KB/s", arrow, v * 1000.0) } else { format!("{} {:.2} MB/s", arrow, v) });
                                        let tx_label = rate_label(crate::lucide::ARROW_UP, avg_tx);
                                        let rx_label = rate_label(crate::lucide::ARROW_DOWN, avg_rx);
                                        ui.label(RichText::new(tx_label).size(10.5).color(p.text_primary));
                                        ui.label(RichText::new(rx_label).size(10.5).color(p.text_secondary));
                                    });
                                });
                            });
                    });

                    ui.add_space(gap);

                    // Right Card: CONNECTION STATS (~36%)
                    ui.allocate_ui_with_layout(egui::vec2(right_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = ui.ctx().style().spacing.item_spacing.x;
                        Frame::none()
                            .fill(p.bg_card)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new("CONNECTION STATS").size(11.0).monospace().strong().color(p.text_muted));
                                ui.add_space(4.0);

                                let (tot_sock, tcp_c, udp_c) = if snap.socket_stats_available {
                                    (snap.socket_total.to_string(), snap.socket_tcp.to_string(), snap.socket_udp.to_string())
                                } else {
                                    ("—".to_string(), "—".to_string(), "—".to_string())
                                };
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(tot_sock).size(22.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("open sockets").size(10.5).color(p.text_muted));
                                });

                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("—").size(11.0).color(accent));
                                        ui.label(RichText::new("TCP").size(10.0).monospace().color(p.text_muted));
                                        ui.label(RichText::new(tcp_c).size(10.5).strong().color(p.text_primary));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("—").size(11.0).color(p.text_muted));
                                        ui.label(RichText::new("UDP").size(10.0).monospace().color(p.text_muted));
                                        ui.label(RichText::new(udp_c).size(10.5).strong().color(p.text_primary));
                                    });
                                });

                                ui.add_space(10.0);
                                if snap.socket_stats_available {
                                    draw_sockets_chart(ui, 130.0, &self.socket_tcp_history, &self.socket_udp_history, accent);
                                } else {
                                    ui.label(RichText::new("Socket history unavailable").size(10.0).color(p.text_muted));
                                }
                            });
                    });
                });

                ui.add_space(10.0);
            });
    }
}
