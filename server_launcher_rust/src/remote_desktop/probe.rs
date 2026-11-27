use super::{credentials::DesktopProtocol, profile::RdpSecurity};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

type DetectionResult = Result<super::setup::model::ServiceInventoryReport, String>;
type ActionResult = Result<usize, String>;

struct PendingAction {
    host: crate::config::SshRemoteHost,
    key: String,
    protocol: DesktopProtocol,
    plan: super::setup::plan::ActionPlan,
    needs_password: bool,
    sudo_available: bool,
    password_attempts: u8,
}

#[derive(Default)]
pub(crate) struct ServerDetection {
    pending: Option<(String, std::sync::mpsc::Receiver<DetectionResult>)>,
    last: Option<(String, DetectionResult, Instant)>,
    action_review: Option<PendingAction>,
    action_password: zeroize::Zeroizing<String>,
    action_pending: Option<std::sync::mpsc::Receiver<super::setup::exec::ActionEvent>>,
    action_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    action_status: String,
    action_logs: String,
    action_result: Option<(String, DesktopProtocol, ActionResult)>,
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
                super::setup::exec::ActionEvent::Finished(result) => {
                    self.action_pending = None;
                    self.action_cancel = None;
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
                            result
                        };
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
        let should_scan = can_scan
            && self.pending.is_none()
            && self
                .last
                .as_ref()
                .map_or(true, |(tested, _, scanned)| {
                    tested != &key || scanned.elapsed() >= Duration::from_secs(60)
                });
        if should_scan {
            self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
        }

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
                    egui::Button::new("Scan again"),
                )
                .clicked()
            {
                self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
            }
        });
        let mut requested_intent = None;
        let mut request_manual_scan = false;
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
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
            }
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
                        needs_password: report.uid != Some(0) && report.passwordless_sudo != Some(true),
                        sudo_available: report.uid == Some(0) || report.sudo_available != Some(false),
                        password_attempts: 0,
                    });
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
                        ui.colored_label(ui.visuals().error_fg_color, format!("Remote action failed: {error}"));
                    }
                }
            }
        }
        self.show_action_review(ui, &key);
    }

    fn show_action_review(&mut self, ui: &mut egui::Ui, key: &str) {
        let Some(action) = self.action_review.as_ref().filter(|action| action.key == key) else {
            return;
        };
        let running = self.action_pending.is_some();
        let mut approve = false;
        let mut cancel = false;
        let mut cancel_after_step = false;
        egui::Window::new("Review remote service action")
            .id(egui::Id::new(("service_action_review", key)))
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ui.ctx(), |ui| {
                ui.heading(action.plan.summary);
                ui.label("Review the exact commands. Nothing runs until you select Run.");
                ui.add_space(6.0);
                for step in &action.plan.steps {
                    ui.label(format!("{} — {}", step.title, step.undo_note));
                    let command = super::setup::exec::command_for_step(
                        step,
                        action.host.user == "root",
                        action.needs_password,
                    )
                    .unwrap_or_else(|_| "Unsupported plan command".into());
                    ui.monospace(command);
                }
                for warning in &action.plan.warnings {
                    ui.colored_label(ui.visuals().warn_fg_color, *warning);
                }
                if !action.sudo_available {
                    ui.colored_label(ui.visuals().error_fg_color, "sudo is not available for this SSH user. Use the manual instructions.");
                } else if action.needs_password {
                    ui.label(format!("Administrator password · attempt {}/3 · used only for this action, never saved:", action.password_attempts + 1));
                    ui.monospace("sudo -S -p '' -- true");
                    ui.add(egui::TextEdit::singleline(&mut *self.action_password).password(true).desired_width(f32::INFINITY));
                } else {
                    ui.label("This account can run the approved command without an administrator password.");
                }
                ui.add_space(8.0);
                if self.action_status.starts_with("Sudo password was rejected") {
                    ui.colored_label(ui.visuals().error_fg_color, &self.action_status);
                }
                ui.horizontal(|ui| {
                    if running {
                        ui.spinner();
                        ui.label(if self.action_status.is_empty() { "Starting approved steps…" } else { self.action_status.as_str() });
                        if ui.button("Cancel after this step").clicked() {
                            cancel_after_step = true;
                        }
                    } else {
                        let can_run = action.sudo_available
                            && (!action.needs_password || !self.action_password.is_empty());
                        if ui.add_enabled(can_run, egui::Button::new("Run")).clicked() {
                            approve = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    }
                });
                if running && !self.action_logs.is_empty() {
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&self.action_logs).monospace()).wrap());
                    });
                }
            });
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
                let password = if action.needs_password {
                    Some(std::mem::take(&mut self.action_password))
                } else {
                    None
                };
                let (sender, receiver) = std::sync::mpsc::channel();
                self.action_pending = Some(receiver);
                self.action_status.clear();
                self.action_logs.clear();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                self.action_cancel = Some(cancel.clone());
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = super::setup::exec::run_approved_plan(
                        &host,
                        &plan,
                        password,
                        &cancel,
                        |event| {
                            let _ = sender.send(event);
                            ctx.request_repaint();
                        },
                    );
                    let _ = sender.send(super::setup::exec::ActionEvent::Finished(result));
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
            return Some(match handshake {
                Ok(_) => "●",
                Err(_) => "◐",
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
        find_port_suggestion(result.as_ref().ok()?.service(protocol), configured_port)
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

fn detection_key(host: &crate::config::SshRemoteHost) -> String {
    format!(
        "{}:{}@{}|{}|{}|{}|{}|{}",
        host.id,
        host.port,
        host.host,
        host.user,
        host.key_path,
        host.credential_ref.as_deref().unwrap_or_default(),
        host.use_agent,
        host.strict_host_key
    )
}

fn detect_servers(
    host: &crate::config::SshRemoteHost,
    timeout: Duration,
) -> DetectionResult {
    let mut report = super::setup::detect::scan(host, timeout)?;
    if let Some(profile) = host.rdp.as_ref().filter(|profile| profile.enabled) {
        if let Some(port) = detected_port(&report.rdp, profile.port) {
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
            report.rdp_handshake = Some(run(&request, timeout));
        }
    }
    if let Some(profile) = host.vnc.as_ref().filter(|profile| profile.enabled) {
        if let Some(port) = detected_port(&report.vnc, profile.port) {
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
            report.vnc_handshake = Some(run(&request, timeout));
        }
    }
    Ok(report)
}

fn detected_port(
    service: &super::setup::model::ServiceInventory,
    configured_port: u16,
) -> Option<u16> {
    service
        .listeners
        .iter()
        .find(|listener| listener.port == configured_port)
        .or_else(|| service.listeners.iter().min_by_key(|listener| listener.port))
        .map(|listener| listener.port)
}

fn find_port_suggestion(
    service: &super::setup::model::ServiceInventory,
    configured_port: u16,
) -> Option<u16> {
    let detected = detected_port(service, configured_port)?;
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
    ui.group(|ui| {
        requested = show_service_inventory_card(
            ui,
            report,
            protocol,
            configured_port,
            request_manual_scan,
        );
    });
    requested
}

fn show_service_inventory_card(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    configured_port: u16,
    request_manual_scan: &mut bool,
) -> Option<super::setup::plan::Intent> {
    use super::setup::model::{Exposure, InitSystem, ServiceState};

    let service = report.service(protocol);
    let (label, color) = match service.state() {
        ServiceState::NotInstalled => ("Not installed", ui.visuals().weak_text_color()),
        ServiceState::InstalledStopped => ("Installed, not running", ui.visuals().warn_fg_color),
        ServiceState::RunningSystemd => ("Service running", ui.visuals().hyperlink_color),
        ServiceState::RunningManual => ("Running process", ui.visuals().hyperlink_color),
        ServiceState::Failed => ("Service failed", ui.visuals().error_fg_color),
        ServiceState::Unknown => ("Status unknown", ui.visuals().weak_text_color()),
    };
    let handshake = report.handshake(protocol);
    let (label, color) = match handshake {
        Some(Ok(_)) => ("OK · protocol handshake succeeded", ui.visuals().hyperlink_color),
        Some(Err(_)) if matches!(service.state(), ServiceState::RunningSystemd | ServiceState::RunningManual) => {
            ("Running, not reachable", ui.visuals().warn_fg_color)
        }
        _ => (label, color),
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, format!("● {label}"));
        if let Some(version) = &service.version {
            ui.label(format!("Version {version}"));
        }
    });

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

    let ssh_user = report.user.as_deref().unwrap_or("SSH user unknown");
    ui.label(format!("✓ SSH reachable as {ssh_user}"));
    if let Some(installed) = service.installed {
        ui.label(if installed {
            "✓ Service executable detected"
        } else {
            "– Service executable not detected"
        });
    } else {
        ui.label("– Installation could not be determined");
    }
    if service.unit_present == Some(true) {
        ui.label("✓ systemd service units found");
    } else if service.unit_present == Some(false) {
        ui.label("– systemd service units not found");
    }
    if service.unit_active == Some(true) {
        ui.label(format!(
            "✓ systemd unit active{}",
            service.unit_enabled.map_or(String::new(), |enabled| if enabled {
                " · enabled at boot".into()
            } else {
                " · not enabled at boot".into()
            })
        ));
    } else if service.manual_process == Some(true) {
        ui.label("✓ Running as a process outside the detected systemd unit");
    } else if service.unit_active == Some(false) {
        ui.label("– systemd unit inactive");
    } else {
        ui.label("– Service manager state unavailable");
    }
    if service.unit_failed == Some(true) {
        ui.colored_label(ui.visuals().error_fg_color, "Service unit reports a failed state");
    }

    let mut ports = service
        .listeners
        .iter()
        .map(|listener| format!("{}:{}", listener.address, listener.port))
        .collect::<Vec<_>>();
    ports.sort();
    ports.dedup();
    if ports.is_empty() {
        ui.label("– No matching listener found");
    } else {
        ui.label(format!("✓ Listening on {}", ports.join(", ")));
        for listener in &service.listeners {
            let process = listener.process.as_deref().unwrap_or("process unknown");
            let pid = listener.pid.map_or(String::new(), |pid| format!(" · PID {pid}"));
            let owner = listener
                .owner
                .as_deref()
                .map_or(String::new(), |owner| format!(" · {owner}"));
            ui.label(format!(
                "{}:{} · {process}{pid}{owner}",
                listener.address, listener.port
            ));
        }
    }

    match service.exposure() {
        Exposure::AllInterfaces => {
            ui.colored_label(ui.visuals().warn_fg_color, "⚠ Listener is bound to all interfaces");
        }
        Exposure::SpecificAddress(address) => {
            ui.label(format!("Listener bound to {address}"));
        }
        Exposure::LocalhostOnly => {
            ui.label("Listener is bound to localhost");
        }
        Exposure::Unknown => {
            ui.label("– Listener exposure unknown");
        }
    }
    if protocol == DesktopProtocol::Vnc {
        for display in &service.displays {
            ui.label(format!(
                "Display {} → port {}{}",
                display.display,
                display.port,
                display.user.as_deref().map_or(String::new(), |user| format!(" · {user}"))
            ));
        }
    }
    if let Some(port) = service.configured_port {
        ui.label(format!("Configured port {port}"));
    }
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
        ServiceState::NotInstalled => actions.push(("Install and start", super::setup::plan::Intent::InstallAndStart)),
        ServiceState::InstalledStopped | ServiceState::Failed => {
            actions.push(("Start", super::setup::plan::Intent::Start));
            actions.push(("Restart", super::setup::plan::Intent::Restart));
        }
        ServiceState::RunningSystemd => {
            actions.push(("Restart", super::setup::plan::Intent::Restart));
            actions.push(("Stop", super::setup::plan::Intent::Stop));
            if service.unit_enabled != Some(true) {
                actions.push(("Enable at boot", super::setup::plan::Intent::EnableAtBoot));
            }
        }
        ServiceState::RunningManual | ServiceState::Unknown => {}
    }
    if protocol == DesktopProtocol::Rdp {
        ui.horizontal_wrapped(|ui| {
            for (title, intent) in actions {
                if super::setup::plan::build_plan(report, protocol, intent).is_ok()
                    && ui.button(title).clicked()
                {
                    requested = Some(intent);
                }
            }
        });
    }
    requested
}

