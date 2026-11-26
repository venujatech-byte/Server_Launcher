use super::profile::*;
use egui::{ComboBox, DragValue, TextEdit, Ui};

fn choice<T: Copy + PartialEq>(ui: &mut Ui, label: &str, value: &mut T, items: &[(T, &str)]) {
    let selected = items.iter().find(|(option, _)| option == value);
    ui.horizontal(|ui| {
        ui.label(label);
        ComboBox::from_id_salt(label)
            .selected_text(selected.map_or("Custom", |(_, title)| *title))
            .show_ui(ui, |ui| {
                for &(option, title) in items {
                    ui.selectable_value(value, option, title);
                }
            });
    });
}

fn text(ui: &mut Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn clipboard(ui: &mut Ui, value: &mut Clipboard) {
    choice(
        ui,
        "Clipboard",
        value,
        &[
            (Clipboard::Both, "Both directions"),
            (Clipboard::RemoteToLocal, "Remote to local"),
            (Clipboard::LocalToRemote, "Local to remote"),
            (Clipboard::Off, "Off"),
        ],
    );
}

fn color_depth(ui: &mut Ui, value: &mut u8) {
    choice(
        ui,
        "Color depth",
        value,
        &[(16, "16 bit"), (24, "24 bit"), (32, "32 bit")],
    );
}

pub(crate) fn rdp(ui: &mut Ui, draft: &mut Option<RdpProfile>) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.checkbox(&mut profile.enabled, "Enable RDP");
    ui.add_enabled_ui(profile.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Port *");
            ui.add(DragValue::new(&mut profile.port).range(1..=u16::MAX));
        });
        ui.checkbox(&mut profile.same_username_as_ssh, "Same username as SSH");
        ui.add_enabled_ui(!profile.same_username_as_ssh, |ui| {
            text(ui, "Username *", &mut profile.username);
        });
        text(ui, "Domain", &mut profile.domain);
        choice(
            ui,
            "Security",
            &mut profile.security,
            &[
                (RdpSecurity::Auto, "Auto"),
                (RdpSecurity::Nla, "NLA"),
                (RdpSecurity::Tls, "TLS"),
            ],
        );
        choice(
            ui,
            "Display painting",
            &mut profile.painting,
            &[
                (RdpPainting::Smooth, "Smooth"),
                (RdpPainting::Fast, "Fast / crisp"),
            ],
        );
        ui.label(
            "Smooth filtering blends scaled pixels; Fast uses sharper nearest-neighbor sampling.",
        );
        choice(
            ui,
            "Frame painting",
            &mut profile.frame_painting,
            &[
                (RdpFramePainting::Incremental, "Incremental regions"),
                (RdpFramePainting::FullFrame, "Full-frame compatibility"),
            ],
        );
        ui.label(
            "Full-frame painting redraws the whole image after each RDP update and may use more resources.",
        );
        choice(
            ui,
            "Quality",
            &mut profile.quality,
            &[
                (Quality::Auto, "Auto / manual settings"),
                (Quality::Best, "Best · 32-bit lossless"),
                (Quality::Good, "Good · 24-bit adaptive"),
                (Quality::Fast, "Fast · 16-bit adaptive"),
            ],
        );
        if profile.quality == Quality::Auto {
            color_depth(ui, &mut profile.color_depth);
        } else {
            ui.label("Quality presets set color depth; adaptive presets allow server color subsampling.");
        }
        choice(
            ui,
            "Resolution",
            &mut profile.resolution,
            &[
                (Resolution::FitWindow, "Fit window (dynamic)"),
                (Resolution::Fixed, "Fixed size"),
                (Resolution::MatchLocalScreen, "Match local screen"),
            ],
        );
        if profile.resolution == Resolution::Fixed {
            ui.horizontal(|ui| {
                ui.add(
                    DragValue::new(&mut profile.width)
                        .range(1..=8192)
                        .prefix("Width "),
                );
                ui.add(
                    DragValue::new(&mut profile.height)
                        .range(1..=8192)
                        .prefix("Height "),
                );
            });
        }
        ui.checkbox(&mut profile.multi_monitor, "Two side-by-side remote monitors");
        ui.label("Uses the selected resolution as a combined display; server support is required.");
        let mut layout = profile.keyboard_layout;
        choice(
            ui,
            "Keyboard layout",
            &mut layout,
            &[
                (None, "Auto"),
                (Some(0x0409), "English (US)"),
                (Some(0x0809), "English (UK)"),
                (Some(0x0407), "German"),
                (Some(0x040c), "French"),
            ],
        );
        profile.keyboard_layout = layout;
        choice(
            ui,
            "Audio output",
            &mut profile.audio,
            &[(Audio::Local, "Play locally"), (Audio::Off, "Off")],
        );
        ui.checkbox(&mut profile.mic, "Share microphone");
        clipboard(ui, &mut profile.clipboard);
        ui.collapsing("Drive redirection", |ui| {
            let mut remove = None;
            for (index, drive) in profile.drives.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.label(drive.path.display().to_string());
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut drive.writable, "Allow remote writes");
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                profile.drives.remove(index);
            }
            if ui.button("Add folder").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    if !profile.drives.iter().any(|drive| drive.path == path) {
                        profile.drives.push(DriveShare {
                            path,
                            writable: false,
                        });
                    }
                }
            }
        });
        ui.collapsing("Experience", |ui| {
            ui.checkbox(&mut profile.wallpaper, "Wallpaper");
            ui.checkbox(&mut profile.font_smoothing, "Font smoothing");
            ui.checkbox(&mut profile.animations, "Animations");
            ui.checkbox(&mut profile.themes, "Themes");
        });
        ui.checkbox(&mut profile.admin_session, "Administrative session");
        text(ui, "RD Gateway host", &mut profile.gateway);
        ui.checkbox(&mut profile.tunnel_via_ssh, "Tunnel via SSH");
        ui.checkbox(&mut profile.auto_reconnect, "Auto-reconnect");
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

