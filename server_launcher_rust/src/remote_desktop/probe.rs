use super::{credentials::DesktopProtocol, profile::RdpSecurity};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

type DetectionResult = Result<super::setup::model::ServiceInventoryReport, String>;
type ActionResult = Result<usize, String>;

fn with_partial_progress(
    result: ActionResult,
    completed_steps: &[String],
    total_steps: usize,
) -> ActionResult {
    let Err(error) = result else {
        return result;
    };
    if completed_steps.is_empty() {
        return Err(error);
    }
    let progress = format!(
        "{}/{} approved step(s) completed: {}",
        completed_steps.len(),
        total_steps,
        completed_steps.join(", ")
    );
    if error.starts_with("Cancelled") {
        Err(format!("{error} Progress: {progress}."))
    } else {
        Err(format!("Partial completion ({progress}). {error}"))
    }
}

struct PendingAction {
    host: crate::config::SshRemoteHost,
    key: String,
    protocol: DesktopProtocol,
    plan: super::setup::plan::ActionPlan,
    intent: super::setup::plan::Intent,
    needs_password: bool,
    sudo_available: bool,
    password_attempts: u8,
    distro: Option<String>,
    prior_unit_active: Option<bool>,
    prior_unit_enabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ServerDetection {
    pending: Option<(String, std::sync::mpsc::Receiver<DetectionResult>)>,
    last: Option<(String, DetectionResult, Instant)>,
    history_cache: Option<(
        String,
        Result<Vec<super::setup::history::HistoryRecord>, String>,
    )>,
    action_review: Option<PendingAction>,
    action_password: zeroize::Zeroizing<String>,
    action_pending: Option<std::sync::mpsc::Receiver<super::setup::exec::ActionEvent>>,
    action_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    scan_window_open: bool,
    show_manual_instructions: bool,
    vnc_display: u8,
    vnc_resolution_idx: usize,
    vnc_depth_idx: usize,
    vnc_localhost_disabled: bool,
    vnc_start_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    vnc_start_error: Option<String>,
    vnc_passwd_input: zeroize::Zeroizing<String>,
    vnc_passwd_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    vnc_passwd_status: Option<Result<String, String>>,
    action_status: String,
    action_logs: String,
    action_history_warning: Option<(String, DesktopProtocol, String)>,
    action_result: Option<(String, DesktopProtocol, ActionResult)>,
    action_port_update: Option<(String, String, DesktopProtocol, Instant)>,
}

impl ServerDetection {
    pub(crate) fn action_running(&self) -> bool {
        self.action_pending.is_some()
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        host: &crate::config::SshRemoteHost,
        protocol: DesktopProtocol,
        timeout: Duration,
    ) {
        let key = detection_key(host);
        if !self
            .history_cache
            .as_ref()
            .is_some_and(|(cached_key, _)| cached_key == &key)
        {
            self.history_cache = Some((
                key.clone(),
                super::setup::history::recent_for_host(host),
            ));
        }
        let events = self
            .action_pending
            .as_ref()
            .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                super::setup::exec::ActionEvent::StepStarted { index, total, title } => {
                    self.action_status = format!("Step {index}/{total}: {title}");
                }
                super::setup::exec::ActionEvent::StepFinished { index, total, title, output } => {
                    self.action_status = format!("Completed {index}/{total}: {title}");
                    if !output.trim().is_empty() {
                        self.action_logs.push_str(&format!("\n{title}\n{output}"));
                        let excess = self.action_logs.len().saturating_sub(24 * 1024);
                        if excess > 0 {
                            let boundary = self.action_logs.char_indices().find(|(i, _)| *i >= excess).map_or(self.action_logs.len(), |(i, _)| i);
                            self.action_logs.drain(..boundary);
                        }
                    }
                }
                super::setup::exec::ActionEvent::HistoryWarning(warning) => {
                    if let Some(action) = &self.action_review {
                        self.action_history_warning = Some((
                            action.key.clone(),
                            action.protocol,
                            warning,
                        ));
                    }
                }
                super::setup::exec::ActionEvent::Finished {
                    result,
                    completed_step_titles,
                    total_steps,
                } => {
                    self.action_pending = None;
                    self.action_cancel = None;
                    self.history_cache = None;
                    let rejected = matches!(&result, Err(error) if error.starts_with("Sudo password was rejected."));
                    let retry = if rejected {
                        self.action_review.as_mut().is_some_and(|action| {
                            action.password_attempts += 1;
                            action.password_attempts < 3
                        })
                    } else {
                        false
                    };
                    if retry {
                        if let Some(action) = &self.action_review {
                            self.action_status = format!(
                                "Sudo password was rejected ({}/3). Enter it again.",
                                action.password_attempts
                            );
                        }
                    } else if let Some(action) = self.action_review.take() {
                        let result = if rejected {
                            Err("Three sudo password attempts failed. Use the manual instructions or verify the account's sudo access.".into())
                        } else {
                            with_partial_progress(result, &completed_step_titles, total_steps)
                        };
                        if result.is_ok()
                            && matches!(
                                action.intent,
                                super::setup::plan::Intent::Start
                                    | super::setup::plan::Intent::Restart
                                    | super::setup::plan::Intent::InstallAndStart
                                    | super::setup::plan::Intent::Update
                            )
                        {
                            self.action_port_update = Some((
                                action.host.id.clone(),
                                action.key.clone(),
                                action.protocol,
                                Instant::now(),
                            ));
                        }
                        // For destructive actions (Uninstall) or successful installs, clear the
                        // stale cached result immediately so the UI shows the scan placeholder
                        // instead of the old state while the post-action rescan runs.
                        if matches!(
                            action.intent,
                            super::setup::plan::Intent::Uninstall
                                | super::setup::plan::Intent::InstallAndStart
                        ) {
                            self.last = None;
                        }
                        self.action_result = Some((action.key.clone(), action.protocol, result));
                        self.start(action.host, action.key, timeout, ui.ctx().clone());
                    }
                    self.action_password.clear();
                }
            }
        }
        if let Some((_, receiver)) = &self.pending {
            use std::sync::mpsc::TryRecvError;
            match receiver.try_recv() {
                Ok(result) => {
                    self.last = self
                        .pending
                        .take()
                        .map(|(key, _)| (key, result, Instant::now()));
                }
                Err(TryRecvError::Disconnected) => {
                    self.last = self
                        .pending
                        .take()
                        .map(|(key, _)| {
                            (
                                key,
                                Err("The server scan stopped unexpectedly.".into()),
                                Instant::now(),
                            )
                        });
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let can_scan = !host.host.trim().is_empty() && !host.user.trim().is_empty();
        let has_current_result = self
            .last
            .as_ref()
            .is_some_and(|(tested, _, _)| tested == &key);

        ui.horizontal(|ui| {
            ui.strong(match protocol {
                DesktopProtocol::Rdp => "xrdp service",
                DesktopProtocol::Vnc => "TigerVNC service",
            });
            if self.pending.is_some() {
                ui.spinner();
                ui.label("Checking over SSH…");
            }
            if ui
                .add_enabled(
                    can_scan && self.pending.is_none(),
                    egui::Button::new(if has_current_result { "Scan again" } else { "Scan" }),
                )
                .clicked()
            {
                self.scan_window_open = true;
                self.show_manual_instructions = false;
                self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
            }
            if has_current_result && !self.scan_window_open && ui.button("Open status").clicked() {
                self.scan_window_open = true;
            }
        });

        let mut requested_intent = None;
        let mut requested_undo = None;
        let mut request_manual_scan = false;
        let mut open = self.scan_window_open;
        if open {
            let window_title = match protocol {
                DesktopProtocol::Rdp => "Service Manager — xrdp",
                DesktopProtocol::Vnc => "Service Manager — TigerVNC",
            };
            egui::Window::new(window_title)
                .id(egui::Id::new(("service_manager_window", protocol as u8)))
                .title_bar(false)
                .resizable(true)
                .default_width(420.0)
                .frame(
                    egui::Frame::window(&ui.ctx().style())
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(75)))
                        .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 18.0,
                        spread: 2.0,
                        color: egui::Color32::from_black_alpha(140),
                    }),
                )
                .show(ui.ctx(), |ui| {
                    ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
                    ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(11.5));
                    ui.style_mut().text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(10.5));
                    ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.0));
                    // — compact custom title bar —
                    ui.horizontal(|ui| {
                        ui.add_space(2.0);
                        // Protocol pill icon
                        let proto_label = match protocol {
                            DesktopProtocol::Rdp => "RDP",
                            DesktopProtocol::Vnc => "VNC",
                        };
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgba_unmultiplied(80, 120, 200, 60))
                            .rounding(egui::Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(proto_label).size(10.0).strong().color(egui::Color32::from_rgb(140, 180, 255)));
                            });
                        ui.label(egui::RichText::new(window_title).size(12.0).strong());
                        if self.pending.is_some() {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close button styled as a round pill
                            let close_resp = ui.add(
                                egui::Button::new(egui::RichText::new("✕").size(10.0))
                                    .min_size(egui::vec2(18.0, 18.0))
                                    .rounding(egui::Rounding::same(999.0)),
                            ).on_hover_text("Close");
                            if close_resp.clicked() {
                                open = false;
                            }
                            if ui
                                .add_enabled(
                                    can_scan && self.pending.is_none(),
                                    egui::Button::new(egui::RichText::new("↺").size(12.0))
                                        .min_size(egui::vec2(18.0, 18.0))
                                        .rounding(egui::Rounding::same(999.0)),
                                )
                                .on_hover_text("Scan again")
                                .clicked()
                            {
                                self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
                            }
                        });
                    });
                    ui.separator();
                    ui.add_space(2.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("service_manager_scroll", protocol as u8))
                        .max_height(ui.ctx().screen_rect().height() * 0.70)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                    if self.pending.is_some() && !has_current_result {
                        show_service_scan_placeholder(ui, protocol);
                    }
                    let mut show_vnc_controls = None;
                    if let Some((_, result, _)) = self.last.as_ref().filter(|(tested, _, _)| tested == &key) {
                        match result {
                            Ok(report) => {
                                let port = match protocol {
                                    DesktopProtocol::Rdp => host.rdp.as_ref().map_or(3389, |profile| profile.port),
                                    DesktopProtocol::Vnc => host.vnc.as_ref().map_or(5900, |profile| profile.port),
                    };
                    requested_intent = show_service_inventory(
                        ui,
                        report,
                        protocol,
                        port,
                        &mut request_manual_scan,
                    );
                    if protocol == DesktopProtocol::Vnc {
                        match report.vnc.state() {
                            super::setup::model::ServiceState::InstalledStopped
                            | super::setup::model::ServiceState::Failed => {
                                show_vnc_controls = Some(false);
                            }
                            super::setup::model::ServiceState::RunningManual
                            | super::setup::model::ServiceState::RunningSystemd => {
                                show_vnc_controls = Some(true);
                            }
                            _ => {}
                        }
                    }
                    let history = self
                        .history_cache
                        .as_ref()
                        .filter(|(history_key, _)| history_key == &key)
                        .map(|(_, result)| result);
                    requested_undo = show_service_history(
                        ui,
                        host,
                        protocol,
                        report,
                        history,
                        self.action_review.is_none() && self.action_pending.is_none(),
                    );
                }
                Err(error) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        scan_error_hint(error).unwrap_or("Remote service scan failed."),
                    );
                    egui::CollapsingHeader::new("Scan error details")
                        .id_salt(("service_scan_error", protocol as u8))
                        .show(ui, |ui| {
                            let detail = error.chars().take(4096).collect::<String>();
                            ui.add(egui::Label::new(detail).wrap());
                        });
                }
            }
        }
                    if let Some(is_running) = show_vnc_controls {
                        self.show_vnc_start_config(ui, host, &key, timeout, is_running);
                    }
                        }); // end ScrollArea
                });
            self.scan_window_open = open;
        } else if !can_scan {
            ui.label("Enter the SSH host and username to scan.");
        }
        if request_manual_scan && self.pending.is_none() {
            self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
        }

        if let Some(intent) = requested_intent {
            if let Some((_, Ok(report), _)) = self.last.as_ref().filter(|(tested, _, _)| tested == &key) {
                if let Ok(plan) = super::setup::plan::build_plan(report, protocol, intent) {
                    self.action_password.clear();
                    self.action_review = Some(PendingAction {
                        host: host.clone(),
                        key: key.clone(),
                        protocol,
                        plan,
                        intent,
                        needs_password: report.uid != Some(0) && report.passwordless_sudo != Some(true),
                        sudo_available: report.uid == Some(0) || report.sudo_available != Some(false),
                        password_attempts: 0,
                        distro: report
                            .os_pretty_name
                            .clone()
                            .or_else(|| report.os_id.clone()),
                        prior_unit_active: report.service(protocol).unit_active,
                        prior_unit_enabled: report.service(protocol).unit_enabled,
                    });
                }
            }
        }

        if let Some(record) = requested_undo {
            if protocol == DesktopProtocol::Rdp {
                if let Some((_, Ok(report), _)) =
                    self.last.as_ref().filter(|(tested, _, _)| tested == &key)
                {
                    let service = report.service(protocol);
                    if service.unit_present == Some(true) {
                        if let Ok(plan) = super::setup::plan::build_restore_plan(
                            record.prior_unit_active,
                            record.prior_unit_enabled,
                            service.unit_active,
                            service.unit_enabled,
                        ) {
                            self.action_password.clear();
                            self.action_review = Some(PendingAction {
                                host: host.clone(),
                                key: key.clone(),
                                protocol,
                                plan,
                                intent: super::setup::plan::Intent::RestoreState,
                                needs_password: report.uid != Some(0)
                                    && report.passwordless_sudo != Some(true),
                                sudo_available: report.uid == Some(0)
                                    || report.sudo_available != Some(false),
                                password_attempts: 0,
                                distro: report
                                    .os_pretty_name
                                    .clone()
                                    .or_else(|| report.os_id.clone()),
                                prior_unit_active: service.unit_active,
                                prior_unit_enabled: service.unit_enabled,
                            });
                        }
                    }
                }
            }
        }

        if let Some((result_key, result_protocol, result)) = &self.action_result {
            if result_key == &key && *result_protocol == protocol {
                match result {
                    Ok(count) => {
                        ui.colored_label(ui.visuals().hyperlink_color, format!("Action completed ({count} step(s)); checking the service again…"));
                    }
                    Err(error) => {
                        let cancelled = error.starts_with("Cancelled");
                        if cancelled {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                format!("Remote action cancelled: {error}"),
                            );
                        } else {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!(
                                    "Remote action failed. {}",
                                    action_error_hint(error).unwrap_or(
                                        "Review the remote output for the cause and next step."
                                    )
                                ),
                            );
                            egui::CollapsingHeader::new("Remote error details")
                                .id_salt(("service_action_error", protocol as u8))
                                .show(ui, |ui| {
                                    let detail = error.chars().take(4096).collect::<String>();
                                    ui.add(egui::Label::new(detail).wrap());
                                });
                        }
                    }
                }
            }
        }
        if let Some((warning_key, warning_protocol, warning)) = &self.action_history_warning {
            if warning_key == &key && *warning_protocol == protocol {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Action ran, but local change history was not saved: {warning}"),
                );
            }
        }
        self.show_action_review(ui, &key);
    }

    fn show_vnc_start_config(
        &mut self,
        ui: &mut egui::Ui,
        host: &crate::config::SshRemoteHost,
        key: &str,
        timeout: Duration,
        is_running: bool,
    ) {
        if let Some(rx) = &self.vnc_passwd_pending {
            if let Ok(result) = rx.try_recv() {
                self.vnc_passwd_pending = None;
                self.vnc_passwd_status = Some(result);
            }
        }
        if let Some(rx) = &self.vnc_start_pending {
            if let Ok(result) = rx.try_recv() {
                self.vnc_start_pending = None;
                match result {
                    Ok(_) => {
                        self.vnc_start_error = None;
                        self.last = None;
                        self.start(host.clone(), key.to_owned(), timeout, ui.ctx().clone());
                    }
                    Err(err) => {
                        self.vnc_start_error = Some(err);
                    }
                }
            }
        }

        const RESOLUTIONS: &[&str] = &["1920x1080", "1600x900", "1366x768", "1280x720", "2560x1440"];
        const DEPTHS: &[&str] = &["24", "16", "32"];
        if self.vnc_display == 0 {
            self.vnc_display = 1;
        }
        let res = RESOLUTIONS
            .get(self.vnc_resolution_idx)
            .copied()
            .unwrap_or("1920x1080");
        let depth = DEPTHS.get(self.vnc_depth_idx).copied().unwrap_or("24");
        let localhost_arg = if self.vnc_localhost_disabled { "no" } else { "yes" };
        let cmd = format!(
            "vncserver :{} -localhost {} -geometry {} -depth {}",
            self.vnc_display, localhost_arg, res, depth
        );

        ui.add_space(6.0);
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(28, 31, 42))
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::same(10.0))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Start a session").strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Display:");
                    ui.add(egui::DragValue::new(&mut self.vnc_display).range(1..=9).prefix(":"));
                    ui.label("Resolution:");
                    egui::ComboBox::from_id_salt("vnc_start_res")
                        .selected_text(res)
                        .show_ui(ui, |ui| {
                            for (i, r) in RESOLUTIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.vnc_resolution_idx, i, *r);
                            }
                        });
                    ui.label("Depth:");
                    egui::ComboBox::from_id_salt("vnc_start_depth")
                        .selected_text(depth)
                        .show_ui(ui, |ui| {
                            for (i, d) in DEPTHS.iter().enumerate() {
                                ui.selectable_value(&mut self.vnc_depth_idx, i, *d);
                            }
                        });
                });
                ui.checkbox(&mut self.vnc_localhost_disabled, "Allow connections from other machines (-localhost no)");
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 20, 28))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("$ {cmd}")).monospace().small().weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(cmd.clone());
                                }
                            });
                        });
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.vnc_start_pending.is_some();
                    let mut run_cmd: Option<String> = None;
                    if is_running {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Stop VNC"))
                            .clicked()
                        {
                            run_cmd = Some(format!("vncserver -kill :{}", self.vnc_display));
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Restart VNC"))
                            .clicked()
                        {
                            run_cmd = Some(format!(
                                "vncserver -kill :{} || true; {}",
                                self.vnc_display, cmd
                            ));
                        }
                    } else if ui
                        .add_enabled(!busy, egui::Button::new("▶ Start VNC"))
                        .clicked()
                    {
                        run_cmd = Some(cmd.clone());
                    }
                    if let Some(cmd_owned) = run_cmd {
                        self.vnc_start_error = None;
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.vnc_start_pending = Some(rx);
                        let host_owned = host.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let command = crate::remote::build_ssh_command(
                                &host_owned,
                                &cmd_owned,
                            );
                            let res = crate::remote::ssh_probe_output(command, timeout)
                                .and_then(|out| {
                                    if out.status.success() {
                                        Ok(String::new())
                                    } else {
                                        Err(std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                                        ))
                                    }
                                })
                                .map_err(|e| e.to_string());
                            let _ = tx.send(res);
                            ctx.request_repaint();
                        });
                    }
                    if busy {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(if is_running {
                                "Updating VNC..."
                            } else {
                                "Starting VNC..."
                            })
                            .small()
                            .weak(),
                        );
                    }
                });
                if let Some(err) = &self.vnc_start_error {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("VNC password:");
                    ui.add(
                        egui::TextEdit::singleline(&mut *self.vnc_passwd_input)
                            .password(true)
                            .desired_width(140.0)
                            .hint_text("6-8+ chars"),
                    );
                    let pw_busy = self.vnc_passwd_pending.is_some();
                    let can_set = !pw_busy && !self.vnc_passwd_input.is_empty();
                    if ui
                        .add_enabled(can_set, egui::Button::new("Set password"))
                        .clicked()
                    {
                        self.vnc_passwd_status = None;
                        let pw = zeroize::Zeroizing::new(format!(
                            "{}\n",
                            self.vnc_passwd_input.as_str()
                        ));
                        self.vnc_passwd_input.clear();
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.vnc_passwd_pending = Some(rx);
                        let host_owned = host.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let command = crate::remote::build_ssh_command(
                                &host_owned,
                                "mkdir -p ~/.vnc && vncpasswd -f > ~/.vnc/passwd && chmod 600 ~/.vnc/passwd",
                            );
                            let res = crate::remote::ssh_probe_output_with_stdin(
                                command,
                                timeout,
                                pw.as_bytes(),
                            )
                            .and_then(|out| {
                                if out.status.success() {
                                    Ok("VNC password saved on server.".to_owned())
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                                    ))
                                }
                            })
                            .map_err(|e| e.to_string());
                            let _ = tx.send(res);
                            ctx.request_repaint();
                        });
                    }
                    if pw_busy {
                        ui.spinner();
                    }
                });
                if let Some(status) = &self.vnc_passwd_status {
                    match status {
                        Ok(msg) => {
                            ui.colored_label(egui::Color32::from_rgb(120, 210, 140), msg);
                        }
                        Err(err) => {
                            ui.colored_label(ui.visuals().error_fg_color, err);
                        }
                    }
                }
            });
    }

    fn show_action_review(&mut self, ui: &mut egui::Ui, key: &str) {
        let Some(action) = self.action_review.as_ref().filter(|action| action.key == key) else {
            return;
        };
        let running = self.action_pending.is_some();
        let mut approve = false;
        let mut cancel = false;
        let mut cancel_after_step = false;
        let mut window_open = true;
        let mut window = egui::Window::new("Review remote service action")
            .id(egui::Id::new(("service_action_review", key)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .frame(
                egui::Frame::window(&ui.ctx().style())
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(75)))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 6.0),
                        blur: 18.0,
                        spread: 2.0,
                        color: egui::Color32::from_black_alpha(140),
                    }),
            );
        if !running {
            window = window.open(&mut window_open);
        }
        window.show(ui.ctx(), |ui| {
                ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
                ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(11.5));
                ui.style_mut().text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(10.5));
                ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.0));
                // — compact custom title bar —
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Service action review").size(12.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !running {
                            let close_btn = ui.add(
                                egui::Button::new(egui::RichText::new("✕").size(10.0))
                                    .min_size(egui::vec2(18.0, 18.0))
                                    .rounding(egui::Rounding::same(999.0)),
                            ).on_hover_text("Cancel");
                            if close_btn.clicked() {
                                cancel = true;
                            }
                        }
                    });
                });
                ui.separator();
                ui.add_space(4.0);

                // — action title + subtitle —
                ui.label(egui::RichText::new(action.plan.summary).size(15.0).strong());
                ui.label(
                    egui::RichText::new("Review the commands below. Nothing runs until you click Run.")
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (idx, step) in action.plan.steps.iter().enumerate() {
                        egui::Frame::group(ui.style())
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{}", idx + 1))
                                            .small()
                                            .weak()
                                            .monospace(),
                                    );
                                    ui.label(egui::RichText::new(step.title).strong().size(13.0));
                                });
                                ui.add_space(2.0);
                                ui.label(egui::RichText::new(step.effect).size(12.0));
                                ui.label(
                                    egui::RichText::new(format!("↩ {}", step.undo_note))
                                        .small()
                                        .weak(),
                                );
                                ui.add_space(5.0);
                                let command = super::setup::exec::command_for_step(
                                    step,
                                    action.host.user == "root",
                                    action.needs_password,
                                )
                                .unwrap_or_else(|_| "Unsupported plan command".into());
                                egui::Frame::none()
                                    .fill(ui.visuals().extreme_bg_color)
                                    .rounding(egui::Rounding::same(4.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(command).monospace().size(11.0),
                                            )
                                            .wrap(),
                                        );
                                    });
                            });
                        if idx + 1 < action.plan.steps.len() {
                            ui.add_space(5.0);
                        }
                    }
                });

                if !action.plan.warnings.is_empty() {
                    ui.add_space(6.0);
                    for warning in &action.plan.warnings {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("⚠").color(ui.visuals().warn_fg_color));
                            ui.label(egui::RichText::new(*warning).small().color(ui.visuals().warn_fg_color));
                        });
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // — sudo / password section —
                if !action.sudo_available {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "sudo is not available for this SSH user. Use the manual instructions.",
                    );
                } else if action.needs_password {
                    ui.label(
                        egui::RichText::new(format!(
                            "🔒 Administrator password — attempt {}/3 (used only for this action, never saved)",
                            action.password_attempts + 1
                        ))
                        .size(12.0),
                    );
                    ui.add_space(3.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut *self.action_password)
                            .password(true)
                            .hint_text("Enter sudo password…")
                            .desired_width(f32::INFINITY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("✓ No administrator password required for this action.")
                            .small()
                            .weak(),
                    );
                }

                if self.action_status.starts_with("Sudo password was rejected") {
                    ui.add_space(2.0);
                    ui.colored_label(ui.visuals().error_fg_color, &self.action_status);
                }

                ui.add_space(8.0);

                // — footer buttons —
                ui.horizontal(|ui| {
                    if running {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(if self.action_status.is_empty() {
                                "Running approved steps…"
                            } else {
                                self.action_status.as_str()
                            })
                            .small(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Cancel after this step").clicked() {
                                cancel_after_step = true;
                            }
                        });
                    } else {
                        let can_run = action.sudo_available
                            && (!action.needs_password || !self.action_password.is_empty());
                        if ui.add_enabled(can_run, egui::Button::new("▶  Run")).clicked() {
                            approve = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    }
                });

                if running && !self.action_logs.is_empty() {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&self.action_logs).monospace().size(11.0)).wrap());
                    });
                }
            });
        if !window_open {
            self.action_review = None;
        }
        if cancel_after_step {
            if let Some(cancel) = &self.action_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                self.action_status = "Cancellation requested; the current step will finish first.".into();
            }
        }
        if cancel {
            self.action_password.clear();
            self.action_review = None;
        }
        if approve {
            if let Some(action) = self.action_review.as_ref() {
                let host = action.host.clone();
                let plan = action.plan.clone();
                let protocol = action.protocol;
                let distro = action.distro.clone();
                let prior_unit_active = action.prior_unit_active;
                let prior_unit_enabled = action.prior_unit_enabled;
                let password = if action.needs_password {
                    Some(std::mem::take(&mut self.action_password))
                } else {
                    None
                };
                let (sender, receiver) = std::sync::mpsc::channel();
                self.action_pending = Some(receiver);
                self.action_status.clear();
                self.action_logs.clear();
                self.action_history_warning = None;
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.action_cancel = Some(cancel.clone());
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let mut completed_steps = Vec::new();
                    let mut completed_step_titles = Vec::new();
                    let result = super::setup::exec::run_approved_plan(
                        &host,
                        &plan,
                        password,
                        &cancel,
                        |event| {
                            if let super::setup::exec::ActionEvent::StepFinished { index, .. } = &event {
                                if let Some(step) = plan.steps.get(index.saturating_sub(1)) {
                                    completed_steps.push(step.id.to_owned());
                                    completed_step_titles.push(step.title.to_owned());
                                }
                            }
                            let _ = sender.send(event);
                            ctx.request_repaint();
                        },
                    );
                    if !completed_steps.is_empty() {
                        let action_completed = result.is_ok() || completed_steps.len() == plan.steps.len();
                        if let Err(warning) = super::setup::history::record_action(
                            &host,
                            protocol.label(),
                            plan.summary,
                            distro.as_deref(),
                            prior_unit_active,
                            prior_unit_enabled,
                            completed_steps,
                            action_completed,
                        ) {
                            let _ = sender.send(
                                super::setup::exec::ActionEvent::HistoryWarning(warning),
                            );
                        }
                    }
                    let _ = sender.send(super::setup::exec::ActionEvent::Finished {
                        result,
                        completed_step_titles,
                        total_steps: plan.steps.len(),
                    });
                    ctx.request_repaint();
                });
            }
        }
    }

    pub(crate) fn indicator(
        &self,
        host: &crate::config::SshRemoteHost,
        protocol: DesktopProtocol,
    ) -> Option<&'static str> {
        let key = detection_key(host);
        let (_, result, _) = self.last.as_ref().filter(|(tested, _, scanned)| {
            tested == &key && scanned.elapsed() < Duration::from_secs(60)
        })?;
        let report = result.as_ref().ok()?;
        if let Some(handshake) = report.handshake(protocol) {
            return Some(match (handshake, report.service(protocol).state()) {
                (
                    Ok(_),
                    super::setup::model::ServiceState::RunningSystemd
                    | super::setup::model::ServiceState::RunningManual,
                ) => "●",
                _ => "◐",
            });
        }
        Some(match report.service(protocol).state() {
            super::setup::model::ServiceState::NotInstalled => "○",
            super::setup::model::ServiceState::InstalledStopped
            | super::setup::model::ServiceState::RunningSystemd
            | super::setup::model::ServiceState::RunningManual => "◐",
            super::setup::model::ServiceState::Failed => "!",
            super::setup::model::ServiceState::Unknown => "?",
        })
    }

    pub(crate) fn suggested_port(
        &self,
        host: &crate::config::SshRemoteHost,
        protocol: DesktopProtocol,
        configured_port: u16,
    ) -> Option<u16> {
        let key = detection_key(host);
        let (_, result, _) = self.last.as_ref().filter(|(tested, _, scanned)| {
            tested == &key && scanned.elapsed() < Duration::from_secs(60)
        })?;
        find_port_suggestion(
            result.as_ref().ok()?.service(protocol),
            protocol,
            configured_port,
        )
    }

    pub(crate) fn take_action_port_update(
        &mut self,
        host: &crate::config::SshRemoteHost,
        protocol: DesktopProtocol,
    ) -> Option<u16> {
        let (action_host_id, key, action_protocol, started) =
            self.action_port_update.as_ref()?;
        if action_host_id != &host.id || *action_protocol != protocol {
            return None;
        }
        if key != &detection_key(host) {
            self.action_port_update = None;
            return None;
        }
        if self.pending.is_some() {
            return None;
        }
        let auto_assign_allowed = match protocol {
            DesktopProtocol::Rdp => host.rdp.as_ref().map_or(true, |profile| {
                profile.port_auto || profile.port_manual == Some(false)
            }),
            DesktopProtocol::Vnc => host.vnc.as_ref().map_or(true, |profile| {
                profile.port_auto || profile.port_manual == Some(false)
            }),
        };
        let report = self
            .last
            .as_ref()
            .filter(|(tested, _, scanned)| tested == key && scanned >= started)
            .and_then(|(_, result, _)| result.as_ref().ok());
        let port = report.and_then(|report| {
            let configured = match protocol {
                DesktopProtocol::Rdp => host.rdp.as_ref().map_or(3389, |profile| profile.port),
                DesktopProtocol::Vnc => host.vnc.as_ref().map_or(5900, |profile| profile.port),
            };
            find_port_suggestion(report.service(protocol), protocol, configured)
        });
        self.action_port_update = None;
        if auto_assign_allowed {
            port
        } else {
            None
        }
    }

    pub(crate) fn vnc_displays(
        &self,
        host: &crate::config::SshRemoteHost,
    ) -> Vec<super::setup::model::VncDisplay> {
        let key = detection_key(host);
        self.last
            .as_ref()
            .filter(|(tested, _, scanned)| {
                tested == &key && scanned.elapsed() < Duration::from_secs(60)
            })
            .and_then(|(_, result, _)| result.as_ref().ok())
            .map_or_else(Vec::new, |report| report.vnc.displays.clone())
    }

    fn start(
        &mut self,
        host: crate::config::SshRemoteHost,
        key: String,
        timeout: Duration,
        ctx: egui::Context,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.pending = Some((key, receiver));
        std::thread::spawn(move || {
            let _ = sender.send(detect_servers(&host, timeout));
            ctx.request_repaint();
        });
    }
}

