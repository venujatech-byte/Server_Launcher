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

fn confirm_drive_access(path: &std::path::Path, writable: bool) -> bool {
    let access = if writable {
        "The remote PC will be able to read and change files in this folder."
    } else {
        "The remote PC will be able to read files in this folder."
    };
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("Share a local folder with the remote PC?")
        .set_description(format!("{}\n\n{}", path.display(), access))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

fn rdp_color_profile(ui: &mut Ui, value: &mut Option<RdpColorProfile>) {
    const SUPPORTED: &[(RdpColorProfile, &str)] = &[
        (RdpColorProfile::Automatic, "Automatic (32 bpp)"),
        (RdpColorProfile::GfxRfx, "GFX RFX (32 bpp)"),
        (
            RdpColorProfile::GfxRfxProgressive,
            "GFX RFX Progressive (32 bpp)",
        ),
        (RdpColorProfile::GfxAvc444, "GFX AVC444 (32 bpp)"),
        (RdpColorProfile::GfxAvc420, "GFX AVC420 (32 bpp)"),
        (RdpColorProfile::RemoteFx, "RemoteFX (32 bpp)"),
        (RdpColorProfile::TrueColor32, "True color (32 bpp)"),
        (RdpColorProfile::TrueColor24, "True color (24 bpp)"),
        (RdpColorProfile::HighColor16, "High color (16 bpp)"),
        (RdpColorProfile::HighColor15, "High color (15 bpp)"),
        (RdpColorProfile::Color256, "256 colors (8 bpp)"),
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
            });
    });
    ui.label(
        "AVC profiles require a Cisco OpenH264 library selected in Settings → Remote Desktop.",
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
                (RdpSecurity::Auto, "Auto (NLA or TLS)"),
                (RdpSecurity::Nla, "NLA"),
                (RdpSecurity::Tls, "TLS"),
            ],
        );
        ui.label("Auto allows NLA or encrypted TLS. Unencrypted legacy RDP is disabled.");
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
        choice(
            ui,
            "Audio output",
            &mut profile.audio,
            &[(Audio::Local, "Play locally"), (Audio::Off, "Off")],
        );
        ui.checkbox(&mut profile.mic, "Share microphone");
        if profile.mic {
            ui.label("The remote PC can receive audio from your selected microphone while connected.");
        }
        clipboard(ui, &mut profile.clipboard);
        ui.collapsing("Drive redirection", |ui| {
            ui.label("Shared folders are visible to the remote PC for this session.");
            if ui.button("Add folder…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    if confirm_drive_access(&path, false) {
                        profile.drives.push(DriveShare {
                            path,
                            writable: false,
                        });
                    }
                }
            }
            if profile.drives.is_empty() {
                ui.label("No folders configured.");
            }
            let mut remove = None;
            for (index, drive) in profile.drives.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(drive.path.display().to_string());
                    let mut writable = drive.writable;
                    if ui.checkbox(&mut writable, "Allow remote changes").changed() {
                        drive.writable = if writable {
                            confirm_drive_access(&drive.path, true)
                        } else {
                            false
                        };
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                profile.drives.remove(index);
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
        text(ui, "RD Gateway host (optional)", &mut profile.gateway);
        ui.label("Uses the RDP username and password for gateway sign-in. Gateway TLS certificates must be system-trusted.");
        ui.add_enabled_ui(profile.gateway.trim().is_empty(), |ui| {
            ui.checkbox(&mut profile.tunnel_via_ssh, "Tunnel via SSH");
        });
        if !profile.gateway.trim().is_empty() {
            if profile.tunnel_via_ssh {
                ui.label("RD Gateway and SSH tunneling cannot be combined. Disable the saved SSH tunnel to connect.");
                if ui.button("Disable SSH tunnel").clicked() {
                    profile.tunnel_via_ssh = false;
                }
            } else {
                ui.label("RD Gateway provides the network tunnel; SSH tunneling is not needed.");
            }
        }
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

fn new_vnc_profile(host: &str) -> VncProfile {
    VncProfile {
        tunnel_via_ssh: super::vnc::needs_unencrypted_warning(host),
        ..VncProfile::default()
    }
}

pub(crate) fn vnc(ui: &mut Ui, draft: &mut Option<VncProfile>, port_text: &mut String, host: &str) {
    let mut profile = draft.clone().unwrap_or_else(|| new_vnc_profile(host));
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
        choice(ui, "Quality", &mut profile.quality, &[
            (Quality::Auto, "Auto"),
            (Quality::Best, "Best"),
            (Quality::Good, "Good"),
            (Quality::Fast, "Fast"),
        ]);
        color_depth(ui, &mut profile.color_depth);
        clipboard(ui, &mut profile.clipboard);

        ui.collapsing("Advanced", |ui| {
            ui.label("Encoding priority (first supported encoding is preferred)");
            for i in 0..profile.encodings.len() {
                let encoding = profile.encodings[i];
                let label = match encoding {
                    Encoding::Tight => "Tight",
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
            ui.checkbox(
                &mut profile.disable_smooth_scrolling,
                "Disable smooth scrolling",
            );
        #[cfg(target_os = "linux")]
            ui.horizontal(|ui| {
                ui.label("TCP user timeout (seconds)");
                ui.add(DragValue::new(&mut profile.tcp_user_timeout_secs).range(0..=2_147_483));
            });
        #[cfg(target_os = "linux")]
            ui.label("0 uses the operating system default; this is not an idle timeout.");
        #[cfg(not(target_os = "linux"))]
            ui.label("TCP user timeout is available for direct Linux connections only.");
        });

        ui.collapsing("Behavior", |ui| {
            ui.checkbox(&mut profile.view_only, "View only");
            ui.checkbox(&mut profile.shared, "Shared session");
            ui.checkbox(&mut profile.show_remote_cursor, "Show remote cursor");
            ui.checkbox(&mut profile.auto_reconnect, "Auto-reconnect");
            ui.checkbox(&mut profile.close_on_failure, "Close on connection failure");
            choice(ui, "Scaling", &mut profile.scaling, &[
                (Scaling::Fit, "Fit"),
                (Scaling::Native, "100%"),
                (Scaling::Fill, "Fill"),
            ]);
        });

        ui.collapsing("SSH Tunnel", |ui| {
            ui.checkbox(&mut profile.tunnel_via_ssh, "Tunnel via SSH");
            if profile.tunnel_via_ssh {
                ui.label("The VNC connection is carried through this host's SSH connection.");
            } else {
                ui.label("Classic VNC is unencrypted. Use an SSH tunnel on untrusted networks.");
            }
            #[cfg(target_os = "linux")]
            ui.label("TCP user timeout only applies to direct connections, not SSH tunnels.");
        });

        ui.collapsing("Notes", |ui| {
            ui.label("Classic VNC clipboard supports Latin-1 text only. Click the remote desktop before pasting.");
            ui.label("Server bell messages are ignored. Dynamic resolution depends on server support.");
        });
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

#[cfg(test)]
mod tests {
    use super::{new_vnc_profile, vnc_port};

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

    #[test]
    fn new_vnc_profiles_tunnel_only_for_public_or_unknown_hosts() {
        assert!(!new_vnc_profile("192.168.1.12").tunnel_via_ssh);
        assert!(new_vnc_profile("100.94.20.30").tunnel_via_ssh);
        assert!(new_vnc_profile("vnc.example.test").tunnel_via_ssh);
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
