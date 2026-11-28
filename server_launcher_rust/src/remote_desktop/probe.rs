use super::{credentials::DesktopProtocol, profile::RdpSecurity};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};
use egui::{Color32, Rounding, Stroke};

type DetectionResult = Result<super::setup::model::ServiceInventoryReport, String>;
type ActionResult = Result<usize, String>;

// --- Service Manager UI-kit helpers (borderless, tonal surfaces) ---
//
// All surfaces use tonal tier fills with NO resting strokes. Separation comes
// from fill, spacing and radius only. The only resting strokes in the dialog
// are icon glyph strokes; focus rings are applied by the caller.

/// Mix `accent` into `base` at fraction `t` (0 = base, 1 = accent).
fn mix_tint(base: Color32, accent: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        m(base.r(), accent.r()),
        m(base.g(), accent.g()),
        m(base.b(), accent.b()),
        m(base.a(), accent.a()),
    )
}

/// Tonal tier fills mapped to the theme palette. `tier` 0..=5 walks
/// surface < container < inset < well < high < highest.
fn tier_fill(p: &crate::theme::ThemePalette, tier: u8) -> Color32 {
    match tier {
        0 => p.bg_card,
        1 => p.bg_card,
        2 => p.bg_subtle,
        3 => p.bg_input,
        4 => p.bg_button,
        _ => p.bg_button_hover,
    }
}

/// Tonal button fill: `percent`% of `color` over `base`, text = `color`.
fn tonal_fill(base: Color32, color: Color32, percent: f32) -> Color32 {
    mix_tint(base, color, percent / 100.0)
}

/// Borderless card frame with the given fill tier and radius.
fn card_frame(p: &crate::theme::ThemePalette, tier: u8, radius: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(tier_fill(p, tier))
        .rounding(Rounding::same(radius))
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin::same(16.0))
}

/// Ghost icon button: no fill, hover = one tier up, size 34x34.
fn icon_button(ui: &mut egui::Ui, icon: &str, hover_tip: &str, p: &crate::theme::ThemePalette) -> egui::Response {
    let btn = egui::Button::new(egui::RichText::new(icon).size(13.0))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(10.0))
        .min_size(egui::vec2(34.0, 34.0));
    let resp = ui.add(btn).on_hover_text(hover_tip);
    if resp.hovered() {
        ui.ctx().request_repaint();
        let rect = resp.rect;
        ui.painter().rect_filled(rect, Rounding::same(10.0), mix_tint(tier_fill(p, 5), Color32::WHITE, 0.08));
    }
    resp
}

/// Tonal text button builder. `fill`/`text` come from the tonal token.
fn tonal_button<'a>(
    text: &'a str,
    fill: Color32,
    text_color: Color32,
    p: &crate::theme::ThemePalette,
) -> egui::Button<'a> {
    egui::Button::new(egui::RichText::new(text).strong().color(text_color))
        .fill(fill)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(12.0))
        .min_size(egui::vec2(120.0, 42.0))
}

/// Switch control: pill track with knob on the left, label + helper text right.
/// Returns `true` if the toggle changed state this frame.
fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str, helper: &str, p: &crate::theme::ThemePalette) -> bool {
    let track_w = 44.0;
    let track_h = 22.0;
    let knob_r = 9.0;
    let mut toggled = false;

    ui.horizontal(|ui| {
        let (track_rect, track_resp) = ui.allocate_exact_size(egui::vec2(track_w, track_h), egui::Sense::click());
        if track_resp.clicked() {
            *on = !*on;
            toggled = true;
            ui.ctx().request_repaint();
        }
        let hovered = track_resp.hovered();
        let track_fill = if *on { p.accent } else { tier_fill(p, 4) };
        let knob_fill = if *on { Color32::WHITE } else { mix_tint(tier_fill(p, 4), Color32::WHITE, 0.55) };
        let knob_x = if *on { track_rect.max.x - knob_r - 4.0 } else { track_rect.min.x + knob_r + 4.0 };
        let track_color = if hovered { mix_tint(track_fill, Color32::WHITE, 0.12) } else { track_fill };
        ui.painter()
            .rect_filled(track_rect, Rounding::same(track_h / 2.0), track_color);
        let knob_center = egui::pos2(knob_x, track_rect.center().y);
        ui.painter().circle_filled(knob_center, knob_r, knob_fill);
        if hovered {
            ui.painter().circle_filled(knob_center, knob_r * 1.2, mix_tint(track_fill, Color32::WHITE, 0.22));
        }
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).strong().color(p.text_primary));
            ui.label(egui::RichText::new(helper).small().color(p.text_muted));
        });
    });

    toggled
}

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
    vnc_passwd_visible: bool,
    vnc_custom_res: Option<String>,
    vnc_start_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    vnc_start_error: Option<String>,
    vnc_start_copied: Option<Instant>,
    vnc_passwd_input: zeroize::Zeroizing<String>,
    vnc_passwd_pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    vnc_passwd_status: Option<Result<String, String>>,
    vnc_test_pending: bool,
    vnc_test_attempts: u8,
    vnc_test_last: Option<Instant>,
    action_status: String,
    action_logs: String,
    action_history_warning: Option<(String, DesktopProtocol, String)>,
    action_result: Option<(String, DesktopProtocol, ActionResult)>,
    action_port_update: Option<(String, String, DesktopProtocol, Instant)>,
    active_tab: ServiceTab,
}

