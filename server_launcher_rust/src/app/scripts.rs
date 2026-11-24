use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_scripts_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.label(RichText::new("Custom scripts").size(12.0).strong().color(p.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new(format!("{} New script", crate::lucide::PLUS)).size(11.0).color(p.accent_text),
                    )
                    .fill(p.accent).stroke(Stroke::NONE)
                    .rounding(Rounding::same(8.0)).min_size(egui::vec2(94.0, 24.0));
                    if ui.add(btn_new).clicked() {
                        self.script_modal.open_create(host_id.map(|s| s.to_string()));
                    }
                });
            });

            ui.add_space(6.0);

            // Match the visible search surface and muted hints used elsewhere.
            Frame::none().fill(p.bg_button).stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut self.script_search_query)
                            .font(egui::FontId::proportional(11.5)).text_color(p.text_secondary)
                            .hint_text(RichText::new("Filter scripts…").color(p.text_muted))
                            .frame(false).margin(egui::Vec2::ZERO).clip_text(true).desired_width(ui.available_width()));
                    });
                });

            ui.add_space(8.0);

            let q = self.script_search_query.trim().to_lowercase();
            let machine_scripts: Vec<CustomScript> = self
                .scripts
                .iter()
                .filter(|s| {
                    s.host_id.as_deref() == host_id
                        && (q.is_empty() || s.name.to_lowercase().contains(&q) || s.script.to_lowercase().contains(&q))
                })
                .cloned()
                .collect();

            if machine_scripts.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No scripts found").size(12.0).color(p.text_muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Click '+ New Script' to create one.").size(11.0).color(p.text_muted));
                });
                return;
            }

            ScrollArea::vertical()
                .id_salt("scripts_left_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut to_run = None;
                    let mut to_stop = None;
                    let mut to_edit = None;
                    let mut to_delete = None;

                    for s in &machine_scripts {
                        let is_selected = self.selected_script_id.as_deref().or_else(|| self.scripts.iter().find(|script| script.host_id.as_deref() == host_id).map(|script| script.id.as_str())) == Some(s.id.as_str());
                        let is_running = self
                            .script_executions
                            .get(&s.id)
                            .and_then(|e| e.status.lock().ok())
                            .map(|st| matches!(*st, ScriptRunStatus::Running))
                            .unwrap_or(false);

                        ui.push_id(("script_card", host_id, &s.id), |ui| Frame::none()
                            .fill(if is_selected { p.bg_card_active } else { p.bg_card })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.set_min_height(20.0);
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(dot.center(), 3.0, if is_running { p.success } else { p.text_muted });
                                    let shell_width = 76.0;
                                    let name_width = (ui.available_width() - shell_width - ui.spacing().item_spacing.x).max(0.0);
                                    if render_server_card_title(ui, &s.name, name_width, 20.0, is_selected, &p).clicked() {
                                        self.selected_script_id = Some(s.id.clone());
                                    }
                                    ui.add_sized([shell_width, 20.0], egui::Label::new(
                                        RichText::new(&s.shell).size(10.0).monospace().color(p.text_muted))
                                        .truncate().halign(egui::Align::Max)).on_hover_text(&s.shell);
                                });

                                if s.autostart || !s.cwd.is_empty() {
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        if s.autostart {
                                            ui.label(
                                                RichText::new(format!("{} Startup", crate::lucide::ZAP))
                                                    .size(10.0)
                                                    .color(p.warning),
                                            );
                                        }
                                        if !s.cwd.is_empty() {
                                            let cwd_w = (ui.available_width() - 4.0).max(40.0);
                                            ui.add_sized(
                                                [cwd_w, 14.0],
                                                egui::Label::new(
                                                    RichText::new(format!("{} {}", crate::lucide::FOLDER, s.cwd))
                                                        .size(10.0)
                                                        .color(p.text_muted),
                                                )
                                                .truncate().halign(egui::Align::Min),
                                            ).on_hover_text(&s.cwd);
                                        }
                                    });
                                }

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    ui.set_min_height(24.0);
                                    let button = if is_running {
                                        egui::Button::new(RichText::new(format!("{} Stop", crate::lucide::STOP_FILLED)).size(10.5).color(Color32::WHITE))
                                            .fill(p.danger)
                                    } else {
                                        egui::Button::new(RichText::new(format!("{} Run", crate::lucide::PLAY_FILLED)).size(10.5).color(p.accent_text))
                                            .fill(p.accent)
                                    }.stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(68.0, 24.0));
                                    if ui.add(button).clicked() {
                                        if is_running { to_stop = Some(s.id.clone()); }
                                        else { to_run = Some(s.clone()); }
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let delete = egui::Button::new(RichText::new(crate::lucide::TRASH).size(11.0).color(p.text_muted))
                                            .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                        if ui.add_sized([24.0, 24.0], delete).on_hover_text("Delete script").clicked() {
                                            to_delete = Some(s.id.clone());
                                        }
                                        let edit = egui::Button::new(RichText::new(crate::lucide::PENCIL).size(11.0).color(p.text_secondary))
                                            .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                        if ui.add_sized([24.0, 24.0], edit).on_hover_text("Edit script").clicked() {
                                            to_edit = Some(s.clone());
                                        }
                                    });
                                });
                            }));

                        ui.add_space(10.0);
                    }

                    if let Some(s) = to_run {
                        self.request_run_script(&s, host_id.map(|h| h.to_string()));
                    }

                    if let Some(id) = to_stop {
                        if let Some(exec) = self.script_executions.get_mut(&id) {
                            exec.stop();
                        }
                    }

                    if let Some(s) = to_edit {
                        self.script_modal.open_edit(&s);
                    }

                    if let Some(id) = to_delete {
                        self.scripts.retain(|s| s.id != id);
                        self.script_executions.remove(&id);
                        if self.selected_script_id.as_deref() == Some(&id) {
                            self.selected_script_id = None;
                        }
                        self.persist_config();
                    }
                });
        });
    }

    pub(in crate::app) fn render_scripts_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        let selected_id = self.selected_script_id.clone().or_else(|| {
            self.scripts
                .iter()
                .find(|s| s.host_id.as_deref() == host_id)
                .map(|s| s.id.clone())
        });

        let script = selected_id.as_ref().and_then(|id| {
            self.scripts.iter().find(|s| &s.id == id).cloned()
        });

        match script {
            Some(s) => {
                let is_running = self
                    .script_executions
                    .get(&s.id)
                    .and_then(|e| e.status.lock().ok())
                    .map(|st| matches!(*st, ScriptRunStatus::Running))
                    .unwrap_or(false);

                ui.vertical(|ui| {
                    // Header Bar
                    Frame::none()
                        .fill(p.bg_card)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SCRIPT).size(20.0).color(p.accent));
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&s.name).size(15.0).strong().color(p.text_primary));
                                        ui.label(
                                            RichText::new(format!("({})", s.shell))
                                                .size(11.0)
                                                .color(p.text_muted),
                                        );
                                        if s.autostart {
                                            ui.label(
                                                RichText::new(format!("{} Runs at Startup", crate::lucide::ZAP))
                                                    .size(10.5)
                                                    .color(Color32::from_rgb(250, 204, 21)),
                                            );
                                        }
                                    });
                                    if !s.cwd.is_empty() {
                                        ui.label(
                                            RichText::new(format!("CWD: {}", s.cwd))
                                                .size(10.5)
                                                .color(p.text_muted),
                                        );
                                    }
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn_edit = egui::Button::new(RichText::new(format!("{} Edit", crate::lucide::PENCIL)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_edit).clicked() {
                                        self.script_modal.open_edit(&s);
                                    }

                                    let btn_clear = egui::Button::new(RichText::new(format!("{} Clear Logs", crate::lucide::TRASH)).size(11.0).color(p.text_secondary))
                                        .fill(p.bg_button);
                                    if ui.add(btn_clear).clicked() {
                                        if let Some(exec) = self.script_executions.get_mut(&s.id) {
                                            if let Ok(mut l) = exec.logs.lock() {
                                                l.clear();
                                            }
                                        }
                                    }

                                    if is_running {
                                        let btn_stop = egui::Button::new(
                                            RichText::new(format!("{} Stop Script", crate::lucide::STOP)).size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(p.danger)
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_stop).clicked() {
                                            if let Some(exec) = self.script_executions.get_mut(&s.id) {
                                                exec.stop();
                                            }
                                        }
                                    } else {
                                        let btn_run = egui::Button::new(
                                            RichText::new(format!("{} Run Script", crate::lucide::PLAY)).size(11.5).strong().color(Color32::WHITE),
                                        )
                                        .fill(Color32::from_rgb(16, 185, 129))
                                        .rounding(Rounding::same(4.0));
                                        if ui.add(btn_run).clicked() {
                                            self.request_run_script(&s, host_id.map(|h| h.to_string()));
                                        }
                                    }
                                });
                            });
                        });

                    ui.add_space(8.0);

                    // Console Output Area
                    let logs = self
                        .script_executions
                        .get(&s.id)
                        .map(|e| e.logs.clone());

                    Frame::none()
                        .fill(p.terminal_bg)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(14.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ScrollArea::vertical()
                                .id_salt("script_console_scroll")
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let Some(logs_arc) = logs {
                                        if let Ok(l) = logs_arc.lock() {
                                            if l.is_empty() {
                                                ui.label(
                                                    RichText::new(format!("Script ready. Click '{} Run Script' to execute.", crate::lucide::PLAY))
                                                        .size(11.5)
                                                        .color(Color32::from_rgb(100, 110, 130)),
                                                );
                                            } else {
                                                for entry in l.iter() {
                                                    let color = match entry.kind {
                                                        LogKind::Launcher => Color32::from_rgb(96, 165, 250),
                                                        LogKind::Stdout => Color32::from_rgb(220, 225, 235),
                                                        LogKind::Stderr | LogKind::Error => Color32::from_rgb(248, 113, 113),
                                                        LogKind::Warning => Color32::from_rgb(250, 204, 21),
                                                        LogKind::Stdin => Color32::from_rgb(52, 211, 153),
                                                    };
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            RichText::new(entry.timestamp.as_ref())
                                                                .size(10.5)
                                                                .color(Color32::from_rgb(90, 100, 120))
                                                                .monospace(),
                                                        );
                                                        ui.label(
                                                            RichText::new(entry.text.as_ref())
                                                                .size(11.5)
                                                                .color(color)
                                                                .monospace(),
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                    } else {
                                        ui.label(
                                            RichText::new(format!("Script ready. Click '{} Run Script' to execute.", crate::lucide::PLAY))
                                                .size(11.5)
                                                .color(Color32::from_rgb(100, 110, 130)),
                                        );
                                    }
                                });
                        });
                });
            }
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new(format!("{} Custom Scripts", crate::lucide::SCRIPT)).size(16.0).strong().color(p.text_primary));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Select a script from the left or click '+ New Script' to create one.\nScripts can execute any command or interpreter and can run automatically on launch.")
                            .size(12.0)
                            .color(p.text_muted),
                    );
                });
            }
        }
    }
}
