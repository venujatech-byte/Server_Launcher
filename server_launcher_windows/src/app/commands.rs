use super::*;

impl LauncherApp {
    // ═════════════════════════════════════════════════════════════════════════
    // COMMANDS TAB: Saved Snippets Library & Interactive Terminal
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_commands_left_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.label(RichText::new("Saved commands").size(12.0).strong().color(p.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_new = egui::Button::new(
                        RichText::new("+ Save").size(11.0).color(p.accent_text),
                    )
                    .fill(p.accent)
                    .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                    if ui.add(btn_new).on_hover_text("Save a new custom command snippet").clicked() {
                        self.command_modal.open_create(host_id.map(|s| s.to_string()));
                    }

                    // Export Button (left of Save Command)
                    let btn_export = egui::Button::new(
                        RichText::new(format!("{} Export", crate::lucide::DOWNLOAD))
                            .size(11.0)
                            .color(p.text_secondary),
                    )
                    .fill(p.bg_button)
                    .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                    if ui.add(btn_export).on_hover_text("Export saved commands as a JSON file").clicked() {
                        self.export_commands_to_file_dialog(host_id);
                    }

                    // Import Button (left of Export)
                    let btn_import = egui::Button::new(
                        RichText::new(format!("{} Import", crate::lucide::UPLOAD))
                            .size(11.0)
                            .color(p.text_secondary),
                    )
                    .fill(p.bg_button)
                    .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                    if ui.add(btn_import).on_hover_text("Import commands from a JSON file").clicked() {
                        self.import_commands_from_file_dialog(host_id);
                    }
                });
            });

            if let Some((ref msg, is_err, ref time)) = self.saved_command_status_msg {
                if time.elapsed().as_secs() < 4 {
                    ui.add_space(4.0);
                    let bg = if is_err { p.danger_subtle } else { p.success_subtle };
                    let fg = if is_err { p.danger } else { p.success };
                    Frame::none()
                        .fill(bg)
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 3.5))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg).size(11.0).color(fg));
                        });
                }
            }

            ui.add_space(6.0);

            // Match the filter surface used by the scripts sidebar.
            Frame::none().fill(p.bg_button).stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(p.text_muted));
                        ui.add(egui::TextEdit::singleline(&mut self.saved_command_search)
                            .font(egui::FontId::proportional(11.5)).text_color(p.text_secondary)
                            .hint_text(RichText::new("Filter commands…").color(p.text_muted))
                            .frame(false).margin(egui::Vec2::ZERO).clip_text(true).desired_width(ui.available_width()));
                    });
                });

            ui.add_space(6.0);

            // Category Filter Row: single-pass O(N) counting across saved commands
            let mut cat_counts: BTreeMap<&str, usize> = BTreeMap::new();
            let mut total_for_host = 0usize;
            for c in &self.saved_commands {
                if c.host_id.as_deref() == host_id {
                    total_for_host += 1;
                    *cat_counts.entry(c.category.as_str()).or_insert(0) += 1;
                }
            }

            let mut categories: Vec<(String, usize)> = Vec::with_capacity(cat_counts.len() + 1);
            categories.push(("All".to_string(), total_for_host));
            for (cat, count) in cat_counts {
                categories.push((cat.to_string(), count));
            }

            if !categories.iter().any(|(cat, _)| cat == &self.saved_command_category) {
                self.saved_command_category = "All".to_string();
            }

            // ComboBox selector for quick direct access to any category
            ui.horizontal(|ui| {
                ui.set_min_height(26.0);
                ui.label(RichText::new("Category").size(11.0).color(p.text_muted));
                let cur_cat = &self.saved_command_category;
                let count = categories
                    .iter()
                    .find(|(cat, _)| cat == cur_cat)
                    .map(|(_, count)| *count)
                    .unwrap_or(0);
                let combo_label = format!("{} ({})", cur_cat, count);

                egui::ComboBox::from_id_salt("cmd_cat_combo")
                    .selected_text(RichText::new(combo_label).size(11.0).color(p.text_secondary))
                    .width((ui.available_width() - 4.0).max(40.0))
                    .height(260.0).wrap_mode(egui::TextWrapMode::Truncate)
                    .show_ui(ui, |ui| {
                        ui.spacing_mut().interact_size.y = 24.0;
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        ui.visuals_mut().selection.bg_fill = p.accent_subtle;
                        ui.visuals_mut().selection.stroke = Stroke::new(1.0_f32, p.accent);
                        ui.visuals_mut().widgets.hovered.weak_bg_fill = p.bg_button_hover;
                        for (cat, cat_count) in &categories {
                            let label = format!("{} ({})", cat, cat_count);
                            let selected = self.saved_command_category == *cat;
                            ui.selectable_value(&mut self.saved_command_category, cat.clone(),
                                RichText::new(&label).size(11.5).color(if selected { p.accent } else { p.text_secondary })).on_hover_text(&label);
                        }
                    });
            });

            ui.add_space(4.0);

            // Single-line horizontal scrollable pill bar (never wraps vertically, keeps cards visible!)
            ScrollArea::horizontal()
                .id_salt("cmd_cat_pills_hscroll")
                .auto_shrink([false, true])
                .max_height(32.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (cat, _) in &categories {
                            let is_active = self.saved_command_category == *cat;
                            let bg = if is_active { p.accent_subtle } else { p.bg_subtle };
                            let fg = if is_active { p.accent } else { p.text_secondary };
                            let btn = egui::Button::new(RichText::new(cat).size(10.5).color(fg))
                                .fill(bg)
                                .stroke(Stroke::NONE).rounding(Rounding::same(8.0)).min_size(egui::vec2(0.0, 24.0));
                            if ui.add(btn).clicked() {
                                self.saved_command_category = cat.clone();
                            }
                        }
                    });
                });

            ui.add_space(6.0);

            let q = self.saved_command_search.trim().to_lowercase();
            let cat_filter = &self.saved_command_category;
            let q_empty = q.is_empty();

            let filtered_indices: Vec<usize> = self
                .saved_commands
                .iter()
                .enumerate()
                .filter_map(|(idx, c)| {
                    if c.host_id.as_deref() != host_id {
                        return None;
                    }
                    if cat_filter != "All" && &c.category != cat_filter {
                        return None;
                    }
                    if !q_empty {
                        let matches = c.name.to_lowercase().contains(&q)
                            || c.command.to_lowercase().contains(&q)
                            || c.description.to_lowercase().contains(&q);
                        if !matches {
                            return None;
                        }
                    }
                    Some(idx)
                })
                .collect();

            if filtered_indices.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No saved commands found").size(12.0).color(p.text_muted));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Click '+ Save Command' to add snippets.").size(11.0).color(p.text_muted));
                });
                return;
            }

            ui.spacing_mut().item_spacing.y = 10.0;
            ScrollArea::vertical()
                .id_salt("saved_commands_scroll")
                .auto_shrink([false, false])
                .show_rows(ui, 90.0, filtered_indices.len(), |ui, row_range| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    let mut to_run_cmd: Option<(String, bool)> = None;
                    let mut to_edit = None;
                    let mut to_delete = None;

                    let mut to_toggle_external: Option<(String, bool)> = None;

                    for idx in row_range {
                        let c = &self.saved_commands[filtered_indices[idx]];
                        ui.push_id(("saved_command_card", host_id, &c.id), |ui| Frame::none()
                            .fill(p.bg_card).stroke(Stroke::NONE).rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_max_width(ui.available_width());
                                // Four compact rows plus padding match the virtualized row height.
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
                                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                                ui.spacing_mut().interact_size.y = 18.0;
                                ui.set_min_height(74.0);
                                ui.horizontal(|ui| {
                                    let category_width = (ui.available_width() * 0.32).min(112.0);
                                    let name_width = (ui.available_width() - category_width - ui.spacing().item_spacing.x).max(0.0);
                                    ui.add_sized([name_width, 18.0], egui::Label::new(
                                        RichText::new(&c.name).size(12.0).strong().color(p.text_primary))
                                        .truncate().halign(egui::Align::Min)).on_hover_text(&c.name);
                                    ui.add_sized([category_width, 18.0], egui::Label::new(
                                        RichText::new(&c.category).size(10.0).color(p.text_muted))
                                        .truncate().halign(egui::Align::Max)).on_hover_text(&c.category);
                                });
                                ui.add_sized([ui.available_width(), 16.0], egui::Label::new(
                                    RichText::new(&c.command).size(11.0).monospace().color(p.info))
                                    .truncate().halign(egui::Align::Min)).on_hover_text(&c.command);
                                ui.add_sized([ui.available_width(), 12.0], egui::Label::new(
                                    RichText::new(&c.description).size(10.0).color(p.text_muted))
                                    .truncate().halign(egui::Align::Min)).on_hover_text(&c.description);
                                ui.horizontal(|ui| {
                                    ui.set_min_height(22.0);
                                    let mut run_in_ext = c.run_in_terminal;
                                    // Button is only "Run"; the checkbox chooses its destination.
                                    let run = egui::Button::new(RichText::new(format!("{} Run", crate::lucide::PLAY_FILLED)).size(10.5).color(p.accent_text))
                                        .fill(p.accent).stroke(Stroke::NONE).rounding(Rounding::same(8.0))
                                        .min_size(egui::vec2(60.0, 22.0));
                                    let run_hover = if run_in_ext { "Execute in external desktop terminal window" } else { "Execute in terminal" };
                                    if ui.add(run).on_hover_text(run_hover).clicked() {
                                        to_run_cmd = Some((c.command.clone(), run_in_ext));
                                    }
                                    let label_color = if run_in_ext { p.info } else { p.text_secondary };
                                    if ui.checkbox(&mut run_in_ext, RichText::new("External").size(10.5).color(label_color))
                                        .on_hover_text("Run this command in an external desktop terminal").changed() {
                                        to_toggle_external = Some((c.id.clone(), run_in_ext));
                                    }
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let delete = egui::Button::new(RichText::new(crate::lucide::TRASH).size(10.5).color(p.text_muted))
                                            .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                        if ui.add_sized([22.0, 22.0], delete).on_hover_text("Delete command").clicked() {
                                            to_delete = Some(c.id.clone());
                                        }
                                        let edit = egui::Button::new(RichText::new(crate::lucide::PENCIL).size(10.5).color(p.text_secondary))
                                            .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                        if ui.add_sized([22.0, 22.0], edit).on_hover_text("Edit command").clicked() {
                                            to_edit = Some(c.clone());
                                        }
                                        let copy = egui::Button::new(RichText::new(crate::lucide::COPY).size(10.5).color(p.text_secondary))
                                            .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                        if ui.add_sized([22.0, 22.0], copy).on_hover_text("Copy command").clicked() {
                                            ui.ctx().copy_text(c.command.clone());
                                        }
                                    });
                                });
                            }));
                    }

                    if let Some((cmd_str, in_external)) = to_run_cmd {
                        if in_external {
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    let _ = crate::remote::launch_external_ssh_command(&host, &cmd_str);
                                }
                            } else {
                                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                                let _ = crate::commands::launch_external_command(&cmd_str, &cwd);
                            }
                        } else {
                            if let Some(hid) = host_id {
                                if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                                    let term = self.remote_terminals.entry(hid.to_string()).or_default();
                                    term.execute_remote(&host, &cmd_str);
                                    term.request_focus = true;
                                }
                            } else {
                                self.local_terminal.execute_local(&cmd_str, "");
                                self.local_terminal.request_focus = true;
                            }
                        }
                    }

                    if let Some(c) = to_edit {
                        self.command_modal.open_edit(&c);
                    }

                    if let Some(id) = to_delete {
                        self.saved_commands.retain(|c| c.id != id);
                        self.persist_config();
                    }

                    if let Some((id, val)) = to_toggle_external {
                        if let Some(cmd) = self.saved_commands.iter_mut().find(|c| c.id == id) {
                            cmd.run_in_terminal = val;
                            self.persist_config();
                        }
                    }
                });
        });
    }

    pub(in crate::app) fn render_commands_right_panel(&mut self, ui: &mut Ui, host_id: Option<&str>) {
        let p = self.palette();
        let host_opt = host_id.and_then(|hid| self.ssh_hosts.iter().find(|h| h.id == hid).cloned());

        let term = if let Some(h) = &host_opt {
            self.remote_terminals.entry(h.id.clone()).or_default()
        } else {
            &mut self.local_terminal
        };

        let is_running = term.is_running();
        let stat_msg = term.status.lock().ok().map(|st| match &*st {
            TerminalCommandStatus::Idle => format!("{} Ready", crate::lucide::CIRCLE_DOT),
            TerminalCommandStatus::Running(c) => format!("{} Running: {}", crate::lucide::PLAY, c),
            TerminalCommandStatus::Success(code) => format!("{} Exit code {}", crate::lucide::CHECK, code),
            TerminalCommandStatus::Failed(err) => format!("{} Failed: {}", crate::lucide::X, err),
        }).unwrap_or_default();

        let title = if let Some(h) = &host_opt {
            format!("SSH terminal · {}@{}:{}", h.user, h.host, h.port)
        } else {
            "Local terminal".to_string()
        };

        let cwd_display = if host_opt.is_none() {
            let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(std::path::PathBuf::from);
            if let Some(home) = home_dir {
                if let Ok(rel) = term.cwd.strip_prefix(&home) {
                    let rel_str = rel.display().to_string();
                    if rel_str.is_empty() {
                        "~".to_string()
                    } else {
                        format!("~/{}", rel_str)
                    }
                } else {
                    term.cwd.display().to_string()
                }
            } else {
                term.cwd.display().to_string()
            }
        } else {
            "~".to_string()
        };

        let prompt_str = term.get_prompt_str(host_opt.as_ref());

        ui.vertical(|ui| {
            // ═════════════════════════════════════════════════════════════════
            // Header Bar
            // ═════════════════════════════════════════════════════════════════
            Frame::none()
                .fill(p.bg_card)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        // Row 1: Title and Action Buttons (No overlap possible)
                        ui.horizontal(|ui| {
                            ui.set_min_height(24.0);
                            let action_width = if is_running { 164.0 } else if host_opt.is_none() { 188.0 } else { 68.0 };
                            let title_width = (ui.available_width() - action_width - 28.0).max(0.0);
                            ui.label(RichText::new(crate::lucide::TERMINAL).size(14.0).color(p.accent));
                            ui.add_sized([title_width, 24.0],
                                egui::Label::new(
                                    RichText::new(&title).size(12.5).strong().color(p.text_primary),
                                )
                                .truncate().halign(egui::Align::Min),
                            ).on_hover_text(&title);

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn_clear = egui::Button::new(
                                    RichText::new(format!("{} Clear", crate::lucide::TRASH))
                                        .size(11.0)
                                        .color(p.text_secondary),
                                )
                                .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                if ui.add_sized([60.0, 24.0], btn_clear).on_hover_text("Clear output · Ctrl+L").clicked() {
                                    term.clear();
                                    term.request_focus = true;
                                }

                                if is_running {
                                    let btn_stop = egui::Button::new(
                                        RichText::new(format!("{} Interrupt", crate::lucide::STOP_FILLED))
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(p.danger).stroke(Stroke::NONE)
                                    .rounding(Rounding::same(6.0));
                                    if ui.add_sized([88.0, 24.0], btn_stop).on_hover_text("Interrupt command · Ctrl+C").clicked() {
                                        term.interrupt();
                                        term.request_focus = true;
                                    }
                                } else if host_opt.is_none() {
                                    let btn_ext = egui::Button::new(
                                        RichText::new(format!("{} Terminal", crate::lucide::EXTERNAL_LINK))
                                            .size(11.0)
                                            .color(p.text_secondary),
                                    )
                                    .fill(p.bg_button).stroke(Stroke::NONE).rounding(Rounding::same(6.0));
                                    if ui
                                        .add_sized([104.0, 24.0], btn_ext)
                                        .on_hover_text("Open native system terminal window in current directory")
                                        .clicked()
                                    {
                                        let _ = crate::commands::open_system_terminal(&term.cwd);
                                    }
                                }
                            });
                        });

                        ui.add_space(3.0);

                        // Row 2: Status & Current Directory Sub-bar (Full width available)
                        ui.horizontal(|ui| {
                            ui.add_space(22.0); // Align with title text past terminal icon
                            let status_width = ui.fonts(|fonts| fonts.layout_no_wrap(stat_msg.clone(),
                                egui::FontId::proportional(11.0), p.text_muted).size().x)
                                .min((ui.available_width() * 0.4).min(180.0));
                            ui.add_sized([status_width, 16.0], egui::Label::new(
                                RichText::new(&stat_msg)
                                    .size(11.0)
                                    .color(if is_running {
                                        p.success
                                    } else {
                                        p.text_muted
                                    })).truncate().halign(egui::Align::Min),
                            ).on_hover_text(&stat_msg);
                            ui.label(RichText::new("•").size(11.0).color(p.text_muted));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{} {}", crate::lucide::FOLDER, cwd_display))
                                        .size(11.0)
                                        .color(p.info)
                                        .monospace(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&cwd_display);
                        });
                    });
                });

            ui.add_space(8.0);

            // ═════════════════════════════════════════════════════════════════
            // Integrated Dark Terminal Window (Output + Prompt at bottom)
            // ═════════════════════════════════════════════════════════════════
            let total_h = (ui.available_height() - 16.0).max(0.0);
            let logs_clone = term.logs.clone();
            let mut should_focus = false;

            Frame::none()
                .fill(p.terminal_bg)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(total_h);
                    let prompt_bar_h = 36.0;
                    let scroll_h = (total_h - prompt_bar_h - 16.0).max(0.0);

                    // 1. Scrollable Output Log Area
                    let scroll_resp = ScrollArea::vertical()
                        .id_salt(("cmd_terminal_scroll", host_id))
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .min_scrolled_height(scroll_h).max_height(scroll_h).drag_to_scroll(false)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if let Ok(l) = logs_clone.lock() {
                                if l.is_empty() {
                                    ui.label(
                                        RichText::new("Terminal ready. Type a command below or press Tab for auto-completion.")
                                            .size(11.5)
                                            .color(p.text_muted),
                                    );
                                } else {
                                    ui.add(egui::Label::new(cached_inline_console_galley(ui, 0, &l, &p)).selectable(false));
                                }
                            }
                        });

                    // Clicking anywhere in the terminal scroll area focuses the input prompt
                    if scroll_resp.inner_rect.contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default())
                        && ui.input(|i| i.pointer.primary_clicked())
                    {
                        should_focus = true;
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(2.0);

                    // 2. Integrated Interactive Terminal Prompt Row
                    ui.horizontal(|ui| {
                        ui.set_min_height(28.0);
                        ui.visuals_mut().extreme_bg_color = p.bg_button;
                        let prompt_width = (ui.available_width() * 0.25).min(140.0);
                        let prompt_label = if is_running {
                            format!("{} stdin > ", crate::lucide::CHEVRON_RIGHT)
                        } else {
                            prompt_str.clone()
                        };

                        let prompt_color = if is_running {
                            p.success
                        } else {
                            p.accent
                        };

                        ui.add_sized([prompt_width, 28.0], egui::Label::new(
                            RichText::new(&prompt_label)
                                .size(12.5)
                                .strong()
                                .monospace()
                                .color(prompt_color)).truncate().halign(egui::Align::Min),
                        ).on_hover_text(&prompt_label);

                        let hint = if is_running {
                            "Send input… · Ctrl+C to interrupt"
                        } else {
                            "Enter command… · Tab to complete"
                        };

                        let input_width = (ui.available_width() - 64.0 - ui.spacing().item_spacing.x).max(0.0);
                        let text_edit = egui::TextEdit::singleline(&mut term.input_text)
                            .id_salt(("commands_prompt", host_id))
                            .font(egui::FontId::monospace(11.5)).text_color(p.text_primary)
                            .margin(egui::vec2(8.0, 5.0)).min_size(egui::vec2(0.0, 28.0))
                            .hint_text(RichText::new(hint).color(p.text_muted))
                            .clip_text(true).desired_width(input_width);

                        let resp = ui.add_sized([input_width, 28.0], text_edit);

                        if term.request_focus || should_focus {
                            resp.request_focus();
                            term.request_focus = false;
                        }

                        // Enter submission (keeps focus!)
                        let enter_hit = (resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                            || (resp.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)));

                        let mut to_execute = None;
                        if enter_hit {
                            resp.request_focus();
                            let cmd = term.input_text.clone();
                            term.input_text.clear();
                            to_execute = Some(cmd);
                        }

                        // Ctrl + C (SIGINT or cancel line)
                        let ctrl_c = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::C))
                            || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::C));
                        if ctrl_c {
                            if is_running {
                                term.interrupt();
                            } else {
                                term.cancel_line();
                            }
                            resp.request_focus();
                        }

                        // Ctrl + L (Clear screen)
                        let ctrl_l = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::L))
                            || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::L));
                        if ctrl_l {
                            term.clear();
                            resp.request_focus();
                        }

                        // Tab (Auto-completion for files and paths)
                        let tab_hit = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
                        if tab_hit && !is_running {
                            term.autocomplete_path();
                            resp.request_focus();
                        }

                        // Up / Down arrow history
                        if resp.has_focus() {
                            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                                if !term.history.is_empty() {
                                    let next_idx = match term.history_idx {
                                        None => term.history.len().saturating_sub(1),
                                        Some(i) => i.saturating_sub(1),
                                    };
                                    if let Some(cmd) = term.history.get(next_idx).cloned() {
                                        term.input_text = cmd;
                                        term.history_idx = Some(next_idx);
                                    }
                                }
                            } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                                if let Some(i) = term.history_idx {
                                    if i + 1 < term.history.len() {
                                        let next_idx = i + 1;
                                        if let Some(cmd) = term.history.get(next_idx).cloned() {
                                            term.input_text = cmd;
                                            term.history_idx = Some(next_idx);
                                        }
                                    } else {
                                        term.input_text.clear();
                                        term.history_idx = None;
                                    }
                                }
                            }
                        }

                        // Inline Action Button
                        if is_running {
                            let btn_stop = egui::Button::new(
                                RichText::new(format!("{} Stop", crate::lucide::STOP_FILLED)).size(11.0).color(Color32::WHITE),
                            )
                            .fill(p.danger).stroke(Stroke::NONE)
                            .rounding(Rounding::same(6.0));
                            if ui.add_sized([64.0, 28.0], btn_stop).on_hover_text("Interrupt running process (Ctrl+C)").clicked() {
                                term.interrupt();
                                resp.request_focus();
                            }
                        } else {
                            let btn_run = egui::Button::new(
                                RichText::new(format!("{} Run", crate::lucide::PLAY_FILLED)).size(11.0).color(p.accent_text),
                            )
                            .fill(p.accent).stroke(Stroke::NONE)
                            .rounding(Rounding::same(6.0));
                            if ui.add_sized([64.0, 28.0], btn_run).on_hover_text("Run command (Enter)").clicked() {
                                to_execute = Some(term.input_text.clone());
                                term.input_text.clear();
                                resp.request_focus();
                            }
                        }

                        // Execution dispatch
                        if let Some(cmd) = to_execute {
                            if is_running {
                                term.send_stdin(&cmd);
                            } else if let Some(h) = &host_opt {
                                term.execute_remote(h, &cmd);
                            } else {
                                term.execute_local(&cmd, "");
                            }
                        }
                    });
                });
        });
    }
}