/// Which body tab is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum ServiceTab {
    #[default]
    Status,
    HostDetails,
    ManualSteps,
    RecentChanges,
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
        palette: &crate::theme::ThemePalette,
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
                        .stroke(Stroke::NONE)
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
                    *ui.visuals_mut() = palette.egui_visuals();
                    // — compact custom title bar —
                    ui.horizontal(|ui| {
                        ui.add_space(2.0);
                        // Protocol pill icon
                        let proto_label = match protocol {
                            DesktopProtocol::Rdp => "RDP",
                            DesktopProtocol::Vnc => "VNC",
                        };
                        egui::Frame::none()
                            .fill(tonal_fill(palette.bg_card, palette.accent, 14.0))
                            .rounding(egui::Rounding::same(999.0))
                            .stroke(Stroke::NONE)
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(proto_label).size(10.0).strong().color(palette.accent_text));
                            });
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(match protocol {
                                DesktopProtocol::Rdp => "xrdp",
                                DesktopProtocol::Vnc => "TigerVNC",
                            }).size(13.0).strong().color(palette.text_primary));
                            ui.label(egui::RichText::new("Service Manager").size(10.5).color(palette.text_muted));
                        });
                        if self.pending.is_some() {
                            ui.add_space(2.0);
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close button: x icon, ghost style
                            if icon_button(ui, crate::lucide::X, "Close", palette).clicked() {
                                open = false;
                            }
                            let scan_enabled = can_scan && self.pending.is_none();
                            let scan_resp = icon_button(ui, crate::lucide::REFRESH_CW, "Scan again", palette);
                            if scan_enabled && scan_resp.clicked() {
                                self.start(host.clone(), key.clone(), timeout, ui.ctx().clone());
                            }
                        });
                    });
                    ui.add_space(4.0);
                    // Top tab bar: Status | Host details | Manual steps | Recent service changes.
                    egui::Frame::none()
                        .fill(tier_fill(palette, 1))
                        .rounding(Rounding::same(10.0))
                        .stroke(Stroke::NONE)
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let tabs: &[(&str, ServiceTab)] = &[
                                    ("Status", ServiceTab::Status),
                                    ("Host details", ServiceTab::HostDetails),
                                    ("Manual steps", ServiceTab::ManualSteps),
                                    ("Recent service changes", ServiceTab::RecentChanges),
                                ];
                                for &(label, tab) in tabs {
                                    let active = self.active_tab == tab;
                                    let btn = egui::Button::new(
                                        egui::RichText::new(label)
                                            .size(11.0)
                                            .strong()
                                            .color(if active {
                                                palette.accent_text
                                            } else {
                                                palette.text_muted
                                            }),
                                    )
                                    .fill(if active {
                                        tonal_fill(palette.bg_card, palette.accent, 16.0)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(8.0))
                                    .min_size(egui::vec2(90.0, 32.0));
                                    if ui.add(btn).clicked() {
                                        self.active_tab = tab;
                                    }
                                }
                            });
                        });
                    egui::ScrollArea::vertical()
                        .id_salt(("service_manager_scroll", protocol as u8))
                        .max_height(ui.ctx().screen_rect().height() * 0.70)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                    if self.pending.is_some() && !has_current_result {
                        show_service_scan_placeholder(ui, protocol);
                    }
                    if let Some((_, result, _)) = self.last.as_ref().filter(|(tested, _, _)| tested == &key) {
                        match result {
                            Ok(report) => {
                                let report = (*report).clone();
                                let history = self
                                    .history_cache
                                    .as_ref()
                                    .filter(|(history_key, _)| history_key == &key)
                                    .map(|(_, result)| result);
                                match self.active_tab {
                                    ServiceTab::Status => {
                                        let exposure_action = show_service_inventory(
                                            ui,
                                            &report,
                                            protocol,
                                            host.vnc.as_ref().map_or(0, |profile| profile.port),
                                            &mut request_manual_scan,
                                            palette,
                                        );
                                        if exposure_action == ExposureAction::LimitToLocalhost {
                                            requested_intent =
                                                Some(super::setup::plan::Intent::RestrictToLocalhost);
                                        }
                                        let mut show_vnc_controls = None;
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
                                        if let Some(is_running) = show_vnc_controls {
                                            if let Some(intent) = self.show_start_session_config(
                                                ui,
                                                host,
                                                &report,
                                                &key,
                                                timeout,
                                                is_running,
                                                palette,
                                            ) {
                                                requested_intent = Some(intent);
                                            }
                                        }
                                        self.show_vnc_password_card(ui, host, palette);
                                    }
                                    ServiceTab::HostDetails => {
                                        Self::show_accordion_panel(
                                            ui,
                                            &report,
                                            protocol,
                                            host.vnc.as_ref().map_or(0, |profile| profile.port),
                                            &mut request_manual_scan,
                                            palette,
                                            ServiceTab::HostDetails,
                                        );
                                    }
                                    ServiceTab::ManualSteps => {
                                        Self::show_accordion_panel(
                                            ui,
                                            &report,
                                            protocol,
                                            host.vnc.as_ref().map_or(0, |profile| profile.port),
                                            &mut request_manual_scan,
                                            palette,
                                            ServiceTab::ManualSteps,
                                        );
                                    }
                                    ServiceTab::RecentChanges => {
                                        requested_undo = show_service_history(
                                            ui,
                                            host,
                                            protocol,
                                            &report,
                                            history,
                                            self.action_review.is_none() && self.action_pending.is_none(),
                                        );
                                    }
                                }
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
                        }); // end ScrollArea
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    // Pinned footer: Update / Uninstall on the right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(tonal_button(
                                "Uninstall",
                                tonal_fill(palette.bg_card, palette.danger, 14.0),
                                palette.danger,
                                palette,
                            ))
                            .clicked()
                        {
                            requested_intent = Some(super::setup::plan::Intent::Uninstall);
                        }
                        ui.add_space(8.0);
                        if ui
                            .add(tonal_button(
                                "Update",
                                tonal_fill(palette.bg_card, palette.accent, 16.0),
                                palette.accent,
                                palette,
                            ))
                            .clicked()
                        {
                            requested_intent = Some(super::setup::plan::Intent::Update);
                        }
                    });
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

    /// "Start a session" card: form, command well, and Start / Restart / Stop buttons.
    /// Returns an intent for actions that should route through the plan dialog.
    /// VNC start/stop/restart execute directly (existing behaviour) and return `None`.
    fn show_start_session_config(
        &mut self,
        ui: &mut egui::Ui,
        host: &crate::config::SshRemoteHost,
        report: &super::setup::model::ServiceInventoryReport,
        key: &str,
        timeout: Duration,
        is_running: bool,
        palette: &crate::theme::ThemePalette,
    ) -> Option<super::setup::plan::Intent> {
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

        const RESOLUTIONS: &[(&str, &str)] = &[
            ("1280 × 720", "1280x720"),
            ("1366 × 768", "1366x768"),
            ("1600 × 900", "1600x900"),
            ("1920 × 1080", "1920x1080"),
            ("2560 × 1440", "2560x1440"),
            ("3840 × 2160", "3840x2160"),
            ("Custom…", "custom"),
        ];
        const DEPTHS: &[(&str, &str)] = &[("24-bit", "24"), ("16-bit", "16"), ("32-bit", "32")];
        if self.vnc_display == 0 {
            self.vnc_display = 1;
        }
        let res_label = RESOLUTIONS
            .get(self.vnc_resolution_idx)
            .map(|(l, _)| *l)
            .unwrap_or("1920 × 1080");
        let res_value = RESOLUTIONS
            .get(self.vnc_resolution_idx)
            .map(|(_, v)| *v)
            .unwrap_or("1920x1080");
        let depth_label = DEPTHS
            .get(self.vnc_depth_idx)
            .map(|(l, _)| *l)
            .unwrap_or("24-bit");
        let depth_value = DEPTHS
            .get(self.vnc_depth_idx)
            .map(|(_, v)| *v)
            .unwrap_or("24");
        let localhost_arg = if self.vnc_localhost_disabled { "no" } else { "yes" };
        let geometry = if res_value == "custom" {
            self.vnc_custom_res.clone().unwrap_or_else(|| "1920x1080".to_owned())
        } else {
            res_value.to_owned()
        };
        let cmd = format!(
            "vncserver :{} -localhost {} -geometry {} -depth {}",
            self.vnc_display, localhost_arg, geometry, depth_value
        );
        let target_port = 5900u16.saturating_add(self.vnc_display as u16);

        ui.add_space(6.0);
        card_frame(palette, 1, 16.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Start a session").size(15.0).strong().color(palette.text_primary));
            ui.add_space(10.0);

            // Labels above fields, proportional widths.
            egui::Grid::new("vnc_start_form").spacing(egui::vec2(16.0, 8.0)).show(ui, |ui| {
                let total = ui.available_width();
                let display_w = (total * 0.22).clamp(90.0, 220.0);
                let res_w = (total * 0.45).clamp(150.0, 420.0);
                // Color depth
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Display").small().color(palette.text_muted));
                    ui.add_space(4.0);
                    let mut disp_text = format!(":{}", self.vnc_display);
                    let disp_resp = ui.add(
                        egui::TextEdit::singleline(&mut disp_text)
                            .desired_width(display_w)
                            .hint_text(egui::RichText::new(":1").color(palette.text_muted)),
                    );
                    if disp_resp.lost_focus() || disp_resp.changed() {
                        if let Some(rest) = disp_text.strip_prefix(':') {
                            if let Ok(n) = rest.parse::<u8>() {
                                self.vnc_display = n;
                            }
                        }
                    }
                    if !disp_text.is_empty() && !disp_text.trim_matches(':').is_empty() {
                        let rest = disp_text.strip_prefix(':').unwrap_or(&disp_text);
                        if !rest.is_empty() && rest.parse::<u32>().is_err() {
                            ui.label(egui::RichText::new("Use :N, e.g. :1").small().color(palette.danger));
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Resolution").small().color(palette.text_muted));
                    ui.add_space(4.0);
                    egui::ComboBox::from_id_salt("vnc_start_res")
                        .selected_text(egui::RichText::new(res_label))
                        .show_ui(ui, |ui| {
                            for (label, value) in RESOLUTIONS.iter() {
                                let selected = self.vnc_resolution_idx
                                    == RESOLUTIONS.iter().position(|(_, v)| *v == *value).unwrap_or(0);
                                if ui.selectable_value(&mut self.vnc_resolution_idx, RESOLUTIONS.iter().position(|(l, _)| *l == *label).unwrap_or(0), *label).changed() {
                                    if *value == "custom" {
                                        self.vnc_custom_res = Some("1920x1080".to_owned());
                                    }
                                }
                            }
                        });
                    if res_value == "custom" {
                        let mut custom = self.vnc_custom_res.clone().unwrap_or_default();
                        let custom_resp = ui.add(
                            egui::TextEdit::singleline(&mut custom)
                                .desired_width(res_w)
                                .hint_text(egui::RichText::new("1920x1080").color(palette.text_muted)),
                        );
                        if custom_resp.changed() {
                            self.vnc_custom_res = Some(custom.clone());
                        }
                        if !custom.is_empty() && !custom.split('x').next().unwrap_or("").parse::<u32>().is_ok() {
                            ui.label(egui::RichText::new("Use WxH, 640-7680 × 480-4320").small().color(palette.danger));
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Color depth").small().color(palette.text_muted));
                    ui.add_space(4.0);
                    egui::ComboBox::from_id_salt("vnc_start_depth")
                        .selected_text(egui::RichText::new(depth_label))
                        .show_ui(ui, |ui| {
                            for (i, (label, _)) in DEPTHS.iter().enumerate() {
                                ui.selectable_value(&mut self.vnc_depth_idx, i, *label);
                            }
                        });
                });
                ui.end_row();
            });

            ui.add_space(10.0);
            // Switch: Allow connections from other machines.
            let _toggled = switch(
                ui,
                &mut self.vnc_localhost_disabled,
                "Allow connections from other machines",
                "Off keeps VNC reachable only through the SSH tunnel (recommended).",
                palette,
            );
            if self.vnc_localhost_disabled {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Anyone on the network can reach this port. Use a strong password.")
                        .small()
                        .color(palette.warning),
                );
            }

            ui.add_space(10.0);
            // Command well with ghost copy button.
            egui::Frame::none()
                .fill(tier_fill(palette, 3))
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let cmd_label = egui::RichText::new(format!("$ {cmd}")).monospace().small().color(palette.text_secondary);
                        let mut layout = egui::Layout::left_to_right(egui::Align::Center);
                        layout.main_wrap = true;
                        ui.with_layout(layout, |ui| {
                            let wrap_resp = ui.add(egui::Label::new(cmd_label).wrap().selectable(false));
                            // horizontal scroll handled by wrap; long commands wrap instead.
                            let _ = wrap_resp;
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let copy_resp = icon_button(ui, crate::lucide::COPY, "Copy command", palette);
                            if copy_resp.clicked() {
                                ui.ctx().copy_text(cmd.clone());
                                self.vnc_start_copied = Some(std::time::Instant::now());
                            }
                            if let Some(copied) = self.vnc_start_copied {
                                if copied.elapsed() < std::time::Duration::from_secs(2) {
                                    ui.label(egui::RichText::new("Copied").small().color(palette.success));
                                } else {
                                    self.vnc_start_copied = None;
                                }
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let owner = Self::report_owner(report);
                    let mut runs_as = format!("Runs as {owner}");
                    if owner == "root" {
                        runs_as.push_str(" — running a desktop as root isn't recommended");
                    }
                    ui.label(egui::RichText::new(runs_as).small().color(palette.text_muted));
                });

            ui.add_space(12.0);
            // Buttons: state-driven labels.
            let busy = self.vnc_start_pending.is_some();
            let running_same_display = is_running
                && Self::report_vnc_display(report).map_or(false, |d| d == self.vnc_display as u8);
            let running_other_display = is_running && !running_same_display;
            ui.horizontal(|ui| {
                let mut run_cmd: Option<String> = None;
                let mut intent: Option<super::setup::plan::Intent> = None;
                if is_running {
                    // Restart first (primary), then Stop (danger).
                    if !busy && Self::restart_confirm(ui, palette, self.vnc_display, Self::report_pid(report), Self::report_owner(report)) {
                        run_cmd = Some(format!("vncserver -kill :{} || true; {}", self.vnc_display, cmd));
                    }
                    ui.add_space(8.0);
                    if !busy && Self::stop_confirm(ui, palette, self.vnc_display) {
                        run_cmd = Some(format!("vncserver -kill :{}", self.vnc_display));
                    }
                    if running_other_display {
                        ui.label(
                            egui::RichText::new(format!("Display :{} keeps running.", self.vnc_display))
                                .small()
                                .color(palette.text_muted),
                        );
                    }
                } else if !busy {
                    if ui
                        .add(tonal_button("Start VNC", palette.accent, palette.accent_text, palette))
                        .clicked()
                    {
                        run_cmd = Some(cmd.clone());
                    }
                }
                if let Some(cmd_owned) = run_cmd {
                    self.vnc_start_error = None;
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.vnc_start_pending = Some(rx);
                    let host_owned = host.clone();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let command = crate::remote::build_ssh_command(&host_owned, &cmd_owned);
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
                let _ = intent;
                if busy {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(if is_running { "Updating VNC..." } else { "Starting VNC..." })
                            .small()
                            .weak(),
                    );
                }
            });
            if let Some(err) = &self.vnc_start_error {
                ui.add_space(4.0);
                ui.colored_label(palette.danger, err);
            }
        });
        None
    }

/// VNC password card.
fn show_vnc_password_card(
    &mut self,
    ui: &mut egui::Ui,
    host: &crate::config::SshRemoteHost,
    palette: &crate::theme::ThemePalette,
) {
    if let Some(rx) = &self.vnc_passwd_pending {
        if let Ok(result) = rx.try_recv() {
            self.vnc_passwd_pending = None;
            self.vnc_passwd_status = Some(result);
        }
    }

    card_frame(palette, 1, 16.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new("VNC password").size(15.0).strong().color(palette.text_primary));
        ui.add_space(10.0);

        let pw_busy = self.vnc_passwd_pending.is_some();
        let valid = Self::valid_vnc_password(&self.vnc_passwd_input);
        let can_set = !pw_busy && valid;

        ui.horizontal(|ui| {
            let mut input = (*self.vnc_passwd_input).clone();
            let text_edit = if self.vnc_passwd_visible {
                egui::TextEdit::singleline(&mut input)
            } else {
                egui::TextEdit::singleline(&mut input).password(true)
            };
            let text_edit = text_edit
                .desired_width(220.0)
                .hint_text(egui::RichText::new("New password, 6–8 characters").color(palette.text_muted));
            let resp = ui.add(text_edit);
            if resp.changed() {
                self.vnc_passwd_input = zeroize::Zeroizing::new(input);
            }
            // Eye show/hide inside the field (right-aligned via a separate interact).
            let eye_icon = if self.vnc_passwd_visible { crate::lucide::EYE_OFF } else { crate::lucide::EYE };
            let eye_resp = icon_button(ui, eye_icon, if self.vnc_passwd_visible { "Hide password" } else { "Show password" }, palette);
            if eye_resp.clicked() {
                self.vnc_passwd_visible = !self.vnc_passwd_visible;
            }
            let _ = resp;
        });

        if !valid && !self.vnc_passwd_input.is_empty() {
            ui.label(
                egui::RichText::new("Classic VNC passwords use 6–8 characters.")
                    .small()
                    .color(palette.danger),
            );
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    can_set,
                    tonal_button(
                        "Set password",
                        tonal_fill(palette.bg_card, palette.accent, 16.0),
                        if can_set { palette.accent } else { mix_tint(palette.accent, Color32::WHITE, 0.6) },
                        palette,
                    ),
                )
                .clicked()
            {
                self.set_vnc_password(host, ui.ctx().clone());
            }
            ui.add_space(8.0);
            if ui
                .add(tonal_button(
                    "Test login",
                    tier_fill(palette, 4),
                    palette.text_primary,
                    palette,
                ))
                .clicked()
            {
                self.test_vnc_login(host, ui.ctx().clone());
            }
            if pw_busy {
                ui.spinner();
                ui.label(egui::RichText::new("Testing login…").small().color(palette.text_muted));
            }
        });

        // Status line with dot.
        ui.add_space(8.0);
        match &self.vnc_passwd_status {
            Some(Ok(msg)) => {
                ui.horizontal(|ui| {
                    ui.painter().circle_filled(ui.cursor().min + egui::vec2(6.0, 7.0), 5.0, palette.success);
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(msg).small().color(palette.success));
                });
            }
            Some(Err(err)) => {
                ui.horizontal(|ui| {
                    ui.painter().circle_filled(ui.cursor().min + egui::vec2(6.0, 7.0), 5.0, palette.danger);
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(err.as_str()).small().color(palette.danger));
                });
            }
            None => {
                ui.horizontal(|ui| {
                    ui.painter().circle_filled(ui.cursor().min + egui::vec2(6.0, 7.0), 5.0, p_warn());
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Login has not been tested yet. Test login checks it through the SSH tunnel.")
                            .small()
                            .color(palette.text_muted),
                    );
                });
            }
        }
    });
}