fn action_error_hint(error: &str) -> Option<&'static str> {
    let message = error.chars().take(8192).collect::<String>().to_ascii_lowercase();
    if message.contains("could not get lock")
        || message.contains("unable to acquire")
        || message.contains("lock-frontend")
    {
        Some("The package manager is busy. Let the current update finish, then retry.")
    } else if message.contains("unable to locate package")
        || message.contains("has no installation candidate")
    {
        Some("The package is unavailable from the configured repositories. Check the distribution and repository setup.")
    } else if message.contains("not in the sudoers")
        || message.contains("not allowed to execute")
        || message.contains("sudo is not available")
    {
        Some("This SSH account does not have permission to run the approved administrator command.")
    } else if message.contains("no space left on device") {
        Some("The remote system is out of disk space. Free space before retrying.")
    } else if message.contains("address already in use") || message.contains("port is already allocated") {
        Some("The requested port is already in use. Rescan the host and choose an available port.")
    } else if message.contains("selinux") && (message.contains("denied") || message.contains("avc:")) {
        Some("SELinux blocked the service operation. Review the audit log and use the distribution's documented policy; do not disable SELinux.")
    } else if message.contains("no desktop environment")
        || message.contains("unable to start the session")
        || message.contains("failed to start the desktop")
    {
        Some("The remote desktop service needs an installed desktop session. Check the manual instructions before adding one.")
    } else if message.contains("temporary failure resolving")
        || message.contains("could not resolve")
        || message.contains("network is unreachable")
        || message.contains("failed to fetch")
    {
        Some("The remote system could not reach its package source. Check its network and repository configuration.")
    } else if message.contains("unit ") && message.contains("not found") {
        Some("The expected systemd unit is missing. Rescan the host and use the manual instructions if it remains absent.")
    } else if message.contains("timed out") || message.contains("deadline") {
        Some("The remote command timed out. Check the server and retry when it is responsive.")
    } else {
        None
    }
}

