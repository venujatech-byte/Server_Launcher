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

fn rdp_color_profile(ui: &mut Ui, value: &mut Option<RdpColorProfile>) {
    const SUPPORTED: &[(RdpColorProfile, &str)] = &[
        (RdpColorProfile::Automatic, "Automatic (32 bpp)"),
        (RdpColorProfile::GfxRfx, "GFX RFX (32 bpp)"),
        (
            RdpColorProfile::GfxRfxProgressive,
            "GFX RFX Progressive (32 bpp)",
        ),
        (RdpColorProfile::RemoteFx, "RemoteFX (32 bpp)"),
        (RdpColorProfile::TrueColor32, "True color (32 bpp)"),
        (RdpColorProfile::TrueColor24, "True color (24 bpp)"),
        (RdpColorProfile::HighColor16, "High color (16 bpp)"),
        (RdpColorProfile::HighColor15, "High color (15 bpp)"),
    ];
    let selected = value.unwrap_or(RdpColorProfile::Automatic);
    ui.horizontal(|ui| {
        ui.label("Color profile");
        ComboBox::from_id_salt("RDP color profile")
            .selected_text(
                SUPPORTED
                    .iter()
                    .find(|(profile, _)| *profile == selected)
                    .map_or("Automatic (32 bpp)", |(_, label)| *label),
            )
            .show_ui(ui, |ui| {
                for &(profile, label) in SUPPORTED {
                    ui.selectable_value(value, Some(profile), label);
                }
                ui.separator();
                ui.add_enabled(false, egui::Button::new("GFX AVC444 (32 bpp)"));
                ui.add_enabled(false, egui::Button::new("GFX AVC420 (32 bpp)"));
                ui.add_enabled(false, egui::Button::new("256 colors (8 bpp)"));
            });
    });
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
                (RdpSecurity::Auto, "Auto (NLA only)"),
                (RdpSecurity::Nla, "NLA"),
                (RdpSecurity::Tls, "TLS"),
            ],
        );
        ui.label("Auto requires NLA. Choose TLS explicitly for servers without NLA.");
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
            "Frame update mode",
            &mut profile.frame_painting,
            &[
                (RdpFramePainting::Incremental, "Incremental repaint (recommended)"),
                (RdpFramePainting::FullFrame, "Full-frame repaint"),
            ],
        );
        ui.label(
            "Full-frame repaint redraws the complete received image after each update. It changes client-side painting only and may use more CPU; it does not change network or server rendering behavior.",
        );
        rdp_color_profile(ui, &mut profile.color_profile);
        ui.label("GFX RFX uses the graphics pipeline; AVC/H.264 and 8 bpp remain unavailable.");
        ui.label("RemoteFX is advertised to the server; the server still chooses the negotiated codec.");
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
        ui.collapsing("Input", |ui| {
            ui.checkbox(&mut profile.left_handed_mouse, "Left-handed mouse support");
            ui.checkbox(&mut profile.disable_smooth_scrolling, "Disable smooth scrolling");
        });
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
        ui.add_enabled_ui(false, |ui| {
            choice(
                ui,
                "Audio output",
                &mut profile.audio,
                &[(Audio::Local, "Play locally"), (Audio::Off, "Off")],
            );
            ui.checkbox(&mut profile.mic, "Share microphone");
        });
        ui.label("RDP audio playback and microphone redirection are not supported yet.");
        clipboard(ui, &mut profile.clipboard);
        ui.collapsing("Drive redirection (unsupported)", |ui| {
            ui.label("RDP drive sharing is not supported yet. Saved entries are preserved:");
            if profile.drives.is_empty() {
                ui.label("No folders configured.");
            }
            for drive in &profile.drives {
                ui.label(format!(
                    "{} · {}",
                    drive.path.display(),
                    if drive.writable { "read/write" } else { "read-only" }
                ));
            }
        });
        ui.collapsing("Experience", |ui| {
            ui.checkbox(&mut profile.wallpaper, "Wallpaper");
            ui.checkbox(&mut profile.font_smoothing, "Font smoothing");
            ui.checkbox(&mut profile.animations, "Animations");
            ui.checkbox(&mut profile.themes, "Themes");
        });
        ui.add_enabled(false, egui::Checkbox::new(&mut profile.admin_session, "Administrative session"));
        ui.label("Administrative sessions are not supported by this RDP client.");
        ui.add_enabled_ui(false, |ui| text(ui, "RD Gateway host", &mut profile.gateway));
        ui.label("RD Gateway connections are not supported by this RDP client.");
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
        ui.add_enabled_ui(false, |ui| {
            text(
                ui,
                "Username (not supported by this client)",
                &mut profile.username,
            );
        });
        ui.label("Only classic VNC password authentication is supported.");
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
        ui.collapsing("VNC encoding order", |ui| {
            ui.label("Order is sent to the server; unsupported encodings are skipped.");
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
        ui.checkbox(&mut profile.show_remote_cursor, "Show remote cursor");
        ui.checkbox(
            &mut profile.disable_smooth_scrolling,
            "Disable smooth scrolling",
        );
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
        #[cfg(target_os = "linux")]
        ui.horizontal(|ui| {
            ui.label("TCP user timeout (seconds)");
            ui.add(DragValue::new(&mut profile.tcp_user_timeout_secs).range(0..=2_147_483));
        });
        #[cfg(target_os = "linux")]
        ui.label(if profile.tunnel_via_ssh {
            "Applies to direct Linux TCP connections only; ignored while SSH tunneling is enabled."
        } else {
            "0 uses the operating system default. This is not an idle timeout."
        });
        #[cfg(not(target_os = "linux"))]
        ui.label("TCP user timeout is available for direct Linux connections only.");
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
