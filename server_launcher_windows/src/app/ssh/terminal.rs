use super::*;

fn advance_cancel_generation(activity: &mut RemoteCommandActivity) {
    activity.cancel_generation = activity.cancel_generation.wrapping_add(1);
}

fn remote_command_was_cancelled(activity: &RemoteCommandActivity, generation: u64) -> bool {
    activity.cancel_generation != generation
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    #[test]
    fn cancellation_is_host_scoped_and_new_jobs_use_the_current_generation() {
        let mut activities = HashMap::from([
            ("first".to_owned(), RemoteCommandActivity { count: 1, ..Default::default() }),
            ("second".to_owned(), RemoteCommandActivity { count: 1, ..Default::default() }),
        ]);
        let first_generation = activities["first"].cancel_generation;
        let second_generation = activities["second"].cancel_generation;
        advance_cancel_generation(activities.get_mut("first").unwrap());
        assert_ne!(activities["first"].cancel_generation, first_generation);
        assert_eq!(activities["second"].cancel_generation, second_generation);
        let new_job_generation = activities["first"].cancel_generation;
        assert!(remote_command_was_cancelled(&activities["first"], first_generation));
        assert!(!remote_command_was_cancelled(&activities["first"], new_job_generation));
        assert!(!remote_command_was_cancelled(&activities["second"], second_generation));
    }
}

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

    pub(in crate::app) fn run_remote_ssh_command(&self, host: &SshRemoteHost, cmd_str: &str, ctx: Option<egui::Context>, refresh_ports: bool) -> bool {
        let logs = self.get_remote_logs(&host.id);
        let generation = {
            const MAX_COMMANDS_PER_HOST: usize = 4;
            let mut counts = self.remote_command_activity.lock().unwrap();
            let count = counts.entry(host.id.clone()).or_default();
            if count.count >= MAX_COMMANDS_PER_HOST {
                crate::service::push_log_line(
                    "[ssh] Four commands are already running on this host. Wait for one to finish.",
                    &logs, 1000, crate::service::LogKind::Warning,
                );
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
                return false;
            }
            count.count += 1;
            count.cancel_generation
        };
        let counts = self.remote_command_activity.clone();
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
            c.stdin(std::process::Stdio::null());
            c.stdout(std::process::Stdio::piped());
            c.stderr(std::process::Stdio::piped());

            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                c.process_group(0);
            }

            let mut exited = false;
            match c.spawn() {
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        crate::service::spawn_stream_reader_with_repaint(stdout, logs_clone.clone(), 1000, crate::service::LogKind::Stdout, ctx_clone.clone());
                    }
                    if let Some(stderr) = child.stderr.take() {
                        crate::service::spawn_stream_reader_with_repaint(stderr, logs_clone.clone(), 1000, crate::service::LogKind::Stderr, ctx_clone.clone());
                    }
                    loop {
                        let message = match child.try_wait() {
                            Ok(Some(status)) => {
                                exited = true;
                                if !status.success() {
                                    crate::service::push_log_line(
                                        &format!("[ssh] Command failed ({status})."),
                                        &logs_clone,
                                        1000,
                                        crate::service::LogKind::Error,
                                    );
                                }
                                break;
                            }
                            Ok(None) => {
                                let reason = {
                                    let activity = counts.lock().unwrap();
                                    activity.get(&host_clone.id).and_then(|state| {
                                        if remote_command_was_cancelled(state, generation) {
                                            Some("[ssh] Canceled command. Closing the local SSH connection.")
                                        } else if state.inactive_since.zip(state.timeout)
                                            .is_some_and(|(since, timeout)| since.elapsed() >= timeout)
                                        {
                                            Some("[ssh] Closing the SSH connection after the configured inactivity timeout.")
                                        } else {
                                            None
                                        }
                                    })
                                };
                                let Some(reason) = reason else {
                                    std::thread::sleep(std::time::Duration::from_millis(250));
                                    continue;
                                };
                                reason.to_owned()
                            }
                            Err(error) => format!("[ssh] Could not check command status: {error}"),
                        };

                        #[cfg(unix)]
                        unsafe {
                            // The unreaped child still owns the group ID.
                            libc::kill(-(child.id() as i32), libc::SIGKILL);
                        }
                        let _ = child.kill();
                        let _ = child.wait();
                        crate::service::push_log_line(&message, &logs_clone, 1000, crate::service::LogKind::Warning);
                        break;
                    }
                }
                Err(e) => {
                    crate::service::push_log_line(
                        &format!("[error] Failed to start SSH process: {}", e),
                        &logs_clone,
                        1000,
                        crate::service::LogKind::Error,
                    );
                }
            }
            {
                let mut counts = counts.lock().unwrap();
                if let Some(count) = counts.get_mut(&host_clone.id) {
                    count.count -= 1;
                    if count.count == 0 {
                        counts.remove(&host_clone.id);
                    }
                }
            }
            if let Some(ctx) = ctx_clone {
                if refresh_ports && exited {
                    request_remote_listener_refresh(&ctx, host_clone.id);
                } else {
                    ctx.request_repaint();
                }
            }
        });
        true
    }

    pub(in crate::app) fn cancel_remote_commands(&self, host_id: &str) {
        if let Some(activity) = self.remote_command_activity.lock().unwrap().get_mut(host_id) {
            advance_cancel_generation(activity);
        }
    }

    pub(in crate::app) fn cancel_all_remote_commands(&self) {
        for activity in self.remote_command_activity.lock().unwrap().values_mut() {
            advance_cancel_generation(activity);
        }
    }

    pub(in crate::app) fn remote_commands_active(&self) -> bool {
        self.remote_command_activity.lock().unwrap().values().any(|activity| activity.count > 0)
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
                    let running = self.remote_command_activity.lock().unwrap().contains_key(&host.id);
                    let tools_width = (ui.available_width() - if running { 164.0 } else { 84.0 }).max(0.0);
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
                                    self.run_remote_ssh_command(host, command, Some(ui.ctx().clone()), false);
                                }
                            }
                        });
                    });
                    if running && ui.add_sized([72.0, 24.0], egui::Button::new(RichText::new("Cancel all").size(11.0))
                        .fill(p.bg_button).stroke(Stroke::NONE).rounding(8.0))
                        .on_hover_text("Close all running command connections for this host").clicked()
                    {
                        self.cancel_remote_commands(&host.id);
                    }
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
                    ScrollArea::vertical().id_salt(("remote_term_output", &host.id)).stick_to_bottom(true)
                        .auto_shrink([false, false]).min_scrolled_height(output_height).max_height(output_height)
                        .show(ui, |ui| {
                            let width = ui.available_width();
                            for entry in &snapshot {
                                let color = match entry.kind {
                                    LogKind::Stdout => p.log_stdout,
                                    LogKind::Stderr | LogKind::Error => p.log_stderr,
                                    LogKind::Stdin => p.text_secondary,
                                    LogKind::Launcher => p.log_launcher,
                                    LogKind::Warning => p.log_warning,
                                };
                                let mut text = egui::text::LayoutJob::single_section(entry.text.to_string(), egui::TextFormat {
                                    font_id: egui::FontId::monospace(11.5), color, line_height: Some(16.0),
                                    ..Default::default()
                                });
                                text.wrap.max_width = width;
                                ui.add(egui::Label::new(text).wrap().selectable(true));
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
                                editor.request_focus();
                            }
                        });
                    });
                if focused {
                    ui.painter().rect_stroke(composer.response.rect, 8.0, Stroke::new(1.0_f32, p.accent));
                }
                if let Some(command) = command {
                    if self.run_remote_ssh_command(host, &command, Some(ui.ctx().clone()), false) {
                        if let Some(input) = self.remote_terminal_inputs.get_mut(&host.id) {
                            input.clear();
                        }
                    }
                }
            });
    }
}