fn scan_error_hint(error: &str) -> Option<&'static str> {
    let message = error.chars().take(8192).collect::<String>().to_ascii_lowercase();
    if message.contains("permission denied") || message.contains("authentication failed") {
        Some("SSH authentication failed. Check the username, password, key path, and key permissions.")
    } else if message.contains("could not resolve hostname")
        || message.contains("temporary failure in name resolution")
        || message.contains("name or service not known")
    {
        Some("The SSH host name could not be resolved. Check the host address and DNS settings.")
    } else if message.contains("connection refused") {
        Some("The SSH server refused the connection. Check the SSH port and server status.")
    } else if message.contains("timed out") || message.contains("operation timeout") {
        Some("The SSH connection timed out. Check the server and network; a firewall or security-group rule may be blocking it.")
    } else if message.contains("no route to host") || message.contains("network is unreachable") {
        Some("The SSH host is unreachable from this computer. Check its address and network route.")
    } else {
        None
    }
}

fn probe_error_summary(error: &str) -> &'static str {
    let message = error.chars().take(8192).collect::<String>().to_ascii_lowercase();
    if message.contains("connection refused") {
        "No service accepted the connection. Check the remote service and port."
    } else if message.contains("timed out") || message.contains("timeout") {
        "The connection timed out. Check the server and network; a firewall or security-group rule may be blocking it."
    } else if message.contains("dns:") {
        "The host name could not be resolved. Check the remote address and DNS settings."
    } else if message.contains("ssh tunnel:") {
        "The SSH tunnel could not reach the remote desktop service. Check SSH access, the detected listener, and the port."
    } else if message.contains("handshake:")
        && (message.contains("unsupported") || message.contains("security type"))
    {
        "The server offered a protocol or security type this client does not support. Check the server's RDP/VNC security settings."
    } else if message.contains("handshake:") {
        "The port answered, but the protocol handshake failed. Confirm that the selected protocol and port match the server."
    } else {
        "The connection check failed. Expand the details to review the reported reason."
    }
}