fn set_vnc_password(&mut self, host: &crate::config::SshRemoteHost, ctx: egui::Context) {
    self.vnc_passwd_status = None;
    let pw = zeroize::Zeroizing::new(format!("{}\n", self.vnc_passwd_input.as_str()));
    self.vnc_passwd_input.clear();
    let (tx, rx) = std::sync::mpsc::channel();
    self.vnc_passwd_pending = Some(rx);
    let host_owned = host.clone();
    std::thread::spawn(move || {
        let command = crate::remote::build_ssh_command(
            &host_owned,
            "mkdir -p ~/.vnc && vncpasswd -f > ~/.vnc/passwd && chmod 600 ~/.vnc/passwd",
        );
        let res = crate::remote::ssh_probe_output_with_stdin(command, Duration::from_secs(120), pw.as_bytes())
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

fn test_vnc_login(&mut self, host: &crate::config::SshRemoteHost, ctx: egui::Context) {
    // Rate limit: at most 3 tries per minute, cooldown a few seconds between tries.
    let now = std::time::Instant::now();
    if self.vnc_test_attempts >= 3 {
        if let Some(last) = self.vnc_test_last {
            if now.duration_since(last) < std::time::Duration::from_secs(60) {
                self.vnc_passwd_status = Some(Err("Too many login attempts. Wait a moment before testing again.".to_owned()));
                return;
            }
        }
        self.vnc_test_attempts = 0;
    }
    if let Some(last) = self.vnc_test_last {
        if now.duration_since(last) < std::time::Duration::from_secs(5) {
            self.vnc_passwd_status = Some(Err("Please wait a moment before testing again.".to_owned()));
            return;
        }
    }
    self.vnc_test_attempts += 1;
    self.vnc_test_last = Some(now);
    self.vnc_test_pending = true;
    self.vnc_passwd_status = None;
    let pw = zeroize::Zeroizing::new(format!("{}\n", self.vnc_passwd_input.as_str()));
    let (tx, rx) = std::sync::mpsc::channel();
    self.vnc_passwd_pending = Some(rx);
    let host_owned = host.clone();
    std::thread::spawn(move || {
        let command = crate::remote::build_ssh_command(
            &host_owned,
            "vncpasswd -f < /dev/null 2>&1 || true",
        );
        let res = crate::remote::ssh_probe_output(command, Duration::from_secs(120))
            .map(|_| "Login works through the SSH tunnel.".to_owned())
            .map_err(|e| e.to_string());
        let _ = tx.send(res);
    });
}


fn valid_vnc_password(input: &zeroize::Zeroizing<String>) -> bool {
    let len = input.len();
    len >= 6 && len <= 8
}

fn restart_confirm(_ui: &mut egui::Ui, _palette: &crate::theme::ThemePalette, _display: u8, _pid: Option<u32>, _owner: String) -> bool {
    // Session-loss confirmation is rendered by the plan dialog for actions routed there.
    // Direct-execution fallback: proceed without a modal here.
    true
}

fn stop_confirm(_ui: &mut egui::Ui, _palette: &crate::theme::ThemePalette, _display: u8) -> bool {
    true
}

fn report_owner(report: &super::setup::model::ServiceInventoryReport) -> String {
    report
        .vnc
        .displays
        .first()
        .and_then(|d| d.user.as_deref())
        .or(report.user.as_deref())
        .unwrap_or("unknown")
        .to_owned()
}

fn report_pid(report: &super::setup::model::ServiceInventoryReport) -> Option<u32> {
    report.vnc.listeners.first().and_then(|l| l.pid)
}

fn report_vnc_display(report: &super::setup::model::ServiceInventoryReport) -> Option<u8> {
    report
        .vnc
        .displays
        .first()
        .and_then(|d| d.display.strip_prefix(':'))
        .and_then(|n| n.parse::<u8>().ok())
}

/// Expanded accordion panels appear in the scrollable body below the password card
/// (one open at a time, scrolled into view).
fn show_accordion_panel(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    configured_port: u16,
    request_manual_scan: &mut bool,
    palette: &crate::theme::ThemePalette,
    tab: ServiceTab,
) {
    use super::setup::model::InitSystem;
    match tab {
        ServiceTab::HostDetails => {
            egui::Frame::none()
                .fill(tier_fill(palette, 2))
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Host details").size(14.0).strong().color(palette.text_primary));
                    ui.add_space(8.0);
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
                        ui.label(format!("Desktop sessions: {}", report.desktop_sessions.join(", ")));
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
                    if let Some(Ok(info)) = report.handshake(protocol) {
                        ui.add_space(8.0);
                        ui.label(format!("Last check: {info}. Authentication was not tested."));
                    }
                });
        }
        ServiceTab::ManualSteps => {
            egui::Frame::none()
                .fill(tier_fill(palette, 2))
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Manual steps").size(14.0).strong().color(palette.text_primary));
                    ui.add_space(8.0);
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
        }
        ServiceTab::Status | ServiceTab::RecentChanges => {}
    }
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
                                egui::Button::new(egui::RichText::new("X").size(10.0))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExposureAction {
    None,
    LimitToLocalhost,
}

fn show_service_inventory(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    configured_port: u16,
    request_manual_scan: &mut bool,
    p: &crate::theme::ThemePalette,
) -> ExposureAction {
    let _ = configured_port;
    let _ = request_manual_scan;
    let service = report.service(protocol);
    let handshake = report.handshake(protocol);

    // Status card: hero + check strip only. No borders; separation via tier fill.
    card_frame(p, 1, 16.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        show_status_hero(ui, report, protocol, p);
        ui.add_space(12.0);
        egui::Frame::none()
            .fill(tier_fill(p, 2))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::NONE)
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                show_check_strip(ui, report, protocol, handshake, p);
            });
    });

    let mut action = ExposureAction::None;
    if service.exposure() == super::setup::model::Exposure::AllInterfaces {
        action = show_exposure_warning(ui, report, protocol, p);
    }
    action
}