/// An optional display host must match the General tab. The profile stores only the port.
pub(crate) fn vnc_port(value: &str, host: &str) -> Option<u16> {
    let value = value.trim();
    if let Ok(port) = value.parse::<u16>() {
        return (port != 0).then_some(port);
    }
    let (prefix, display) = value.rsplit_once(':')?;
    let unbracket = |s: &str| s.trim_matches(['[', ']']).to_ascii_lowercase();
    if !prefix.is_empty() && unbracket(prefix) != unbracket(host.trim()) {
        return None;
    }
    if prefix.contains(':') && !(prefix.starts_with('[') && prefix.ends_with(']')) {
        return None;
    }
    5900u16.checked_add(display.parse().ok()?)
}

pub(crate) fn vnc(ui: &mut Ui, draft: &mut Option<VncProfile>, port_text: &mut String, host: &str) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.checkbox(&mut profile.enabled, "Enable VNC");
    ui.add_enabled_ui(profile.enabled, |ui| {
        text(ui, "Port or display *", port_text);
        if let Some(port) = vnc_port(port_text, host) {
            profile.port = port;
        }
        text(
            ui,
            "Username (only if required by server)",
            &mut profile.username,
        );
        choice(
            ui,
            "Quality",
            &mut profile.quality,
            &[
                (Quality::Auto, "Auto"),
                (Quality::Best, "Best"),
                (Quality::Good, "Good"),
                (Quality::Fast, "Fast"),
            ],
        );
        ui.collapsing("Encoding priority", |ui| {
            for i in 0..profile.encodings.len() {
                let encoding = profile.encodings[i];
                let label = match encoding {
                    Encoding::Tight => "Tight (unsupported; skipped)",
                    Encoding::Zrle => "ZRLE",
                    Encoding::CopyRect => "CopyRect",
                    Encoding::Raw => "Raw",
                };
                let move_up = ui
                    .push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add_enabled(i > 0, egui::Button::new("Move up"))
                                .clicked()
                        })
                        .inner
                    })
                    .inner;
                if move_up {
                    profile.encodings.swap(i, i - 1);
                    break;
                }
            }
        });
        color_depth(ui, &mut profile.color_depth);
        ui.checkbox(&mut profile.view_only, "View only");
        ui.checkbox(&mut profile.shared, "Shared session");
        clipboard(ui, &mut profile.clipboard);
        ui.label("Classic VNC shares Latin-1 text. Click the remote desktop before pasting.");
        choice(
            ui,
            "Scaling",
            &mut profile.scaling,
            &[
                (Scaling::Fit, "Fit"),
                (Scaling::Native, "100%"),
                (Scaling::Fill, "Fill"),
            ],
        );
        ui.checkbox(&mut profile.tunnel_via_ssh, "Tunnel via SSH");
        if !profile.tunnel_via_ssh {
            ui.label("Classic VNC is unencrypted. Use an SSH tunnel on untrusted networks.");
        }
        ui.checkbox(&mut profile.auto_reconnect, "Auto-reconnect");
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

#[cfg(test)]
mod tests {
    use super::vnc_port;

    #[test]
    fn accepts_ports_and_displays_for_the_configured_host() {
        assert_eq!(vnc_port("5901", "example.invalid"), Some(5901));
        assert_eq!(vnc_port(":1", "example.invalid"), Some(5901));
        assert_eq!(vnc_port("EXAMPLE.invalid:2", "example.invalid"), Some(5902));
        assert_eq!(vnc_port("[::1]:3", "::1"), Some(5903));
        assert_eq!(vnc_port("::1", "::1"), None);
        assert_eq!(vnc_port("other.invalid:1", "example.invalid"), None);
        assert_eq!(vnc_port("0", "::1"), None);
        assert_eq!(vnc_port(":65535", "::1"), None);
    }
}

#[derive(Default)]
pub(crate) struct PasswordDraft {
    pub value: zeroize::Zeroizing<String>,
    reveal: bool,
}

impl PasswordDraft {
    pub(crate) fn show(&mut self, ui: &mut Ui, reference: &mut Option<String>) {
        ui.horizontal(|ui| {
            ui.label("Password");
            let toggle = if self.reveal { "Hide" } else { "Show" };
            if ui.small_button(toggle).clicked() {
                self.reveal = !self.reveal;
            }
            if reference.is_some() {
                ui.label("Saved in system credential store");
                if ui.small_button("Forget saved password").clicked() {
                    *reference = None;
                    self.value = Default::default();
                }
            }
        });
        let hint = if reference.is_some() {
            "Enter a replacement password"
        } else {
            "Enter password"
        };
        let mut output = TextEdit::singleline(&mut *self.value)
            .password(!self.reveal)
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .show(ui);
        // egui keeps an undo copy even in password mode.
        output.state.clear_undoer();
        output.state.store(ui.ctx(), output.response.id);
    }
}