fn detection_key(host: &crate::config::SshRemoteHost) -> String {
    use sha2::{Digest, Sha256};

    let password_fingerprint = format!("{:x}", Sha256::digest(host.password.as_bytes()));
    let rdp = host.rdp.as_ref();
    let vnc = host.vnc.as_ref();
    let rdp_security = rdp.map_or_else(|| "none".into(), |profile| format!("{:?}", profile.security));
    format!(
        "{}:{}@{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        host.id,
        host.port,
        host.host,
        host.user,
        host.key_path,
        host.credential_ref.as_deref().unwrap_or_default(),
        host.use_agent,
        host.strict_host_key,
        host.remote_cwd,
        password_fingerprint,
        rdp.is_some_and(|profile| profile.enabled),
        rdp.map_or(0, |profile| profile.port),
        rdp_security,
        rdp.is_some_and(|profile| profile.tunnel_via_ssh),
        rdp.map_or("", |profile| profile.gateway.as_str()),
        vnc.is_some_and(|profile| profile.enabled),
        vnc.map_or(0, |profile| profile.port),
        vnc.is_some_and(|profile| profile.tunnel_via_ssh)
    )
}

fn detect_servers(
    host: &crate::config::SshRemoteHost,
    timeout: Duration,
) -> DetectionResult {
    let mut report = super::setup::detect::scan(host, timeout)?;
    if let Some(profile) = host.rdp.as_ref().filter(|profile| profile.enabled) {
        if let Some(port) = detected_port(&report.rdp, DesktopProtocol::Rdp, profile.port) {
            let mut ssh_host = host.clone();
            ssh_host.password.clear();
            let request = ProbeRequest {
                host: host.host.trim().trim_matches(['[', ']']).to_owned(),
                port,
                protocol: DesktopProtocol::Rdp,
                rdp_security: profile.security,
                ssh_host: Some(ssh_host),
                tunnel_via_ssh: profile.tunnel_via_ssh,
                gateway: (!profile.gateway.trim().is_empty()).then(|| profile.gateway.clone()),
                config_path: None,
            };
            report.rdp_handshake = Some(run_with_target(
                &request,
                tunnel_target(&report.rdp.listeners, port),
                timeout,
            ));
        }
    }
    if let Some(profile) = host.vnc.as_ref().filter(|profile| profile.enabled) {
        if let Some(port) = detected_port(&report.vnc, DesktopProtocol::Vnc, profile.port) {
            let mut ssh_host = host.clone();
            ssh_host.password.clear();
            let request = ProbeRequest {
                host: host.host.trim().trim_matches(['[', ']']).to_owned(),
                port,
                protocol: DesktopProtocol::Vnc,
                rdp_security: RdpSecurity::default(),
                ssh_host: Some(ssh_host),
                tunnel_via_ssh: profile.tunnel_via_ssh,
                gateway: None,
                config_path: None,
            };
            report.vnc_handshake = Some(run_with_target(
                &request,
                tunnel_target(&report.vnc.listeners, port),
                timeout,
            ));
        }
    }
    Ok(report)
}

fn detected_port(
    service: &super::setup::model::ServiceInventory,
    protocol: DesktopProtocol,
    configured_port: u16,
) -> Option<u16> {
    let detected_display = (protocol == DesktopProtocol::Vnc)
        .then(|| {
            service
                .displays
                .iter()
                .find(|display| display.port == configured_port)
                .or_else(|| service.displays.iter().min_by_key(|display| display.port))
                .map(|display| display.port)
        })
        .flatten();
    detected_display.or_else(|| {
        service
        .listeners
        .iter()
        .find(|listener| listener.port == configured_port)
        .or_else(|| service.listeners.iter().min_by_key(|listener| listener.port))
        .map(|listener| listener.port)
    })
}

