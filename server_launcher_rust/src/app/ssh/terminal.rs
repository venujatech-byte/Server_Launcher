use super::*;

impl LauncherApp {
    pub(in crate::app) fn get_remote_logs(&self, host_id: &str) -> Arc<Mutex<VecDeque<crate::service::LogEntry>>> {
        let mut map = self.remote_terminal_logs.lock().unwrap();
        if let Some(logs) = map.get(host_id) {
            logs.clone()
        } else {
            let logs = Arc::new(Mutex::new(VecDeque::with_capacity(1000)));
            logs.lock().unwrap().push_back(crate::service::LogEntry::new(
                "[ssh] Terminal initialized. Ready to execute remote commands.".to_string(),
                crate::service::LogKind::Launcher,
            ));
            map.insert(host_id.to_string(), logs.clone());
            logs
        }
    }

    pub(in crate::app) fn run_remote_ssh_command(&self, host: &SshRemoteHost, cmd_str: &str, ctx: Option<egui::Context>) {
        let logs = self.get_remote_logs(&host.id);
        let prompt = format!("{}> {}", host.user, cmd_str);
        crate::service::push_log_line(&prompt, &logs, 1000, crate::service::LogKind::Stdin);
        if let Some(ref c) = ctx {
            c.request_repaint();
        }

        let host_clone = host.clone();
        let cmd_owned = cmd_str.to_string();
        let logs_clone = logs.clone();
        let ctx_clone = ctx.clone();

        std::thread::spawn(move || {
            let mut c = build_ssh_command(&host_clone, &cmd_owned);
            c.stdout(std::process::Stdio::piped());
            c.stderr(std::process::Stdio::piped());

            match c.spawn() {
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        crate::service::spawn_stream_reader(stdout, logs_clone.clone(), 1000, crate::service::LogKind::Stdout);
                    }
                    if let Some(stderr) = child.stderr.take() {
                        crate::service::spawn_stream_reader(stderr, logs_clone.clone(), 1000, crate::service::LogKind::Stderr);
                    }
                    let _ = child.wait();
                    if let Some(c) = ctx_clone {
                        c.request_repaint();
                    }
                }
                Err(e) => {
                    crate::service::push_log_line(
                        &format!("[error] Failed to start SSH process: {}", e),
                        &logs_clone,
                        1000,
                        crate::service::LogKind::Error,
                    );
                    if let Some(c) = ctx_clone {
                        c.request_repaint();
                    }
                }
            }
        });
    }

    pub(in crate::app) fn clear_remote_terminal_logs(&self, host_id: &str) {
        if let Ok(map) = self.remote_terminal_logs.lock() {
            if let Some(logs) = map.get(host_id) {
                if let Ok(mut l) = logs.lock() {
                    l.clear();
                }
            }
        }
    }

    pub(in crate::app) fn render_remote_servers_right_panel(&mut self, ui: &mut Ui, host: &SshRemoteHost) {
        let p = self.palette();
        let logs = self.get_remote_logs(&host.id);
        Frame::none().fill(p.bg_sidebar).rounding(Rounding::same(8.0))
            .inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let tools_width = (ui.available_width() - 84.0).max(0.0);
                    ui.allocate_ui_with_layout([tools_width, 0.0].into(), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(tools_width);
                        ui.label(RichText::new("Remote SSH Terminal").size(12.0).strong().color(p.text_primary));
                        ui.horizontal_wrapped(|ui| {
                            for (command, hint) in [
                                ("ss -tulpn", "Listening Ports"), ("docker ps", "Docker Containers"),
                                ("uptime", "Uptime & Load"), ("free -m", "Memory Usage"), ("df -h", "Disk Usage"),
                            ] {
                                let button = egui::Button::new(RichText::new(command).monospace().size(11.0).color(p.text_secondary))
                                    .fill(p.bg_button).stroke(Stroke::NONE).rounding(8.0);
                                if ui.add(button).on_hover_text(hint).clicked() {
                                    self.run_remote_ssh_command(host, command, Some(ui.ctx().clone()));
                                }
                            }
                        });
                    });
                    if ui.add_sized([68.0, 24.0], egui::Button::new(RichText::new(format!("{} Clear", crate::lucide::TRASH)).size(11.0))
                        .fill(p.bg_button).stroke(Stroke::NONE).rounding(8.0)).on_hover_text("Clear terminal output").clicked() {
                        self.clear_remote_terminal_logs(&host.id);
                    }
                });
                let output_height = (ui.available_height() - 32.0 - ui.spacing().item_spacing.y - 20.0).max(0.0);
                Frame::none().fill(p.terminal_bg).rounding(Rounding::same(8.0)).inner_margin(10.0).show(ui, |ui| {
                    ui.spacing_mut().scroll.bar_width = 6.0;
                    ui.spacing_mut().scroll.floating = false;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let snapshot = logs.lock().unwrap().clone();
                    let rows: Vec<_> = snapshot.iter().flat_map(|entry| {
                        entry.text.split('\n').map(move |line| (&entry.kind, line))
                    }).collect();
                    ScrollArea::both().id_salt(("remote_term_scroll", &host.id)).stick_to_bottom(true)
                        .auto_shrink([false, false]).min_scrolled_height(output_height).max_height(output_height)
                        .show_rows(ui, 16.0, rows.len(), |ui, visible| {
                            for &(kind, line) in &rows[visible] {
                                let color = match kind {
                                    LogKind::Stdout => p.log_stdout,
                                    LogKind::Stderr | LogKind::Error => p.log_stderr,
                                    LogKind::Stdin => p.text_secondary,
                                    LogKind::Launcher => p.log_launcher,
                                    LogKind::Warning => p.log_warning,
                                };
                                let text = egui::text::LayoutJob::single_section(line.to_owned(), egui::TextFormat {
                                    font_id: egui::FontId::monospace(11.5), color, line_height: Some(16.0),
                                    ..Default::default()
                                });
                                ui.add_sized([0.0, 16.0], egui::Label::new(text).extend().selectable(true));
                            }
                        });
                });
                let mut focused = false;
                let mut command = None;
                let composer = Frame::none().fill(p.bg_input).rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let prompt = format!("{}@{}:~$", host.user, host.host);
                            let prompt_width = (ui.available_width() * 0.30).min(180.0);
                            ui.add_sized([prompt_width, 24.0], egui::Label::new(
                                RichText::new(&prompt).monospace().size(12.0).color(p.success)
                            ).truncate()).on_hover_text(&prompt);
                            let input = self.remote_terminal_inputs.entry(host.id.clone()).or_default();
                            let input_width = (ui.available_width() - 68.0 - ui.spacing().item_spacing.x).max(0.0);
                            let editor = ui.add_sized([input_width, 24.0], egui::TextEdit::singleline(input)
                                .frame(false).id_source(("ssh_remote_composer", &host.id))
                                .margin(egui::Margin::ZERO).vertical_align(egui::Align::Center)
                                .font(egui::FontId::monospace(11.5))
                                .hint_text(RichText::new("Enter SSH command…").color(p.text_muted)));
                            let execute = ui.add_sized([68.0, 24.0], egui::Button::new(
                                RichText::new("Execute").size(11.5).strong().color(p.accent_text)
                            ).fill(p.accent).stroke(Stroke::NONE).rounding(8.0));
                            focused = editor.has_focus() || execute.has_focus();
                            let enter = ui.is_enabled() && editor.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if (execute.clicked() || enter) && !input.trim().is_empty() {
                                command = Some(input.trim().to_owned());
                                input.clear();
                                editor.request_focus();
                            }
                        });
                    });
                if focused {
                    ui.painter().rect_stroke(composer.response.rect, 8.0, Stroke::new(1.0_f32, p.accent));
                }
                if let Some(command) = command {
                    self.run_remote_ssh_command(host, &command, Some(ui.ctx().clone()));
                }
            });
    }
}