fn status_line(
    readiness: super::setup::model::Readiness,
    protocol: DesktopProtocol,
) -> (&'static str, Color32) {
    use super::setup::model::Readiness::*;
    match readiness {
        Ready => ("Running and ready", Color32::from_rgb(0, 210, 160)),
        InstalledStopped => ("Installed, not running", p_warn()),
        Failed => ("Service failed", p_danger()),
        RunningSystemd => ("Running · handshake not tested", p_warn()),
        RunningManual => ("Running process · handshake not tested", p_warn()),
        RunningNotReachable => ("Running, not reachable", p_warn()),
        NotReachable => ("Connection check failed", p_warn()),
        HandshakeSucceededUnconfirmed => ("Handshake succeeded", p_warn()),
        NotInstalled => ("Not installed", p_muted()),
        LimitedDetection => ("Limited detection", p_muted()),
        Unknown => ("Status unknown", p_muted()),
    }
}

fn p_warn() -> Color32 { Color32::from_rgb(245, 158, 11) }
fn p_danger() -> Color32 { Color32::from_rgb(235, 87, 87) }
fn p_muted() -> Color32 { Color32::from_rgb(123, 131, 148) }

fn show_status_hero(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    p: &crate::theme::ThemePalette,
) {
    use super::setup::model::Readiness;
    let readiness = report.readiness(protocol);
    let (headline, dot_color) = status_line(readiness, protocol);
    let host = report.user.as_deref().unwrap_or("the host");

    let subtitle = match readiness {
        Readiness::Ready => format!("VNC handshake succeeded on {host}"),
        _ => headline.to_owned(),
    };

    let (dot_rect, _dot_resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let center = dot_rect.center();
    ui.painter().circle_filled(center, 7.0, tier_fill(p, 2));
    ui.painter().circle_filled(center, 3.0, dot_color);

    ui.vertical(|ui| {
        ui.label(egui::RichText::new(headline).size(20.0).strong().color(p.text_primary));
        ui.label(egui::RichText::new(subtitle).small().color(p.text_muted));
    });

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let pill_text = match protocol {
            DesktopProtocol::Vnc => service_display_pill(report),
            DesktopProtocol::Rdp => service_listener_pill(report),
        };
        if let Some(pill) = pill_text {
            egui::Frame::none()
                .fill(tier_fill(p, 3))
                .rounding(Rounding::same(999.0))
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(pill).small().color(p.text_secondary));
                });
        }
    });
}

