use super::*;

impl LauncherApp {
    // ─────────────────────────────────────────────────────────────────────────
    // SENSORS PAGE: Application Data Usage Dashboard
    // ─────────────────────────────────────────────────────────────────────────
    pub(in crate::app) fn render_sensors_data_usage_page(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);
        let mut interfaces: Vec<_> = snap.networks.iter().collect();
        interfaces.sort_by(|a, b| {
            (b.rx_rate_bps + b.tx_rate_bps)
                .partial_cmp(&(a.rx_rate_bps + a.tx_rate_bps))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });

        ScrollArea::vertical()
            .id_salt("sensors_data_usage_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(crate::lucide::ACTIVITY).size(15.0).color(accent));
                            ui.label(RichText::new("NETWORK").size(10.5).monospace().strong().color(p.text_muted));
                            ui.label(RichText::new(if snap.network_stats_available { format!("{} interfaces", interfaces.len()) } else { "Interface counters unavailable".to_string() }).size(10.5).color(p.text_secondary));
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new("Network data usage").size(18.0).strong().color(p.text_primary));
                        ui.label(
                            RichText::new(if host_id.is_some() { "Remote counters · all interfaces, including loopback and virtual" } else if cfg!(windows) { "Reported interfaces · some disconnected or virtual interfaces may be omitted" } else { "Local counters · all interfaces, including loopback and virtual" })
                                .size(10.5)
                                .color(p.text_muted),
                        );
                    });

                ui.add_space(4.0);
                ui.label(RichText::new("Traffic carried by both a VPN and its underlying interface can be counted twice.").size(10.0).color(p.text_muted));
                ui.add_space(6.0);

                let metrics = [
                    (crate::lucide::DOWNLOAD, "RECEIVED", sensor_network_bytes(snap, snap.total_rx_bytes), sensor_network_rate(snap, snap.total_rx_rate_bps)),
                    (crate::lucide::UPLOAD, "SENT", sensor_network_bytes(snap, snap.total_tx_bytes), sensor_network_rate(snap, snap.total_tx_rate_bps)),
                    (crate::lucide::ACTIVITY, "LIVE RATE", sensor_network_rate(snap, snap.total_rx_rate_bps + snap.total_tx_rate_bps), if snap.network_rates_available { "RX + TX".to_string() } else { "Unavailable".to_string() }),
                    (crate::lucide::WIFI, "ACTIVE", if snap.network_rates_available { interfaces.iter().filter(|net| net.rx_rate_bps > 0.0 || net.tx_rate_bps > 0.0).count().to_string() } else { "—".to_string() }, if snap.network_rates_available { "interfaces with traffic".to_string() } else { "Rate unavailable".to_string() }),
                ];
                sensor_card_columns(ui, metrics.len(), |ui, index| {
                    let (icon, label, value, detail) = &metrics[index];
                    Frame::none().fill(p.bg_card).rounding(Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(18.0, 15.0)).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(*icon).size(15.0).color(accent));
                                ui.label(RichText::new(*label).size(11.0).monospace().strong().color(p.text_muted));
                            });
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(RichText::new(value).size(30.0).strong().color(p.text_primary)).truncate()).on_hover_text(value);
                            ui.add(egui::Label::new(RichText::new(detail).size(11.0).monospace().color(p.text_secondary)).truncate()).on_hover_text(detail);
                        });
                });

                ui.add_space(10.0);

                Frame::none()
                    .fill(p.bg_card)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Interfaces").size(14.0).strong().color(p.text_primary));
                        ui.add_space(8.0);

                        if !snap.network_stats_available {
                            ui.label(RichText::new("Interface counters unavailable from this host.").size(11.0).color(p.text_muted));
                            return;
                        }
                        if interfaces.is_empty() {
                            ui.label(RichText::new("No network interface counters were reported.").size(11.0).color(p.text_muted));
                            return;
                        }

                        const NAME_W: f32 = 150.0;
                        const TYPE_W: f32 = 90.0;
                        const VALUE_W: f32 = 130.0;
                        const RATE_W: f32 = 150.0;
                        let max_total = interfaces.iter().map(|net| net.rx_bytes.saturating_add(net.tx_bytes)).max().unwrap_or(1).max(1);

                        if ui.available_width() < 850.0 {
                            for net in &interfaces {
                                let kind = if net.name == "lo" { "Loopback" } else { match net.is_wireless { Some(true) => "Wi-Fi", Some(false) => "Other", None => "Unknown" } };
                                let total = net.rx_bytes.saturating_add(net.tx_bytes);
                                Frame::none()
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&net.name).size(11.5).strong().color(p.text_primary));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                ui.label(RichText::new(kind).size(10.0).color(p.text_muted));
                                            });
                                        });
                                        ui.add_space(5.0);
                                        ui.columns(3, |cols| {
                                            for (column, label, value) in [
                                                (0, "Received", crate::sensors::format_bytes(net.rx_bytes)),
                                                (1, "Sent", crate::sensors::format_bytes(net.tx_bytes)),
                                                (2, "Total", crate::sensors::format_bytes(total)),
                                            ] {
                                                cols[column].vertical(|ui| {
                                                    ui.label(RichText::new(label).size(9.0).color(p.text_muted));
                                                    ui.label(RichText::new(value).size(10.5).monospace().color(p.text_secondary));
                                                });
                                            }
                                        });
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new("Live").size(9.0).color(p.text_muted));
                                            ui.label(RichText::new(format!("↓ {}  ↑ {}", sensor_network_rate(snap, net.rx_rate_bps), sensor_network_rate(snap, net.tx_rate_bps))).size(10.0).monospace().color(p.text_secondary));
                                        });
                                    });
                                ui.add_space(6.0);
                            }
                        } else {
                        ScrollArea::horizontal()
                            .id_salt("data_usage_interfaces_h")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("data_usage_interfaces_grid")
                                    .num_columns(6)
                                    .spacing(egui::vec2(12.0, 6.0))
                                    .show(ui, |ui| {
                                        for (width, heading) in [
                                            (NAME_W, "Interface"),
                                            (TYPE_W, "Type"),
                                            (VALUE_W, "Received"),
                                            (VALUE_W, "Sent"),
                                            (VALUE_W, "Total"),
                                            (RATE_W, "Live rate"),
                                        ] {
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
                                            ui.painter().text(
                                                egui::pos2(if heading == "Interface" { rect.left() } else { rect.right() }, rect.center().y),
                                                if heading == "Interface" { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER },
                                                heading,
                                                egui::FontId::proportional(10.0),
                                                p.text_muted,
                                            );
                                        }
                                        ui.end_row();

                                        for net in &interfaces {
                                            let (name_rect, _) = ui.allocate_exact_size(egui::vec2(NAME_W, 28.0), egui::Sense::hover());
                                            ui.painter().text(name_rect.left_center(), egui::Align2::LEFT_CENTER, &net.name, egui::FontId::proportional(11.5), p.text_primary);

                                            let (type_rect, _) = ui.allocate_exact_size(egui::vec2(TYPE_W, 28.0), egui::Sense::hover());
                                            let kind = if net.name == "lo" { "Loopback" } else { match net.is_wireless { Some(true) => "Wi-Fi", Some(false) => "Other", None => "Unknown" } };
                                            ui.painter().text(type_rect.right_center(), egui::Align2::RIGHT_CENTER, kind, egui::FontId::proportional(10.5), p.text_secondary);

                                            for (width, value) in [
                                                (VALUE_W, crate::sensors::format_bytes(net.rx_bytes)),
                                                (VALUE_W, crate::sensors::format_bytes(net.tx_bytes)),
                                            ] {
                                                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
                                                ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, value, egui::FontId::monospace(11.0), p.text_secondary);
                                            }

                                            let total = net.rx_bytes.saturating_add(net.tx_bytes);
                                            let (total_rect, _) = ui.allocate_exact_size(egui::vec2(VALUE_W, 28.0), egui::Sense::hover());
                                            let bar = egui::Rect::from_min_size(
                                                egui::pos2(total_rect.left(), total_rect.center().y - 1.5),
                                                egui::vec2(34.0, 3.0),
                                            );
                                            let bar_width = 34.0 * total as f32 / max_total as f32;
                                            ui.painter().rect_filled(bar, Rounding::same(1.5), p.bg_button);
                                            ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar_width, bar.height())), Rounding::same(1.5), accent);
                                            ui.painter().text(total_rect.right_center(), egui::Align2::RIGHT_CENTER, crate::sensors::format_bytes(total), egui::FontId::monospace(11.0), p.text_primary);

                                            let rx_rate = sensor_network_rate(snap, net.rx_rate_bps);
                                            let tx_rate = sensor_network_rate(snap, net.tx_rate_bps);
                                            let (rate_rect, _) = ui.allocate_exact_size(egui::vec2(RATE_W, 28.0), egui::Sense::hover());
                                            ui.painter().text(rate_rect.right_center(), egui::Align2::RIGHT_CENTER, format!("↓ {rx_rate}  ↑ {tx_rate}"), egui::FontId::monospace(10.5), p.text_secondary);
                                            ui.end_row();
                                        }
                                    });
                            });
                        }
                    });
            });
    }
}