fn tunnel_target(
    listeners: &[super::setup::model::Listener],
    port: u16,
) -> Option<std::net::IpAddr> {
    use std::net::{IpAddr, Ipv4Addr};

    let mut specific = None;
    for listener in listeners.iter().filter(|listener| listener.port == port) {
        let address = listener.address.trim_matches(['[', ']']);
        if address == "*" || address == "localhost" {
            return Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        if let Ok(address) = address.parse::<IpAddr>() {
            if address.is_unspecified() || address.is_loopback() {
                return Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            specific = Some(address);
        }
    }
    specific
}

pub(crate) fn tunnel_target_for_session(
    host: &crate::config::SshRemoteHost,
    protocol: DesktopProtocol,
    port: u16,
) -> std::net::IpAddr {
    use std::net::{IpAddr, Ipv4Addr};

    super::setup::detect::scan(host, Duration::from_secs(5))
        .ok()
        .and_then(|report| tunnel_target(&report.service(protocol).listeners, port))
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn find_port_suggestion(
    service: &super::setup::model::ServiceInventory,
    protocol: DesktopProtocol,
    configured_port: u16,
) -> Option<u16> {
    let detected = detected_port(service, protocol, configured_port)?;
    (detected != configured_port).then_some(detected)
}

fn show_service_inventory(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    configured_port: u16,
    request_manual_scan: &mut bool,
) -> Option<super::setup::plan::Intent> {
    let mut requested = None;
    use super::setup::model::Readiness;
    let readiness = report.readiness(protocol);
    let color = match readiness {
        Readiness::Ready => ui.visuals().hyperlink_color,
        Readiness::InstalledStopped
        | Readiness::RunningSystemd
        | Readiness::RunningManual
        | Readiness::RunningNotReachable
        | Readiness::NotReachable
        | Readiness::HandshakeSucceededUnconfirmed => ui.visuals().warn_fg_color,
        Readiness::Failed => ui.visuals().error_fg_color,
        _ => ui.visuals().weak_text_color(),
    };
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(28, 31, 42))
        .stroke(egui::Stroke::NONE)
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(12.0)
        .show(ui, |ui| {
        requested = show_service_inventory_card(
            ui,
            report,
            protocol,
            configured_port,
            request_manual_scan,
            color,
        );
    });
    requested
}

fn show_service_history(
    ui: &mut egui::Ui,
    host: &crate::config::SshRemoteHost,
    protocol: DesktopProtocol,
    report: &super::setup::model::ServiceInventoryReport,
    history: Option<&Result<Vec<super::setup::history::HistoryRecord>, String>>,
    allow_undo: bool,
) -> Option<super::setup::history::HistoryRecord> {
    let records = match history {
        Some(Ok(records)) => records
            .iter()
            .filter(|record| record.protocol == protocol.label())
            .collect::<Vec<_>>(),
        Some(Err(error)) => {
            egui::CollapsingHeader::new("Local service history")
                .id_salt(("service_history_error", protocol as u8))
                .show(ui, |ui| {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("History unavailable: {error}"),
                    );
                });
            return None;
        }
        None => return None,
    };
    if records.is_empty() {
        return None;
    }

    let mut requested_undo = None;
    egui::CollapsingHeader::new("Recent service changes")
        .id_salt(("service_history", protocol as u8))
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("service_history_scroll", protocol as u8))
                .max_height(180.0)
                .show(ui, |ui| {
                    for (index, record) in records.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&record.action);
                    ui.weak(&record.at);
                    if let Some(distro) = &record.distro {
                        ui.weak(distro);
                    }
                    ui.weak(if record.completed { "Complete" } else { "Partial" });
                    ui.weak(format!("{} step(s)", record.completed_steps.len()));
                    if index == 0 && protocol == DesktopProtocol::Rdp {
                        let service = report.service(protocol);
                        let undo_available =
                            service.unit_present == Some(true)
                                && super::setup::plan::build_restore_plan(
                                    record.prior_unit_active,
                                    record.prior_unit_enabled,
                                    service.unit_active,
                                    service.unit_enabled,
                                )
                                .is_ok();
                        if allow_undo
                            && ui
                                .add_enabled(undo_available, egui::Button::new("Undo"))
                                .on_disabled_hover_text(
                                    "Undo needs a known prior state and a current systemd status.",
                                )
                                .clicked()
                        {
                            requested_undo = Some((**record).clone());
                        }
                    }
                });
            }
            ui.weak(format!("History for {} · {}@{}:{}", host.name, host.user, host.host, host.port));
                });
        });
    requested_undo
}

fn show_service_scan_placeholder(ui: &mut egui::Ui, protocol: DesktopProtocol) {
    let service_name = match protocol {
        DesktopProtocol::Rdp => "xrdp",
        DesktopProtocol::Vnc => "TigerVNC",
    };
    egui::Frame::group(ui.style())
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_min_height(180.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong(format!("Checking {service_name} over SSH…"));
            });
            ui.add_space(8.0);
            for check in [
                "SSH host",
                "Operating system and service package",
                "Service state",
                "Listening port and bind address",
                "Protocol handshake",
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("◌").color(ui.visuals().weak_text_color()));
                    ui.label(egui::RichText::new(check).color(ui.visuals().weak_text_color()));
                });
            }
        });
}

fn show_service_inventory_card(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    configured_port: u16,
    request_manual_scan: &mut bool,
    color: egui::Color32,
) -> Option<super::setup::plan::Intent> {
    use super::setup::model::{Exposure, InitSystem, Readiness, ServiceState};

    let service = report.service(protocol);
    let handshake = report.handshake(protocol);
    let (label, _) = match report.readiness(protocol) {
        Readiness::NotInstalled => ("Not installed", ui.visuals().weak_text_color()),
        Readiness::InstalledStopped => {
            ("Installed, not running", ui.visuals().warn_fg_color)
        }
        Readiness::Failed => ("Service failed", ui.visuals().error_fg_color),
        Readiness::RunningSystemd => (
            "Service running · handshake not tested",
            ui.visuals().warn_fg_color,
        ),
        Readiness::RunningManual => (
            "Running process · handshake not tested",
            ui.visuals().warn_fg_color,
        ),
        Readiness::RunningNotReachable => {
            ("Running, not reachable", ui.visuals().warn_fg_color)
        }
        Readiness::NotReachable => {
            ("Connection check failed", ui.visuals().warn_fg_color)
        }
        Readiness::HandshakeSucceededUnconfirmed => {
            (
                "Handshake succeeded; service state is unconfirmed",
                ui.visuals().warn_fg_color,
            )
        }
        Readiness::Ready => (
            "OK · running and protocol handshake succeeded",
            ui.visuals().hyperlink_color,
        ),
        Readiness::LimitedDetection => {
            ("Limited detection", ui.visuals().weak_text_color())
        }
        Readiness::Unknown => ("Status unknown", ui.visuals().weak_text_color()),
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, egui::RichText::new("●").size(14.0));
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).size(14.0).strong());
            if let Some(version) = &service.version {
                ui.label(egui::RichText::new(format!("Version {version}")).small().weak());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pill_text = if protocol == DesktopProtocol::Vnc {
                service.displays.first().map(|d| format!("Display {} → :{}", d.display, d.port))
            } else {
                service.listeners.first().map(|l| format!("Port :{}", l.port))
            };
            if let Some(pill) = pill_text {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(38, 43, 58))
                    .rounding(egui::Rounding::same(999.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(pill).small());
                    });
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let ok_color = ui.visuals().hyperlink_color;
    let warn_color = ui.visuals().warn_fg_color;
    let installed_val = match (service.installed, service.version.as_deref()) {
        (Some(true), Some(v)) => v.to_owned(),
        (Some(true), None) => "Detected".into(),
        (Some(false), _) => "Not installed".into(),
        (None, _) => "Unknown".into(),
    };
    let svc_active = service.unit_active == Some(true) || service.manual_process == Some(true);
    let svc_val = if service.manual_process == Some(true) {
        "Manual process"
    } else if service.unit_active == Some(true) {
        "Systemd active"
    } else if service.unit_active == Some(false) {
        "Inactive"
    } else {
        "Unavailable"
    };
    let hs_ok = matches!(handshake, Some(Ok(_)));
    let hs_val = match handshake {
        Some(Ok(_)) => if protocol == DesktopProtocol::Vnc { "VNC 3.8" } else { "RDP OK" },
        Some(Err(_)) => "Failed",
        None => "Not tested",
    };
    let ssh_user = report.user.as_deref().unwrap_or("unknown");

    ui.columns(4, |cols| {
        let mut kpi = |col: &mut egui::Ui, ok: bool, title: &str, val: &str| {
            col.horizontal(|ui| {
                ui.colored_label(if ok { ok_color } else { warn_color }, if ok { "✓" } else { "–" });
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).small().weak());
                    ui.label(egui::RichText::new(val).small().strong());
                });
            });
        };
        kpi(&mut cols[0], true, "SSH", ssh_user);
        kpi(&mut cols[1], service.installed == Some(true), "Installed", &installed_val);
        kpi(&mut cols[2], svc_active, "Service", svc_val);
        kpi(&mut cols[3], hs_ok, "Handshake", hs_val);
    });
    if service.unit_failed == Some(true) {
        ui.colored_label(ui.visuals().error_fg_color, "Service unit reports a failed state");
        let prefix = match protocol {
            DesktopProtocol::Rdp => "# RDP log ",
            DesktopProtocol::Vnc => "# VNC log ",
        };
        let log_tail = report
            .raw_output
            .lines()
            .filter_map(|line| line.strip_prefix(prefix))
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        if !log_tail.is_empty() {
            egui::CollapsingHeader::new("Recent service logs")
                .id_salt(("service_logs", protocol as u8))
                .show(ui, |ui| {
                    if ui.small_button("Copy logs").clicked() {
                        ui.ctx().copy_text(log_tail.clone());
                    }
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&log_tail).monospace()).wrap());
                    });
                });
        }
    }

    // Compact listener + exposure + display into a single row each
    let unique_ports: Vec<u16> = {
        let mut ps: Vec<u16> = service.listeners.iter().map(|l| l.port).collect();
        ps.sort();
        ps.dedup();
        ps
    };
    let exposure_suffix = match service.exposure() {
        Exposure::AllInterfaces => " · all interfaces ⚠",
        Exposure::LocalhostOnly => " · localhost only",
        Exposure::SpecificAddress(_) => " · specific address",
        Exposure::Unknown => "",
    };
    if unique_ports.is_empty() {
        ui.label(egui::RichText::new("– No matching listener found").weak());
    } else {
        let ports_str = unique_ports.iter().map(|p| format!(":{p}")).collect::<Vec<_>>().join(", ");
        let line = format!("✓ Listening on {ports_str}{exposure_suffix}");
        if service.exposure() == Exposure::AllInterfaces {
            ui.colored_label(ui.visuals().warn_fg_color, line);
        } else {
            ui.label(line);
        }
    }
    if protocol == DesktopProtocol::Vnc {
        for display in &service.displays {
            let user = display.user.as_deref().map_or(String::new(), |u| format!(" · {u}"));
            ui.label(
                egui::RichText::new(format!("Display {} → :{}{}", display.display, display.port, user))
                    .weak()
            );
        }
    }
    if let Some(port) = service.configured_port {
        ui.label(egui::RichText::new(format!("Configured port {port}")).weak());
    }
    egui::CollapsingHeader::new("Host details")
        .id_salt(("service_inventory_host_details", protocol as u8))
        .default_open(false)
        .show(ui, |ui| {
            let os_name = report
                .os_pretty_name
                .as_deref()
                .or(report.os_id.as_deref())
                .unwrap_or("Operating system unknown");
            ui.label(format!(
                "{os_name}{} · {}",
                report
                    .os_version
                    .as_deref()
                    .map_or(String::new(), |version| format!(" {version}")),
                match report.init_system {
                    Some(InitSystem::Systemd) => "systemd",
                    Some(InitSystem::Other) => "non-systemd init",
                    Some(InitSystem::Unknown) | None => "init system unknown",
                }
            ));
            if let Some(package_manager) = &report.package_manager {
                ui.label(format!("Package manager: {package_manager}"));
            }
            if report.sudo_available == Some(false) {
                ui.label("sudo is unavailable for this SSH user");
            } else if report.sudo_available == Some(true) {
                ui.label(if report.passwordless_sudo == Some(true) {
                    "Passwordless sudo is available"
                } else {
                    "sudo is available; it may require authentication"
                });
            }
            if !report.desktop_sessions.is_empty() {
                ui.label(format!(
                    "Desktop sessions: {}",
                    report.desktop_sessions.join(", ")
                ));
            }
            for hint in &report.firewall_hints {
                ui.label(format!("Firewall: {hint}"));
            }
            if let Some(memory) = report.available_memory_mb {
                ui.label(format!("Available memory: {memory} MiB"));
            }
            if let Some(disk) = report.free_disk_mb {
                ui.label(format!("Free disk in home filesystem: {disk} MiB"));
            }
        });
    match handshake {
        Some(Ok(info)) => {
            ui.label(format!("✓ Protocol handshake · {info}"));
        }
        Some(Err(error)) => {
            ui.colored_label(ui.visuals().warn_fg_color, format!("◐ {error}"));
        }
        None if service.state() == ServiceState::RunningSystemd || service.state() == ServiceState::RunningManual => {
            ui.label("Protocol handshake not tested");
        }
        None => {}
    }
    let raw_protocol = protocol.label();
    let raw_output = report
        .raw_output
        .lines()
        .filter(|line| {
            line.starts_with(&format!("# {raw_protocol} "))
                || line.starts_with("# Listener snapshot")
                || line.starts_with("SLD/1\tfield\thost\t")
                || (line.starts_with("SLD/1\tfield\trdp\t")
                    && protocol == DesktopProtocol::Rdp)
                || (line.starts_with("SLD/1\tfield\tvnc\t")
                    && protocol == DesktopProtocol::Vnc)
                || (line.starts_with("SLD/1\tlistener\trdp\t")
                    && protocol == DesktopProtocol::Rdp)
                || (line.starts_with("SLD/1\tlistener\tvnc\t")
                    && protocol == DesktopProtocol::Vnc)
                || (line.starts_with("SLD/1\tdisplay\tvnc\t")
                    && protocol == DesktopProtocol::Vnc)
                || *line == "SLD/1\tbegin"
                || *line == "SLD/1\tend"
        })
        .collect::<Vec<_>>()
        .join("\n");
    egui::CollapsingHeader::new("Show raw output")
        .id_salt(("service_inventory_raw", protocol as u8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(raw_output.clone());
                }
            });
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(raw_output.as_str()).monospace())
                            .wrap(),
                    );
                });
        });

    egui::CollapsingHeader::new("Manual instructions")
        .id_salt(("service_inventory_manual", protocol as u8))
        .show(ui, |ui| {
            let Some(instructions) = super::setup::manual::instructions(
                report.os_id.as_deref(),
                protocol,
                configured_port,
            ) else {
                ui.label("No automatic guidance is available for this distribution. Use its official xrdp/TigerVNC documentation. The app will not guess package or service commands.");
                return;
            };
            ui.label(instructions.note);
            show_manual_commands(ui, "Install manually", &instructions.install);
            show_manual_commands(ui, "Start manually", &instructions.start);
            show_manual_commands(ui, "Verify", &instructions.verify);
            if ui.button("I did it manually, check again").clicked() {
                *request_manual_scan = true;
            }
        });

    let mut requested = None;
    let mut actions = Vec::new();
    match service.state() {
        ServiceState::NotInstalled => {
            let label = match protocol {
                DesktopProtocol::Rdp => "Install and start",
                DesktopProtocol::Vnc => "Install TigerVNC",
            };
            actions.push((label, super::setup::plan::Intent::InstallAndStart));
        }
        ServiceState::InstalledStopped | ServiceState::Failed => {
            actions.push(("Start", super::setup::plan::Intent::Start));
            actions.push(("Restart", super::setup::plan::Intent::Restart));
            if service.state() == ServiceState::Failed {
                actions.push(("Reinstall", super::setup::plan::Intent::InstallAndStart));
            }
            actions.push(("⟳ Update", super::setup::plan::Intent::Update));
            actions.push(("Uninstall", super::setup::plan::Intent::Uninstall));
        }
        ServiceState::RunningSystemd => {
            actions.push(("Restart", super::setup::plan::Intent::Restart));
            actions.push(("Stop", super::setup::plan::Intent::Stop));
            if service.unit_enabled != Some(true) {
                actions.push(("Enable at boot", super::setup::plan::Intent::EnableAtBoot));
            }
            actions.push(("⟳ Update", super::setup::plan::Intent::Update));
            actions.push(("Uninstall", super::setup::plan::Intent::Uninstall));
        }
        ServiceState::RunningManual => {
            actions.push(("⟳ Update", super::setup::plan::Intent::Update));
            actions.push(("Uninstall", super::setup::plan::Intent::Uninstall));
        }
        ServiceState::Unknown => {}
    }
    let mut first_block_reason: Option<&'static str> = None;
    let mut any_shown = false;
    ui.add_space(4.0);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (title, intent) in actions {
            match super::setup::plan::build_plan(report, protocol, intent) {
                Ok(_) => {
                    any_shown = true;
                    let is_uninstall = matches!(intent, super::setup::plan::Intent::Uninstall);
                    let is_update = matches!(intent, super::setup::plan::Intent::Update);
                    let btn = egui::Button::new(title).rounding(egui::Rounding::same(6.0));
                    let btn = if is_uninstall {
                        // Red-tinted danger button
                        btn.fill(egui::Color32::from_rgba_unmultiplied(160, 40, 40, 80))
                           .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(180, 70, 70)))
                    } else if is_update {
                        // Blue-tinted accent button
                        btn.fill(egui::Color32::from_rgba_unmultiplied(40, 80, 160, 80))
                           .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(70, 120, 200)))
                    } else {
                        btn
                    };
                    if ui.add(btn).clicked() {
                        requested = Some(intent);
                    }
                }
                Err(reason) => {
                    if first_block_reason.is_none() {
                        first_block_reason = Some(reason);
                    }
                }
            }
        }
    });
    if !any_shown {
        if let Some(reason) = first_block_reason {
            ui.label(egui::RichText::new(reason).small().weak());
        }
    }
    requested
}

