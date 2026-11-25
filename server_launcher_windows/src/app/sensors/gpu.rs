use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: GPU Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_gpu_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let is_remote = host_id.is_some();
        ScrollArea::vertical()
            .id_salt("sensors_gpu_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.render_sensor_hardware_access(ui, host_id);

                if snap.gpus.is_empty() {
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("GPU data unavailable").size(15.0).strong().color(p.text_primary));
                            ui.label(RichText::new("This host did not report a graphics adapter to the sensor collector.").size(11.0).color(p.text_muted));
                        });
                    return;
                }

                let active_gpu_idx = self.selected_gpu_idx.min(snap.gpus.len() - 1);
                let gpu = snap.gpus.get(active_gpu_idx).or_else(|| snap.gpus.first());
                let gpu_name = gpu
                    .map(|g| g.name.as_str())
                    .filter(|n| !n.trim().is_empty() && *n != "Unknown GPU")
                    .unwrap_or("GPU information unavailable");





                let gpu_spec_label = gpu.map(|g| g.driver.as_str()).filter(|driver| !driver.trim().is_empty()).unwrap_or("Driver unavailable");

                // Multi-GPU Dispatch Switcher Pills (when host has 2+ GPUs)
                if snap.gpus.len() > 1 {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(RichText::new("DISPATCH SELECTOR:").size(9.0).monospace().color(p.text_muted));
                        for (i, g) in snap.gpus.iter().enumerate() {
                            let is_active = i == active_gpu_idx;
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
                            let pill_txt = format!("GPU {} • {}", i + 1, g.name);
                            let text_col = if is_active { p.text_primary } else { p.text_secondary };

                            if ui.add(egui::Button::new(RichText::new(pill_txt).size(9.5).monospace().strong().color(text_col))
                                .fill(pill_bg)
                                .stroke(pill_stroke)
                                .rounding(Rounding::same(4.0))).clicked() {
                                self.selected_gpu_idx = i;
                            }
                        }
                    });
                    ui.add_space(4.0);
                }

                // 1. Top Header Card
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical(|ui| {
                            // Row 1: Overline
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(RichText::new(crate::lucide::GPU).size(15.0).color(accent));
                                let overline = if is_remote {
                                    format!("REMOTE GPU SUBSYSTEM • HOST '{}' • GPU {}", self.host_display_name(host_id), active_gpu_idx + 1)
                                } else {
                                    format!("GPU ENGINE #{}", active_gpu_idx + 1)
                                };
                                ui.label(RichText::new(overline).size(10.5).monospace().strong().color(p.text_muted));
                            });

                            ui.add_space(2.0);

                            ui.label(RichText::new(gpu_name).size(18.0).strong().color(p.text_primary));
                            ui.add_space(6.0);

                            // Row 3: Subtitle spanning the full card width underneath Row 2
                            let driver_str = gpu_spec_label;
                            let topology = gpu.map(|gpu| {
                                if let Some(address) = gpu.device_path.strip_prefix("pci:") {
                                    format!("PCI {address}")
                                } else if !gpu.device_path.is_empty() {
                                    "Device identity reported".to_string()
                                } else {
                                    "Device identity unavailable".to_string()
                                }
                            }).unwrap_or_else(|| "Device identity unavailable".to_string());
                            let date_or_link = if is_remote { "Remote telemetry" } else { "Local telemetry" };

                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for detail in [driver_str, date_or_link, topology.as_str()] {
                                    Frame::none().fill(p.bg_button).rounding(Rounding::same(6.0))
                                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(detail).size(10.5).monospace().color(p.text_secondary));
                                        });
                                }
                                if ui.add(egui::Button::new(RichText::new(crate::lucide::REFRESH_CW).size(11.0).color(p.text_muted))
                                    .fill(p.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(4.0))).hover_tip("Refresh GPU telemetry").clicked() {
                                    if let Some(hid) = host_id {
                                        if let Some(host) = self.ssh_hosts.iter().find(|host| host.id == hid).cloned() {
                                            self.trigger_remote_sensors_query(host);
                                        }
                                    } else {
                                        self.trigger_local_sensors_query(false, Some(ui.ctx().clone()));
                                    }
                                }
                            });
                        });
                    });

                ui.add_space(8.0);

                // 2. Row of 4 Big Metric Cards
                let gpu_identity = gpu.map(|gpu| if gpu.device_path.is_empty() { gpu.name.as_str() } else { gpu.device_path.as_str() }).unwrap_or("");
                let metric_key = format!("{gpu_identity}:{}", self.selected_gpu_idx);
                let hardware_timestamp = snap.hardware_history_timestamp();
                let (display_load, is_engine_peak) = gpu_display_load(gpu);
                let load_key = if is_engine_peak { "busiest_engine" } else { "load" };
                let gpu_slice = sensor_metric_history(ui, host_id, &format!("{metric_key}:{load_key}"), hardware_timestamp, display_load);
                let render_load = gpu.and_then(|gpu| gpu.engine_loads.iter().filter(|(name, _)| name.to_lowercase().contains("render") || name.contains("3D")).map(|(_, load)| *load).reduce(f32::max));
                let gpu_3d_slice = sensor_metric_history(ui, host_id, &format!("{metric_key}:render"), hardware_timestamp, render_load);
                let gpu_clock_slice = sensor_metric_history(ui, host_id, &format!("{metric_key}:clock"), hardware_timestamp, gpu.and_then(|gpu| gpu.clock_mhz).map(|value| value as f32));
                let max_clk = gpu_clock_slice.iter().copied().filter(|value| value.is_finite()).reduce(f32::max).unwrap_or(1.0).max(1.0);
                let gpu_memory = gpu.and_then(|gpu| gpu.mem_used_mb.zip(gpu.mem_total_mb))
                    .filter(|(used, total)| *total > 0 && used <= total);
                let gpu_vram_slice = sensor_metric_history(ui, host_id, &format!("{metric_key}:vram"), hardware_timestamp, gpu_memory.map(|(used, _)| used as f32 / 1024.0));
                let (vram_val, vram_sub, vram_peak, vram_pwr, total_vram_mb, used_vram_mb) =
                    if let Some((used, total)) = gpu_memory {
                        let pct = used as f64 / total.max(1) as f64 * 100.0;
                        (format!("{:.1}", used as f64 / 1024.0), format!("/ {:.1} GB", total as f64 / 1024.0),
                         format!("USED {:.0}%", pct), gpu.and_then(|g| g.power_watts).map(|w| format!("POWER {:.1}W", w)).unwrap_or_else(|| "Power unavailable".to_string()), total, used)
                    } else {
                        ("—".to_string(), String::new(), "Unavailable from this collector".to_string(), String::new(), 0, 0)
                    };

                sensor_card_columns(ui, 4, |ui, column| match column {
                    0 => {
                    // Card 1: GPU CORE LOAD
                    let mode_lbl = "";
                    let compute_array_lbl = if is_engine_peak { "Busiest engine; overall GPU load unavailable" } else { "Reported GPU utilization" };
                    render_big_stat_card(
                        ui,
                        crate::lucide::GAUGE,
                        if is_engine_peak { "BUSIEST GPU ENGINE" } else { "GPU CORE LOAD" },
                        &reported_sensor_value(display_load, 1),
                        "%",
                        compute_array_lbl,
                        mode_lbl,
                        "",
                        &gpu_slice,
                        100.0,
                        display_load.map(|value| value / 100.0),
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    1 => {
                    // Card 2: CORE CLOCK
                    let min_clk_str = "Clock range unavailable";
                    let max_clk_str = "";
                    render_big_stat_card(
                        ui,
                        crate::lucide::ZAP,
                        "CORE CLOCK",
                        &gpu.and_then(|g| g.clock_mhz).map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                        "MHz",
                        "Reported GPU clock",
                        &min_clk_str,
                        &max_clk_str,
                        &gpu_clock_slice,
                        max_clk * 1.05,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    2 => {
                    // Card 3: 3D RENDER ENGINE
                    render_big_stat_card(
                        ui,
                        crate::lucide::BOX,
                        "3D RENDER ENGINE",
                        &reported_sensor_value(render_load, 1),
                        "%",
                        if render_load.is_some() { "Measured engine utilization" } else { "Per-engine data unavailable" },
                        "",
                        "",
                        &gpu_3d_slice,
                        100.0,
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );

                    },
                    3 => {
                    // Card 4: SHARED / DEDICATED VRAM
                    let card_title = "GPU MEMORY";
                    let card_subtitle = "Reported GPU memory";

                    render_big_stat_card(
                        ui,
                        crate::lucide::DATABASE,
                        card_title,
                        &vram_val,
                        &vram_sub,
                        card_subtitle,
                        &vram_peak,
                        &vram_pwr,
                        &gpu_vram_slice,
                        (total_vram_mb as f32 / 1024.0).max(4.0),
                        None,
                        accent,
                        p.bg_card,
                        p.text_muted,
                    );
                    },
                    _ => unreachable!(),
                });

                // Multi-GPU Cluster Subsystem Matrix (when host has 2+ GPUs)
                if snap.gpus.len() > 1 {
                    ui.add_space(8.0);
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new(crate::lucide::LAYERS).size(12.0).color(accent));
                                ui.label(RichText::new(format!("Detected GPU adapters ({})", snap.gpus.len())).size(12.5).strong().color(p.text_primary));
                            });
                            ui.add_space(8.0);

                            for (i, g) in snap.gpus.iter().enumerate() {
                                let is_sel = i == active_gpu_idx;
                                let card_bg = if is_sel {
                                    accent_with_alpha(accent, 16)
                                } else {
                                    p.bg_input
                                };
                                let border = if is_sel {
                                    Stroke::new(1.0_f32, accent)
                                } else {
                                    Stroke::NONE
                                };

                                Frame::none()
                                    .fill(card_bg)
                                    .stroke(border)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;
                                            ui.label(RichText::new(format!("GPU {}", i + 1)).size(12.0).monospace().strong().color(p.text_primary));
                                            ui.label(RichText::new(&g.name).size(12.0).strong().color(p.text_primary));
                                            ui.label(RichText::new(format!("• {}", g.driver)).size(9.5).monospace().color(p.text_muted));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 10.0;
                                                    if let Some(w) = g.power_watts {
                                                        ui.label(RichText::new(format!("{:.1} W", w)).size(10.0).monospace().color(p.text_secondary));
                                                    }
                                                    if let Some(clk) = g.clock_mhz {
                                                        ui.label(RichText::new(format!("{} MHz", clk)).size(10.0).monospace().color(p.text_primary));
                                                    }
                                                    if let Some(t) = g.temp {
                                                        ui.label(RichText::new(format!("{:.1}°C", t)).size(10.0).monospace().color(p.text_primary));
                                                    }
                                                    if let Some(ld) = g.load_pct {
                                                        ui.label(RichText::new(format!("LOAD {:.1}%", ld)).size(10.0).monospace().strong().color(p.text_primary));
                                                    }
                                                });
                                            });
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });
                }

                ui.add_space(8.0);

                // Plot only engines reported by the selected adapter's PMU.
                Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0)).show(ui, |ui| {
                        ui.label(RichText::new("GPU engine utilization").size(14.0).strong().color(p.text_primary));
                        let engines = gpu.map(|gpu| gpu.engine_loads.as_slice()).unwrap_or(&[]);
                        if engines.is_empty() {
                            ui.label(RichText::new("Per-engine counters are unavailable for this GPU or collector.").size(11.0).color(p.text_muted));
                        } else {
                            sensor_card_columns(ui, engines.len(), |ui, index| {
                                let (name, load) = &engines[index];
                                ui.label(RichText::new(name).size(11.0).color(p.text_secondary));
                                ui.label(RichText::new(format!("{load:.1}%")).size(20.0).strong().color(p.text_primary));
                                let history = sensor_metric_history(ui, host_id, &format!("{metric_key}:{name}"), hardware_timestamp, Some(*load));
                                draw_card_sparkline(ui, 48.0, &history, 100.0, None, accent);
                            });
                        }
                    });
                ui.add_space(8.0);

                if gpu_memory.is_some() {
                // 4. Memory Subsystem Breakdown Card
                let free_vram_mb = total_vram_mb.saturating_sub(used_vram_mb);
                let ded_mb_val = total_vram_mb;
                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(crate::lucide::LAYERS).size(12.0).color(accent));
                            ui.label(RichText::new("Memory subsystem breakdown").size(14.0).strong().color(p.text_primary));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    if total_vram_mb > 0 {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                            ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.text_muted);
                                            ui.label(RichText::new(format!("Free: {} MB", free_vram_mb)).size(9.0).monospace().color(p.text_muted));
                                        });
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                            ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.info);
                                            ui.label(RichText::new(format!("Used: {} MB", used_vram_mb)).size(9.0).monospace().color(p.text_muted));
                                        });
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                            ui.painter().rect_filled(d_rect, Rounding::same(1.5), accent);
                                            ui.label(RichText::new(format!("Total: {} MB", ded_mb_val)).size(9.0).monospace().color(p.text_muted));
                                        });
                                    }
                                });
                            });
                        });

                        ui.add_space(8.0);

                        // Segmented bar
                        let (bar_res, bar_p) = ui.allocate_painter(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
                        let br = bar_res.rect;
                        bar_p.rect_filled(br, Rounding::same(4.0), p.bg_input);

                        if total_vram_mb > 0 {
                            let used_width = br.width() * (used_vram_mb as f32 / total_vram_mb.max(1) as f32).clamp(0.0, 1.0);
                            bar_p.rect_filled(egui::Rect::from_min_size(br.min, egui::vec2(used_width, br.height())), Rounding::same(3.0), accent);
                            bar_p.rect_filled(egui::Rect::from_min_size(egui::pos2(br.min.x + used_width, br.min.y), egui::vec2(br.width() - used_width, br.height())), Rounding::same(3.0), p.bg_button);
                        } else {
                            bar_p.text(br.center(), egui::Align2::CENTER_CENTER, "Unavailable", egui::FontId::monospace(8.5), p.text_muted);
                        }

                        ui.add_space(8.0);

                        // 3 columns underneath
                        ui.columns(3, |cols| {
                            cols[0].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), accent);
                                    ui.label(RichText::new("REPORTED MEMORY").size(9.0).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                if total_vram_mb > 0 {
                                    ui.label(RichText::new(format!("{} MB", total_vram_mb)).size(14.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("GPU memory capacity").size(8.5).monospace().color(p.text_muted));
                                } else {
                                    ui.label(RichText::new("—").size(14.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("Unavailable").size(8.5).monospace().color(p.text_muted));
                                }
                            });

                            cols[1].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.info);
                                    ui.label(RichText::new("MEMORY USED").size(9.0).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                if total_vram_mb > 0 {
                                    ui.label(RichText::new(format!("{:.1} MB / {:.1} GB", used_vram_mb as f64, total_vram_mb as f64 / 1024.0)).size(14.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("Reported GPU memory").size(8.5).monospace().color(p.text_muted));
                                } else {
                                    ui.label(RichText::new("—").size(14.0).strong().color(p.text_primary));
                                    ui.label(RichText::new("Unavailable").size(8.5).monospace().color(p.text_muted));
                                }
                            });

                            cols[2].vertical(|ui| {
                                ui.horizontal(|ui| {
                                    let (d_rect, _) = ui.allocate_exact_size(egui::vec2(6.5, 6.5), egui::Sense::hover());
                                    ui.painter().rect_filled(d_rect, Rounding::same(1.5), p.text_muted);
                                    ui.label(RichText::new("BANDWIDTH").size(9.0).monospace().strong().color(p.text_muted));
                                });
                                ui.add_space(2.0);
                                ui.label(RichText::new("—").size(14.0).strong().color(p.text_primary));
                                ui.label(RichText::new("Unavailable").size(8.5).monospace().color(p.text_muted));
                            });
                        });
                    });

                ui.add_space(8.0);
                }

                // Show every display mode reported by the host collector.
                if !snap.displays.is_empty() {
                    Frame::none()
                        .fill(p.bg_card)
                        .rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.label(RichText::new(crate::lucide::MONITOR).size(12.0).color(accent));
                                ui.label(RichText::new("Detected displays").size(13.0).strong().color(p.text_primary));
                            });
                            ui.add_space(8.0);
                            for display in &snap.displays {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(RichText::new(&display.name).size(11.0).color(p.text_secondary));
                                    if display.resolution != "Not reported" {
                                        ui.label(RichText::new(&display.resolution).size(18.0).strong().color(p.text_primary));
                                    }
                                    if display.refresh_hz > 0.0 {
                                        ui.label(RichText::new(format!("@ {:.1} Hz", display.refresh_hz)).size(10.0).monospace().color(p.text_secondary));
                                    }
                                    if display.connector_type != "Not reported" {
                                        ui.label(RichText::new(&display.connector_type).size(10.0).monospace().color(p.text_muted));
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        });
                    ui.add_space(10.0);
                }

                ui.add_space(10.0);
            });
    }
}