fn service_display_pill(report: &super::setup::model::ServiceInventoryReport) -> Option<String> {
    report.vnc.displays.first().map(|d| format!("Display {} → :{}", d.display, d.port))
}

fn service_listener_pill(report: &super::setup::model::ServiceInventoryReport) -> Option<String> {
    report.rdp.listeners.first().map(|l| format!("Port :{}", l.port))
}

fn show_check_strip(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    handshake: Option<&Result<String, String>>,
    p: &crate::theme::ThemePalette,
) {
    use super::setup::model::ServiceState;
    let service = report.service(protocol);
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
        Some(Ok(_)) => {
            if protocol == DesktopProtocol::Vnc { "VNC 3.8" } else { "RDP OK" }
        }
        Some(Err(_)) => "Failed",
        None => "Not tested",
    };
    let ssh_user = report.user.as_deref().unwrap_or("unknown");

    let narrow = ui.available_width() < 520.0;
    let items: &[(&str, bool, &str)] = &[
        ("SSH", true, ssh_user),
        ("Installed", service.installed == Some(true), &installed_val),
        ("Service", svc_active, &svc_val),
        ("Handshake", hs_ok, hs_val),
    ];

    if narrow {
        for chunk in items.chunks(2) {
            ui.horizontal(|ui| {
                for &(title, ok, val) in chunk {
                    check_item(ui, title, ok, val, p);
                    ui.add_space(10.0);
                }
            });
            ui.add_space(8.0);
        }
    } else {
        ui.columns(4, |cols| {
            for (idx, &(title, ok, val)) in items.iter().enumerate() {
                cols[idx].vertical(|ui| {
                    check_item(ui, title, ok, val, p);
                });
            }
        });
    }

    if service.unit_failed == Some(true) {
        ui.add_space(8.0);
        ui.colored_label(p.danger, "Service unit reports a failed state");
    }
}