fn show_manual_commands(ui: &mut egui::Ui, title: &str, commands: &[String]) {
    egui::CollapsingHeader::new(title).show(ui, |ui| {
        if ui.small_button("Copy all").clicked() {
            ui.ctx().copy_text(commands.join("\n"));
        }
        for (index, command) in commands.iter().enumerate() {
            ui.push_id((title, index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(command);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(command.clone());
                    }
                });
            });
        }
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProbeRequest {
    pub host: String,
    pub port: u16,
    pub protocol: DesktopProtocol,
    pub rdp_security: RdpSecurity,
    pub ssh_host: Option<crate::config::SshRemoteHost>,
    pub tunnel_via_ssh: bool,
    pub gateway: Option<String>,
    pub config_path: Option<std::path::PathBuf>,
}

pub(crate) fn run(request: &ProbeRequest, timeout: Duration) -> Result<String, String> {
    let target = if request.tunnel_via_ssh {
        request.ssh_host.as_ref().and_then(|host| {
            super::setup::detect::scan(host, timeout.min(Duration::from_secs(5)))
                .ok()
                .and_then(|report| {
                    tunnel_target(&report.service(request.protocol).listeners, request.port)
                })
        })
    } else {
        None
    };
    run_with_target(request, target, timeout)
}

fn run_with_target(
    request: &ProbeRequest,
    tunnel_target: Option<std::net::IpAddr>,
    timeout: Duration,
) -> Result<String, String> {
    if request.tunnel_via_ssh
        && request
            .gateway
            .as_deref()
            .is_some_and(|gateway| !gateway.trim().is_empty())
    {
        return Err("RD Gateway and Tunnel via SSH cannot be used together.".into());
    }

    if request.tunnel_via_ssh {
        let ssh_host = request
            .ssh_host
            .as_ref()
            .ok_or_else(|| "SSH tunnel: host settings are unavailable.".to_owned())?;
        let mut stream = super::tunnel::open(
            ssh_host,
            tunnel_target.unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            request.port,
            timeout,
            request.config_path.as_deref(),
        )
            .map_err(|error| format!("SSH tunnel: {error}"))?;
        return check_handshake(request, &mut *stream)
            .map(|info| format!("{info} through the SSH tunnel. Authentication was not tested."));
    }

    if let Some(gateway) = request
        .gateway
        .as_deref()
        .filter(|gateway| !gateway.trim().is_empty())
    {
        if request.protocol != DesktopProtocol::Rdp {
            return Err("RD Gateway can only be used for RDP.".into());
        }
        let host = request
            .ssh_host
            .as_ref()
            .ok_or_else(|| "RD Gateway: host settings are unavailable.".to_owned())?;
        let profile = host
            .rdp
            .as_ref()
            .ok_or_else(|| "RD Gateway: save the RDP profile before testing.".to_owned())?;
        let configured_gateway = super::rdp::gateway_endpoint(&profile.gateway).map_err(|_| {
            "RD Gateway: enter a valid gateway address in the RDP profile.".to_owned()
        })?;
        let requested_gateway = super::rdp::gateway_endpoint(gateway).map_err(|_| {
            "RD Gateway: enter a valid gateway address in the RDP profile.".to_owned()
        })?;
        if configured_gateway != requested_gateway || profile.port != request.port {
            return Err(
                "RD Gateway: save the current RDP connection settings before testing.".into(),
            );
        }
        let password = super::credentials::load(host, DesktopProtocol::Rdp)
            .map_err(|error| format!("RD Gateway credentials: {error}"))?
            .ok_or_else(|| "RD Gateway: save the RDP password before testing.".to_owned())?;
        let username = if profile.same_username_as_ssh {
            &host.user
        } else {
            &profile.username
        };
        let (mut stream, _) = super::rdp::open_gateway(
            gateway,
            &request.host,
            request.port,
            username,
            &profile.domain,
            &password,
        )
        .map_err(|error| format!("RD Gateway: {error}"))?;
        return check_handshake(request, &mut *stream)
            .map(|info| format!("{info} through RD Gateway. RDP authentication was not tested."));
    }

    let deadline = Instant::now() + timeout;
    let addresses = (request.host.as_str(), request.port)
        .to_socket_addrs()
        .map_err(|_| "DNS: could not resolve the host. Check the address in General.".to_owned())?;
    let mut connection = None;
    let mut last_connect_error = None;
    for address in addresses.take(16) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3))) {
            Ok(socket) => {
                connection = Some(socket);
                break;
            }
            Err(error) => last_connect_error = Some(error.kind()),
        }
    }
    let hint = match request.protocol {
        DesktopProtocol::Rdp => "Check that Remote Desktop or xrdp is listening.",
        DesktopProtocol::Vnc => "Check that your VNC server is listening on this port.",
    };
    let socket = connection.ok_or_else(|| {
        let reason = match last_connect_error {
            Some(std::io::ErrorKind::ConnectionRefused) => "connection refused",
            Some(std::io::ErrorKind::TimedOut) => "connection timed out",
            _ => "could not connect",
        };
        format!("TCP: {reason} on port {}. {hint}", request.port)
    })?;
    let mut stream = TimedSocket { socket, deadline };
    check_handshake(request, &mut stream)
        .map(|info| format!("{info}. Authentication was not tested."))
}