fn show_manual_commands(ui: &mut egui::Ui, title: &str, commands: &[String]) {
    egui::CollapsingHeader::new(title).show(ui, |ui| {
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
    for address in addresses.take(16) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(socket) =
            TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3)))
        {
            connection = Some(socket);
            break;
        }
    }
    let hint = match request.protocol {
        DesktopProtocol::Rdp => "Check that Remote Desktop or xrdp is listening.",
        DesktopProtocol::Vnc => "Check that your VNC server is listening on this port.",
    };
    let socket = connection
        .ok_or_else(|| format!("TCP: could not connect to port {}. {hint}", request.port))?;
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
                        ui.label(format!("○ {error}"));
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
    fn port_suggestion_uses_detected_listener_without_overriding_matching_port() {
        let service = crate::remote_desktop::setup::model::ServiceInventory {
            listeners: vec![crate::remote_desktop::setup::model::Listener {
                address: "0.0.0.0".into(),
                port: 5901,
                process: Some("Xtigervnc".into()),
                pid: Some(42),
                owner: Some("ubuntu".into()),
            }],
            ..Default::default()
        };
        assert_eq!(find_port_suggestion(&service, 5900), Some(5901));
        assert_eq!(find_port_suggestion(&service, 5901), None);
        assert_eq!(find_port_suggestion(&Default::default(), 5900), None);
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