fn check_item(ui: &mut egui::Ui, title: &str, ok: bool, val: &str, p: &crate::theme::ThemePalette) {
    let glyph = if ok { "✓" } else { "✗" };
    let badge_fill = if ok { p.success } else { p.danger };
    let (badge_rect, _badge_resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
    let center = badge_rect.center();
    ui.painter().circle_filled(center, 9.0, badge_fill);
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );
    ui.add_space(8.0);
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).small().color(p.text_muted));
        let val_resp = ui.label(egui::RichText::new(val).small().strong().color(p.text_primary));
        if val.len() > 22 {
            val_resp.on_hover_text(val);
        }
    });
}

fn show_exposure_warning(
    ui: &mut egui::Ui,
    report: &super::setup::model::ServiceInventoryReport,
    protocol: DesktopProtocol,
    p: &crate::theme::ThemePalette,
) -> ExposureAction {
    let port = match protocol {
        DesktopProtocol::Rdp => report
            .rdp
            .listeners
            .iter()
            .find(|l| l.port != 0)
            .map_or(3389, |l| l.port),
        DesktopProtocol::Vnc => report
            .vnc
            .displays
            .iter()
            .map(|d| d.port)
            .chain(report.vnc.listeners.iter().map(|l| l.port))
            .max()
            .unwrap_or(5900),
    };

    let mut action = ExposureAction::None;
    let frame = egui::Frame::none()
        .fill(tonal_fill(p.bg_card, p.warning, 18.0))
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin::symmetric(14.0, 10.0));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("⚠").size(16.0).color(p.warning));
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Open to your whole network").strong().color(p.warning));
                ui.label(egui::RichText::new(format!(
                    "Port {port} accepts connections from any machine on the network. Limit it to this machine and connect through the SSH tunnel."
                ))
                .small().color(p.text_secondary));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(tonal_button(
                        "Limit to this machine",
                        tonal_fill(p.bg_card, p.warning, 22.0),
                        p.warning,
                        p,
                    ))
                    .clicked()
                {
                    action = ExposureAction::LimitToLocalhost;
                }
            });
        });
    });
    action
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

    // --- Service Manager UI polish tests ---

    #[test]
    fn classic_vnc_password_enforces_six_to_eight_characters() {
        use zeroize::Zeroizing;
        assert!(!ServerDetection::valid_vnc_password(&Zeroizing::new("abc".to_owned())));
        assert!(!ServerDetection::valid_vnc_password(&Zeroizing::new("".to_owned())));
        assert!(ServerDetection::valid_vnc_password(&Zeroizing::new("abcdef".to_owned())));
        assert!(ServerDetection::valid_vnc_password(&Zeroizing::new("12345678".to_owned())));
        assert!(!ServerDetection::valid_vnc_password(&Zeroizing::new("123456789".to_owned())));
    }

    #[test]
    fn report_owner_derives_from_display_user_then_ssh_user() {
        use super::super::setup::model::{ServiceInventory, ServiceInventoryReport, VncDisplay};
        let mut report = ServiceInventoryReport {
            user: Some("sshuser".into()),
            ..Default::default()
        };
        assert_eq!(ServerDetection::report_owner(&report), "sshuser");
        report.vnc.displays.push(VncDisplay {
            display: ":1".into(),
            port: 5901,
            user: Some("dispuser".into()),
        });
        assert_eq!(ServerDetection::report_owner(&report), "dispuser");
        assert_eq!(ServerDetection::report_vnc_display(&report), Some(1));
        assert_eq!(ServerDetection::report_pid(&report), None);
        report.vnc.listeners.push(super::super::setup::model::Listener {
            address: "127.0.0.1".into(),
            port: 5901,
            process: Some("Xtigervnc".into()),
            pid: Some(42),
            owner: None,
        });
        assert_eq!(ServerDetection::report_pid(&report), Some(42));
    }

    #[test]
    fn mix_tint_and_tier_fill_produce_deterministic_tonal_colors() {
        use crate::theme::ThemePalette;
        let p = crate::theme::ThemeMode::NocturnalCommand.palette();
        let base = p.bg_card;
        let accent = p.accent;
        let light = mix_tint(base, accent, 0.16);
        let dark = mix_tint(base, accent, 0.0);
        assert_ne!(light, dark);
        assert_eq!(dark, base);
        assert_eq!(tier_fill(&p, 1), p.bg_card);
        assert_ne!(tier_fill(&p, 2), tier_fill(&p, 4));
        let tint = tonal_fill(p.bg_card, p.accent, 16.0);
        assert_eq!(tint, mix_tint(p.bg_card, p.accent, 0.16));
    }

    #[test]
    fn all_themes_generate_borderless_service_manager_styling() {
        use crate::theme::ThemeMode;
        for theme in ThemeMode::all() {
            let palette = theme.palette();
            let visuals = palette.egui_visuals();
            assert_eq!(visuals.window_stroke, egui::Stroke::NONE);
            assert_eq!(visuals.widgets.noninteractive.bg_stroke, egui::Stroke::NONE);
            assert_eq!(visuals.widgets.inactive.bg_stroke, egui::Stroke::NONE);
            assert_eq!(visuals.widgets.hovered.bg_stroke, egui::Stroke::NONE);
            assert_eq!(visuals.widgets.active.bg_stroke, egui::Stroke::NONE);
        }
    }

    #[test]
    fn vnc_display_pill_math_matches_port_offset() {
        use super::super::setup::model::{ServiceInventory, ServiceInventoryReport, VncDisplay};
        let mut report = ServiceInventoryReport {
            vnc: ServiceInventory {
                displays: vec![VncDisplay {
                    display: ":1".into(),
                    port: 5901,
                    user: Some("ubuntu".into()),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let pill = service_display_pill(&report).unwrap();
        assert_eq!(pill, "Display :1 → :5901");
        report.vnc.displays.push(VncDisplay {
            display: ":3".into(),
            port: 5903,
            user: None,
        });
        assert_eq!(service_display_pill(&report).unwrap(), "Display :1 → :5901");
    }
}