fn check_handshake<S: Read + Write + ?Sized>(
    request: &ProbeRequest,
    stream: &mut S,
) -> Result<String, String> {
    let hint = match request.protocol {
        DesktopProtocol::Rdp => "Check that Remote Desktop or xrdp is listening.",
        DesktopProtocol::Vnc => "Check that your VNC server is listening on this port.",
    };
    let info = match request.protocol {
        DesktopProtocol::Rdp => rdp(stream, request.rdp_security),
        DesktopProtocol::Vnc => vnc(stream),
    }
    .map_err(|error| format!("Handshake: {error}. {hint}"))?;
    Ok(info)
}

struct TimedSocket {
    socket: TcpStream,
    deadline: Instant,
}

impl TimedSocket {
    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::Error::new(io::ErrorKind::TimedOut, "probe timed out"))
        } else {
            Ok(remaining)
        }
    }
}

impl Read for TimedSocket {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(buffer)
    }
}

impl Write for TimedSocket {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason.to_owned())
}

fn rdp<S: Read + Write + ?Sized>(stream: &mut S, security: RdpSecurity) -> io::Result<String> {
    let offered: u8 = match security {
        RdpSecurity::Auto => 1 | 2 | 8,
        RdpSecurity::Nla => 2 | 8,
        RdpSecurity::Tls => 1,
    };
    let request = [
        3, 0, 0, 19, 14, 0xe0, 0, 0, 0, 0, 0, 1, 0, 8, 0, offered, 0, 0, 0,
    ];
    stream.write_all(&request)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u16::from_be_bytes([header[2], header[3]]);
    if header[..2] != [3, 0] || !matches!(size, 11 | 19) {
        return Err(invalid("expected an RDP packet header"));
    }
    let mut body = [0; 15];
    let length = usize::from(size - 4);
    stream.read_exact(&mut body[..length])?;
    if usize::from(body[0]) + 1 != length || body[1] != 0xd0 || body[6] != 0 {
        return Err(invalid("expected an X.224 connection confirmation"));
    }
    if size == 11 {
        return Err(invalid(
            "the server did not negotiate encrypted RDP security",
        ));
    }
    if body[9..11] != [8, 0] {
        return Err(invalid("invalid RDP negotiation length"));
    }
    let protocol = u32::from_le_bytes([body[11], body[12], body[13], body[14]]);
    match (body[7], protocol, security) {
        (2, 2 | 8, RdpSecurity::Auto | RdpSecurity::Nla) => Ok("RDP endpoint agreed to NLA".into()),
        (2, 1, RdpSecurity::Auto | RdpSecurity::Tls) => Ok("RDP endpoint agreed to TLS".into()),
        (3, 5, _) => Err(invalid("the server requires NLA")),
        (3, _, _) => Err(invalid(&format!(
            "the server rejected RDP negotiation (code {protocol})"
        ))),
        _ => Err(invalid(
            "the server selected an unexpected RDP security mode",
        )),
    }
}

fn vnc<S: Read + Write + ?Sized>(stream: &mut S) -> io::Result<String> {
    let mut banner = [0; 12];
    stream.read_exact(&mut banner)?;
    let version = match &banner {
        b"RFB 003.003\n" => "3.3",
        b"RFB 003.007\n" => "3.7",
        b"RFB 003.008\n" => "3.8",
        _ => {
            return Err(invalid(
                "expected a supported VNC version (3.3, 3.7, or 3.8)",
            ))
        }
    };
    stream.write_all(&banner)?;
    let security = if version == "3.3" {
        let mut kind = [0; 4];
        stream.read_exact(&mut kind)?;
        vec![u8::try_from(u32::from_be_bytes(kind))
            .map_err(|_| invalid("unsupported VNC security type"))?]
    } else {
        let mut count = [0];
        stream.read_exact(&mut count)?;
        let mut types = vec![0; usize::from(count[0])];
        stream.read_exact(&mut types)?;
        types
    };
    let kind = if security.contains(&2) {
        "VNC password authentication offered"
    } else if security.contains(&1) {
        "no authentication offered"
    } else {
        return Err(invalid(
            "the server offered no supported VNC authentication method",
        ));
    };
    Ok(format!("VNC {version} endpoint confirmed, {kind}"))
}

pub(crate) fn menu_status(
    ctx: &egui::Context,
    host: &crate::config::SshRemoteHost,
    protocol: DesktopProtocol,
    config_path: &std::path::Path,
) -> (&'static str, Option<String>) {
    let (port, rdp_security, tunnel_via_ssh, gateway) = match protocol {
        DesktopProtocol::Rdp => match host.rdp.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (
                profile.port,
                profile.security,
                profile.tunnel_via_ssh,
                (!profile.gateway.trim().is_empty()).then(|| profile.gateway.clone()),
            ),
            None => return ("○ Not configured", None),
        },
        DesktopProtocol::Vnc => match host.vnc.as_ref().filter(|profile| profile.enabled) {
            Some(profile) => (
                profile.port,
                RdpSecurity::Auto,
                profile.tunnel_via_ssh,
                None,
            ),
            None => return ("○ Not configured", None),
        },
    };
    let mut ssh_host = host.clone();
    ssh_host.password.clear();
    let request = ProbeRequest {
        host: host.host.trim().trim_matches(['[', ']']).into(),
        port,
        protocol,
        rdp_security,
        ssh_host: Some(ssh_host),
        tunnel_via_ssh,
        gateway,
        config_path: Some(config_path.to_owned()),
    };
    let last = ctx.data_mut(|data| {
        data.get_temp::<(ProbeRequest, ProbeResult)>(probe_result_id(&host.id, protocol))
    });
    if let Some((_, result)) = last.filter(|(tested, _)| tested == &request) {
        return match result {
            Ok(info) => ("● Reachable", Some(info)),
            Err(error) => ("◐ Unreachable", Some(error)),
        };
    }
    ("○ Not tested", None)
}

fn probe_result_id(host_id: &str, protocol: DesktopProtocol) -> egui::Id {
    egui::Id::new(("remote_desktop_probe_result", host_id, protocol as u8))
}

type ProbeResult = Result<String, String>;

#[derive(Default)]
pub(crate) struct ProbeControl {
    pending: Option<(ProbeRequest, std::sync::mpsc::Receiver<ProbeResult>)>,
    last: Option<(ProbeRequest, ProbeResult)>,
}

impl ProbeControl {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        request: Option<ProbeRequest>,
        unavailable: &str,
        timeout: Duration,
    ) {
        if let Some((tested, receiver)) = &self.pending {
            use std::sync::mpsc::TryRecvError;
            let result = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("The connection check stopped unexpectedly.".into()))
                }
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        probe_result_id(
                            tested
                                .ssh_host
                                .as_ref()
                                .map_or(tested.host.as_str(), |host| host.id.as_str()),
                            tested.protocol,
                        ),
                        (tested.clone(), result.clone()),
                    );
                });
                self.last = Some((tested.clone(), result));
                self.pending = None;
            }
        }

        ui.horizontal_wrapped(|ui| {
            let button = ui.add_enabled(
                self.pending.is_none() && request.is_some(),
                egui::Button::new("Test Connection"),
            );
            if button.clicked() {
                if let Some(request) = request.as_ref() {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let target = request.clone();
                    self.pending = Some((request.clone(), receiver));
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(run(&target, timeout));
                        ctx.request_repaint();
                    });
                }
            }

            if self.pending.is_some() {
                ui.spinner();
                ui.label("Testing…");
            } else if request.is_none() {
                ui.label(unavailable);
            } else if let Some((_, result)) = self
                .last
                .as_ref()
                .filter(|(tested, _)| request.as_ref() == Some(tested))
            {
                match result {
                    Ok(info) => {
                        ui.label("● Protocol reachable").on_hover_text(info);
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().warn_fg_color, probe_error_summary(error));
                        egui::CollapsingHeader::new("Connection check details")
                            .id_salt(("protocol_probe_error", request.as_ref().map(|r| r.protocol as u8)))
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(error.chars().take(4096).collect::<String>()).wrap());
                            });
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_actions_name_completed_steps_and_keep_cancellation_classifiable() {
        let completed = vec!["Update package index".to_owned()];
        let error = with_partial_progress(
            Err("Install xrdp failed".into()),
            &completed,
            4,
        )
        .unwrap_err();
        assert!(error.starts_with("Partial completion (1/4"));
        assert!(error.contains("Update package index"));
        assert!(error.contains("Install xrdp failed"));

        let cancelled = with_partial_progress(
            Err("Cancelled after 1 completed step(s).".into()),
            &completed,
            4,
        )
        .unwrap_err();
        assert!(cancelled.starts_with("Cancelled"));
        assert!(cancelled.contains("Update package index"));
    }

    #[test]
    fn connection_failures_get_safe_actionable_summaries() {
        assert!(probe_error_summary("TCP: connection refused on port 3389")
            .contains("No service accepted"));
        assert!(probe_error_summary("TCP: connection timed out on port 5901")
            .contains("may be blocking"));
        assert!(probe_error_summary("Handshake: unsupported security type")
            .contains("does not support"));
        assert!(probe_error_summary("DNS: could not resolve host").contains("resolved"));
        assert!(probe_error_summary("unclassified error").contains("Expand the details"));
    }

    fn detection_host() -> crate::config::SshRemoteHost {
        crate::config::SshRemoteHost {
            id: "host".into(),
            name: "Host".into(),
            host: "example.test".into(),
            port: 22,
            user: "user".into(),
            key_path: String::new(),
            password: String::new(),
            credential_ref: None,
            remote_cwd: String::new(),
            use_agent: true,
            strict_host_key: false,
            rdp: Some(crate::remote_desktop::profile::RdpProfile::default()),
            vnc: Some(crate::remote_desktop::profile::VncProfile::default()),
        }
    }

    #[test]
    fn detection_cache_key_tracks_probe_settings_without_exposing_password() {
        let host = detection_host();
        let original = detection_key(&host);
        assert_eq!(original, detection_key(&host));

        let mut changed = host.clone();
        changed.rdp.as_mut().unwrap().port += 1;
        assert_ne!(original, detection_key(&changed));

        let mut changed = host.clone();
        changed.vnc.as_mut().unwrap().tunnel_via_ssh = false;
        assert_ne!(original, detection_key(&changed));

        let mut changed = host;
        changed.password = "private-test-password".into();
        let key = detection_key(&changed);
        assert_ne!(original, key);
        assert!(!key.contains("private-test-password"));
    }

    #[test]
    fn action_port_update_waits_for_the_post_action_scan_and_is_one_shot() {
        use super::super::setup::model::{Listener, ServiceInventory, ServiceInventoryReport};

        let host = detection_host();
        let key = detection_key(&host);
        let service = ServiceInventory {
            listeners: vec![Listener {
                address: "127.0.0.1".into(),
                port: 3390,
                process: Some("xrdp".into()),
                pid: None,
                owner: None,
            }],
            ..Default::default()
        };
        let report = ServiceInventoryReport {
            rdp: service,
            ..Default::default()
        };
        let started = Instant::now();
        let mut detection = ServerDetection {
            last: Some((key.clone(), Ok(report.clone()), started)),
            action_port_update: Some((
                host.id.clone(),
                key.clone(),
                DesktopProtocol::Rdp,
                started + Duration::from_secs(1),
            )),
            ..Default::default()
        };

        assert_eq!(detection.take_action_port_update(&host, DesktopProtocol::Rdp), None);
        detection.last = Some((key, Ok(report), started + Duration::from_secs(2)));
        assert_eq!(detection.take_action_port_update(&host, DesktopProtocol::Rdp), Some(3390));
        assert_eq!(detection.take_action_port_update(&host, DesktopProtocol::Rdp), None);
    }

    #[test]
    fn action_port_update_does_not_replace_legacy_or_manually_edited_ports() {
        use super::super::setup::model::{Listener, ServiceInventory, ServiceInventoryReport};

        for manual in [None, Some(true)] {
            let mut host = detection_host();
            host.rdp.as_mut().unwrap().port_manual = manual;
            let key = detection_key(&host);
            let started = Instant::now();
            let report = ServiceInventoryReport {
                rdp: ServiceInventory {
                    listeners: vec![Listener {
                        address: "127.0.0.1".into(),
                        port: 3390,
                        process: Some("xrdp".into()),
                        pid: None,
                        owner: None,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut detection = ServerDetection {
                last: Some((key.clone(), Ok(report), started + Duration::from_secs(2))),
                action_port_update: Some((
                    host.id.clone(),
                    key,
                    DesktopProtocol::Rdp,
                    started,
                )),
                ..Default::default()
            };
            assert_eq!(
                detection.take_action_port_update(&host, DesktopProtocol::Rdp),
                None
            );
            assert!(detection.action_port_update.is_none());
        }
    }

    #[test]
    fn action_failures_get_specific_hints_without_treating_unknown_text_as_known() {
        assert!(action_error_hint("Could not get lock /var/lib/dpkg/lock-frontend")
            .unwrap()
            .contains("package manager is busy"));
        assert!(action_error_hint("Unable to locate package xrdp")
            .unwrap()
            .contains("unavailable"));
        assert!(action_error_hint("user is not in the sudoers file").is_some());
        assert!(action_error_hint("bind: Address already in use")
            .unwrap()
            .contains("port is already in use"));
        assert!(action_error_hint("SELinux AVC: denied")
            .unwrap()
            .contains("do not disable SELinux"));
        assert!(action_error_hint("Unable to start the session")
            .unwrap()
            .contains("desktop session"));
        assert_eq!(action_error_hint("unexpected remote output"), None);
    }

    #[test]
    fn scan_failures_distinguish_ssh_auth_dns_refusal_and_timeout() {
        assert!(scan_error_hint("SSH scan failed: Permission denied (publickey)")
            .unwrap()
            .contains("authentication failed"));
        assert!(scan_error_hint("Could not resolve hostname example")
            .unwrap()
            .contains("resolved"));
        assert!(scan_error_hint("connect: Connection refused")
            .unwrap()
            .contains("refused"));
        assert!(scan_error_hint("Connection timed out")
            .unwrap()
            .contains("may be blocking"));
        assert_eq!(scan_error_hint("unclassified remote error"), None);
    }

    #[test]
    fn port_suggestion_uses_detected_listener_without_overriding_matching_port() {
        use crate::remote_desktop::setup::model::{Listener, ServiceInventory, VncDisplay};

        let service = ServiceInventory {
            listeners: vec![Listener {
                address: "0.0.0.0".into(),
                port: 5901,
                process: Some("Xtigervnc".into()),
                pid: Some(42),
                owner: Some("ubuntu".into()),
            }],
            ..Default::default()
        };
        assert_eq!(
            find_port_suggestion(&service, DesktopProtocol::Vnc, 5900),
            Some(5901)
        );
        assert_eq!(
            find_port_suggestion(&service, DesktopProtocol::Vnc, 5901),
            None
        );
        assert_eq!(
            find_port_suggestion(&Default::default(), DesktopProtocol::Vnc, 5900),
            None
        );

        let process_only = ServiceInventory {
            displays: vec![VncDisplay {
                display: ":1".into(),
                port: 5901,
                user: Some("ubuntu".into()),
            }],
            ..Default::default()
        };
        assert_eq!(detected_port(&process_only, DesktopProtocol::Vnc, 5900), Some(5901));

        let display_is_more_specific_than_an_unrelated_listener = ServiceInventory {
            listeners: vec![Listener {
                address: "0.0.0.0".into(),
                port: 5900,
                process: Some("other-service".into()),
                pid: Some(12),
                owner: None,
            }],
            displays: process_only.displays,
            ..Default::default()
        };
        assert_eq!(
            detected_port(
                &display_is_more_specific_than_an_unrelated_listener,
                DesktopProtocol::Vnc,
                5900
            ),
            Some(5901)
        );
    }

    #[test]
    fn protocol_probe_state_is_scoped_to_host() {
        assert_ne!(
            probe_result_id("host-a", DesktopProtocol::Rdp),
            probe_result_id("host-b", DesktopProtocol::Rdp)
        );
        assert_ne!(
            probe_result_id("host-a", DesktopProtocol::Rdp),
            probe_result_id("host-a", DesktopProtocol::Vnc)
        );
    }

    #[test]
    fn tunnel_target_uses_listener_bind_address_safely() {
        use crate::remote_desktop::setup::model::Listener;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        let listener = |address: &str| Listener {
            address: address.into(),
            port: 5901,
            process: None,
            pid: None,
            owner: None,
        };
        let loopback = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(tunnel_target(&[listener("0.0.0.0")], 5901), loopback);
        assert_eq!(tunnel_target(&[listener("[::]")], 5901), loopback);
        assert_eq!(tunnel_target(&[listener("127.0.0.1")], 5901), loopback);
        assert_eq!(tunnel_target(&[listener("localhost")], 5901), loopback);
        assert_eq!(
            tunnel_target(&[listener("192.0.2.12")], 5901),
            Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 12)))
        );
        assert_eq!(
            tunnel_target(&[listener("2001:db8::12")], 5901),
            Some(IpAddr::V6("2001:db8::12".parse::<Ipv6Addr>().unwrap()))
        );
        assert_eq!(tunnel_target(&[listener("unknown")], 5901), None);
        assert_eq!(tunnel_target(&[listener("192.0.2.12")], 5900), None);
    }

    #[test]
    fn requires_transport_settings_for_tunneled_probes() {
        let request = ProbeRequest {
            host: "127.0.0.1".into(),
            port: 5900,
            protocol: DesktopProtocol::Vnc,
            rdp_security: RdpSecurity::Auto,
            ssh_host: None,
            tunnel_via_ssh: true,
            gateway: None,
            config_path: None,
        };
        assert!(run(&request, Duration::from_secs(1))
            .unwrap_err()
            .contains("host settings are unavailable"));
    }

    #[test]
    fn rejects_gateway_route_for_vnc_before_connecting() {
        let request = ProbeRequest {
            host: "127.0.0.1".into(),
            port: 5900,
            protocol: DesktopProtocol::Vnc,
            rdp_security: RdpSecurity::Auto,
            ssh_host: None,
            tunnel_via_ssh: false,
            gateway: Some("gateway.example.test".into()),
            config_path: None,
        };
        assert!(run(&request, Duration::from_secs(1))
            .unwrap_err()
            .contains("only be used for RDP"));
    }

    struct Peer {
        response: io::Cursor<Vec<u8>>,
        request: Vec<u8>,
    }

    impl Peer {
        fn new(response: &[u8]) -> Self {
            Self {
                response: io::Cursor::new(response.to_vec()),
                request: Vec::new(),
            }
        }
    }

    impl Read for Peer {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.response.read(bytes)
        }
    }

    impl Write for Peer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.request.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn checks_vnc_banner_and_security_without_sending_credentials() {
        let mut peer = Peer::new(b"RFB 003.008\n\x02\x01\x02");
        assert!(vnc(&mut peer).unwrap().contains("password"));
        assert_eq!(peer.request, b"RFB 003.008\n");
        assert!(vnc(&mut Peer::new(b"RFB 003.008\n\x01\x13")).is_err());
        assert!(vnc(&mut Peer::new(b"RFB 003.003\n\0\0\0\x02")).is_ok());
    }

    #[test]
    fn rejects_rdp_security_downgrade_and_malformed_reply() {
        let mut reply = [3, 0, 0, 19, 14, 0xd0, 0, 0, 0, 0, 0, 2, 0, 8, 0, 2, 0, 0, 0];
        let mut auto = Peer::new(&reply);
        assert!(rdp(&mut auto, RdpSecurity::Auto).is_ok());
        assert_eq!(auto.request[15], 11);
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_ok());
        reply[15] = 8;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_ok());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_ok());
        reply[15] = 1;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Nla).is_err());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_ok());
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Tls).is_ok());
        reply[3] = 255;
        assert!(rdp(&mut Peer::new(&reply), RdpSecurity::Auto).is_err());
    }
}
