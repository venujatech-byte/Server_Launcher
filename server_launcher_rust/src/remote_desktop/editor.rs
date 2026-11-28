use super::profile::*;
use super::ui_kit as kit;
use crate::theme::ThemePalette;
use egui::{Response, RichText, Ui};

pub(crate) fn new_rdp_profile(defaults: &crate::config::RemoteDesktopSettings) -> RdpProfile {
    RdpProfile {
        quality: defaults.default_quality,
        clipboard: defaults.default_clipboard,
        scaling: defaults.default_scaling,
        auto_reconnect: defaults.default_auto_reconnect,
        ..RdpProfile::default()
    }
}

fn new_vnc_profile(host: &str, defaults: &crate::config::RemoteDesktopSettings) -> VncProfile {
    VncProfile {
        quality: defaults.default_quality,
        clipboard: defaults.default_clipboard,
        scaling: defaults.default_scaling,
        auto_reconnect: defaults.default_auto_reconnect,
        tunnel_via_ssh: super::vnc::needs_unencrypted_warning(host),
        ..VncProfile::default()
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

fn clipboard_items() -> &'static [(Clipboard, &'static str)] {
    &[
        (Clipboard::Both, "Both directions"),
        (Clipboard::RemoteToLocal, "Remote to local"),
        (Clipboard::LocalToRemote, "Local to remote"),
        (Clipboard::Off, "Off"),
    ]
}

fn color_depth_items() -> &'static [(u8, &'static str)] {
    &[(16, "16 bit"), (24, "24 bit"), (32, "32 bit")]
}

fn quality_items() -> &'static [(Quality, &'static str)] {
    &[
        (Quality::Auto, "Auto"),
        (Quality::Best, "Best"),
        (Quality::Good, "Good"),
        (Quality::Fast, "Fast"),
    ]
}

fn scaling_items() -> &'static [(Scaling, &'static str)] {
    &[
        (Scaling::Fit, "Fit"),
        (Scaling::Native, "100%"),
        (Scaling::Fill, "Fill"),
    ]
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

pub(crate) fn rdp_enable_switch(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) -> Response {
    let mut profile = draft.clone().unwrap_or_else(|| new_rdp_profile(defaults));
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(120.0, 29.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| kit::switch(ui, &mut profile.enabled, "Enable RDP", None, palette),
        )
        .inner;
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
    response
}

pub(crate) fn vnc_enable_switch(
    ui: &mut Ui,
    draft: &mut Option<VncProfile>,
    host: &str,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) -> Response {
    let mut profile = draft
        .clone()
        .unwrap_or_else(|| new_vnc_profile(host, defaults));
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(120.0, 29.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| kit::switch(ui, &mut profile.enabled, "Enable VNC", None, palette),
        )
        .inner;
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
    response
}

pub(crate) fn rdp_connection(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_else(|| new_rdp_profile(defaults));
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::two_fields(
            ui,
            |ui| {
                let response = kit::drag_field(
                    ui,
                    palette,
                    "Port",
                    &mut profile.port,
                    1..=u16::MAX,
                    "",
                    true,
                    profile.port_auto.then_some("auto"),
                );
                if response.changed() {
                    profile.port_auto = false;
                    profile.port_manual = Some(true);
                }
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Security",
                    &mut profile.security,
                    &[
                        (RdpSecurity::Auto, "Auto (NLA or TLS)"),
                        (RdpSecurity::Nla, "NLA"),
                        (RdpSecurity::Tls, "TLS"),
                    ],
                    Some("Auto negotiates NLA or TLS. Unencrypted legacy RDP is disabled."),
                );
            },
        );

        kit::field_label(ui, "RD Gateway host", false, palette);
        kit::text_field(ui, &mut profile.gateway, "Optional", false, palette);
        kit::hint(
            ui,
            "Uses the RDP username and password for gateway sign-in. Gateway TLS certificates must be system-trusted.",
        );

        kit::two_fields(
            ui,
            |ui| {
                ui.add_enabled_ui(profile.gateway.trim().is_empty(), |ui| {
                    kit::switch(
                        ui,
                        &mut profile.tunnel_via_ssh,
                        "Tunnel via SSH",
                        None,
                        palette,
                    )
                })
            },
            |ui| {
                kit::switch(
                    ui,
                    &mut profile.auto_reconnect,
                    "Auto-reconnect",
                    None,
                    palette,
                )
            },
        );

        if !profile.gateway.trim().is_empty() && profile.tunnel_via_ssh {
            ui.add_space(10.0);
            kit::status_line(
                ui,
                "RD Gateway and SSH tunneling cannot be combined. Disable the saved SSH tunnel to connect.",
            );
            if kit::compact_button(ui, "Disable SSH tunnel", palette).clicked() {
                profile.tunnel_via_ssh = false;
            }
        } else if !profile.gateway.trim().is_empty() {
            ui.add_space(10.0);
            kit::status_line(
                ui,
                "RD Gateway provides the network tunnel; SSH tunneling is not needed.",
            );
        }
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn rdp_credentials(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    password: &mut PasswordDraft,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::switch(
            ui,
            &mut profile.same_username_as_ssh,
            "Same username as SSH",
            None,
            palette,
        );
        ui.add_space(12.0);
        kit::two_fields(
            ui,
            |ui| {
                kit::field_label(ui, "Username", true, palette);
                ui.add_enabled_ui(!profile.same_username_as_ssh, |ui| {
                    kit::text_field_with_disabled_style(
                        ui,
                        &mut profile.username,
                        "ubuntu",
                        false,
                        palette,
                        kit::DISABLED,
                        kit::DISABLED_TEXT,
                    )
                })
            },
            |ui| {
                kit::field_label(ui, "Domain", false, palette);
                kit::text_field(ui, &mut profile.domain, "Optional", false, palette)
            },
        );
        if profile.enabled {
            ui.add_space(12.0);
            password.show(ui, &mut profile.credential_ref, palette);
        }
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn rdp_display(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_else(|| new_rdp_profile(defaults));
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Display painting",
                    &mut profile.painting,
                    &[
                        (RdpPainting::Smooth, "Smooth"),
                        (RdpPainting::Fast, "Fast / crisp"),
                    ],
                    Some("Smooth filtering blends scaled pixels; Fast uses sharper nearest-neighbor sampling."),
                )
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Texture upload",
                    &mut profile.frame_painting,
                    &[
                        (RdpFramePainting::Incremental, "Changed regions (recommended)"),
                        (RdpFramePainting::FullFrame, "Full-frame compatibility mode"),
                    ],
                    Some("Full-frame mode uploads the complete local image after each update. It can use more CPU and memory; it does not change remote rendering, codec, or network behavior."),
                )
            },
        );

        ui.add_space(12.0);
        let selected = profile.color_profile.unwrap_or(RdpColorProfile::Automatic);
        let supported = [
            (RdpColorProfile::Automatic, "Automatic (32 bpp)"),
            (RdpColorProfile::GfxRfx, "GFX RFX (32 bpp)"),
            (RdpColorProfile::GfxRfxProgressive, "GFX RFX Progressive (32 bpp)"),
            (RdpColorProfile::GfxAvc444, "GFX AVC444 (32 bpp)"),
            (RdpColorProfile::GfxAvc420, "GFX AVC420 (32 bpp)"),
            (RdpColorProfile::RemoteFx, "RemoteFX (32 bpp)"),
            (RdpColorProfile::TrueColor32, "True color (32 bpp)"),
            (RdpColorProfile::TrueColor24, "True color (24 bpp)"),
            (RdpColorProfile::HighColor16, "High color (16 bpp)"),
            (RdpColorProfile::HighColor15, "High color (15 bpp)"),
            (RdpColorProfile::Color256, "256 colors (8 bpp)"),
        ];
        let items: Vec<(Option<RdpColorProfile>, &str)> = supported
            .iter()
            .map(|(profile, label)| (Some(*profile), *label))
            .collect();
        let mut value = Some(selected);
        kit::select_field(
            ui,
            palette,
            "Color profile",
            &mut value,
            &items,
            Some("AVC profiles require a Cisco OpenH264 library selected in Settings → Remote Desktop."),
        );
        profile.color_profile = value;
        kit::hint(
            ui,
            "RemoteFX is advertised to the server; the server still chooses the negotiated codec.",
        );

        ui.add_space(12.0);
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Initial scaling",
                    &mut profile.scaling,
                    scaling_items(),
                    None,
                )
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Resolution",
                    &mut profile.resolution,
                    &[
                        (Resolution::FitWindow, "Fit window (dynamic)"),
                        (Resolution::Fixed, "Fixed size"),
                        (Resolution::MatchLocalScreen, "Match local screen"),
                    ],
                    None,
                )
            },
        );
        if profile.resolution == Resolution::Fixed {
            ui.add_space(10.0);
            kit::two_fields(
                ui,
                |ui| {
                    kit::drag_field(
                        ui,
                        palette,
                        "Width",
                        &mut profile.width,
                        1..=8192,
                        "",
                        false,
                        None,
                    )
                },
                |ui| {
                    kit::drag_field(
                        ui,
                        palette,
                        "Height",
                        &mut profile.height,
                        1..=8192,
                        "",
                        false,
                        None,
                    )
                },
            );
        }

        ui.add_space(12.0);
        kit::switch(
            ui,
            &mut profile.multi_monitor,
            "Two side-by-side remote monitors",
            Some("Uses the selected resolution as a combined display; server support is required."),
            palette,
        );
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn rdp_input(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.add_enabled_ui(profile.enabled, |ui| {
        let mut layout = profile.keyboard_layout;
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Keyboard layout",
                    &mut layout,
                    &[
                        (None, "Auto"),
                        (Some(0x0409), "English (US)"),
                        (Some(0x0809), "English (UK)"),
                        (Some(0x0407), "German"),
                        (Some(0x040c), "French"),
                    ],
                    None,
                )
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Clipboard",
                    &mut profile.clipboard,
                    clipboard_items(),
                    None,
                )
            },
        );
        profile.keyboard_layout = layout;

        ui.add_space(12.0);
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Audio output",
                    &mut profile.audio,
                    &[(Audio::Local, "Play locally"), (Audio::Off, "Off")],
                    None,
                )
            },
            |ui| {
                ui.add_space(25.5);
                let mic_hint = profile.mic.then_some(
                    "The remote PC can receive audio from your selected microphone while connected.",
                );
                kit::switch(
                    ui,
                    &mut profile.mic,
                    "Share microphone",
                    mic_hint,
                    palette,
                )
            },
        );

        ui.add_space(12.0);
        kit::section(ui, "Input options", |ui| {
            ui.add_space(4.0);
            kit::two_fields(
                ui,
                |ui| {
                    kit::switch(
                        ui,
                        &mut profile.view_only,
                        "Start in view-only mode",
                        None,
                        palette,
                    )
                },
                |ui| {
                    kit::switch(
                        ui,
                        &mut profile.left_handed_mouse,
                        "Left-handed mouse support",
                        None,
                        palette,
                    )
                },
            );
            ui.add_space(10.0);
            kit::switch(
                ui,
                &mut profile.disable_smooth_scrolling,
                "Disable smooth scrolling",
                None,
                palette,
            );
        });
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn rdp_devices(
    ui: &mut Ui,
    draft: &mut Option<RdpProfile>,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::section(ui, "Drive redirection", |ui| {
            ui.add_space(4.0);
            kit::status_line(ui, "Shared folders are visible to the remote PC for this session.");
            if kit::compact_button(ui, "Add folder…", palette).clicked() {
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
                kit::status_line(ui, "No folders configured.");
            }
            let mut remove = None;
            for (index, drive) in profile.drives.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(drive.path.display().to_string());
                    let mut writable = drive.writable;
                    if kit::switch(
                        ui,
                        &mut writable,
                        "Allow remote changes",
                        None,
                        palette,
                    )
                    .changed()
                    {
                        drive.writable = if writable {
                            confirm_drive_access(&drive.path, true)
                        } else {
                            false
                        };
                    }
                    if kit::compact_button(ui, "Remove", palette).clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                profile.drives.remove(index);
            }
        });

        ui.add_space(10.0);
        kit::section(ui, "Experience", |ui| {
            ui.add_space(4.0);
            kit::two_fields(
                ui,
                |ui| kit::switch(ui, &mut profile.wallpaper, "Wallpaper", None, palette),
                |ui| {
                    kit::switch(ui, &mut profile.font_smoothing, "Font smoothing", None, palette)
                },
            );
            ui.add_space(10.0);
            kit::two_fields(
                ui,
                |ui| kit::switch(ui, &mut profile.animations, "Animations", None, palette),
                |ui| kit::switch(ui, &mut profile.themes, "Themes", None, palette),
            );
        });

        ui.add_space(12.0);
        kit::switch(
            ui,
            &mut profile.admin_session,
            "Administrative session",
            Some("Requests the server's administrative session; server policy and permissions apply."),
            palette,
        );
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_connection(
    ui: &mut Ui,
    draft: &mut Option<VncProfile>,
    port_text: &mut String,
    host: &str,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) {
    let mut profile = draft
        .clone()
        .unwrap_or_else(|| new_vnc_profile(host, defaults));
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::two_fields(
            ui,
            |ui| {
                kit::field_label(ui, "Port or display", true, palette);
                if kit::text_field(ui, port_text, "5900", false, palette).changed() {
                    profile.port_auto = false;
                    profile.port_manual = Some(true);
                }
                if profile.port_auto {
                    kit::hint(ui, "Port assigned from the detected VNC display.");
                }
            },
            |ui| {
                kit::field_label(ui, "Username", false, palette);
                kit::text_field(ui, &mut profile.username, "Optional", false, palette);
                kit::hint(
                    ui,
                    &format!(
                        "{} Username/password auth requires VeNCrypt Plain through an SSH tunnel; direct plaintext auth is blocked.",
                        crate::lucide::SHIELD
                    ),
                );
            },
        );

        ui.add_space(12.0);
        kit::switch(
            ui,
            &mut profile.tunnel_via_ssh,
            "Tunnel via SSH",
            Some("The VNC connection is carried through this host's SSH connection."),
            palette,
        );
        if !profile.tunnel_via_ssh {
            ui.add_space(6.0);
            kit::status_line(
                ui,
                "Classic VNC is unencrypted. Use an SSH tunnel on untrusted networks.",
            );
        }
        if let Some(port) = vnc_port(port_text, host) {
            profile.port = port;
        }
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_credentials(
    ui: &mut Ui,
    draft: &mut Option<VncProfile>,
    password: &mut PasswordDraft,
    palette: &ThemePalette,
) {
    let mut profile = draft.clone().unwrap_or_default();
    if profile.enabled {
        password.show(ui, &mut profile.credential_ref, palette);
        ui.add_space(12.0);
        if profile.username.trim().is_empty() {
            kit::status_line(ui, "Classic VNC passwords are limited to eight bytes.");
        } else if !profile.tunnel_via_ssh {
            ui.label(
                egui::RichText::new(
                    "Enable Tunnel via SSH before using username/password authentication.",
                )
                .size(12.0)
                .color(kit::DANGER),
            );
        }
    }
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_display(
    ui: &mut Ui,
    draft: &mut Option<VncProfile>,
    defaults: &crate::config::RemoteDesktopSettings,
    palette: &ThemePalette,
) {
    let mut profile = draft
        .clone()
        .unwrap_or_else(|| new_vnc_profile("", defaults));
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Quality",
                    &mut profile.quality,
                    quality_items(),
                    None,
                )
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Color depth",
                    &mut profile.color_depth,
                    color_depth_items(),
                    None,
                )
            },
        );
        ui.add_space(12.0);
        kit::two_fields(
            ui,
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Clipboard",
                    &mut profile.clipboard,
                    clipboard_items(),
                    None,
                )
            },
            |ui| {
                kit::select_field(
                    ui,
                    palette,
                    "Scaling",
                    &mut profile.scaling,
                    scaling_items(),
                    None,
                )
            },
        );
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_behavior(ui: &mut Ui, draft: &mut Option<VncProfile>, palette: &ThemePalette) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::two_fields(
            ui,
            |ui| kit::switch(ui, &mut profile.view_only, "View only", None, palette),
            |ui| {
                kit::switch(
                    ui,
                    &mut profile.auto_reconnect,
                    "Auto-reconnect",
                    None,
                    palette,
                )
            },
        );
        ui.add_space(10.0);
        kit::two_fields(
            ui,
            |ui| kit::switch(ui, &mut profile.shared, "Shared session", None, palette),
            |ui| {
                kit::switch(
                    ui,
                    &mut profile.close_on_failure,
                    "Close on connection failure",
                    None,
                    palette,
                )
            },
        );
        ui.add_space(10.0);
        kit::two_fields(
            ui,
            |ui| {
                kit::switch(
                    ui,
                    &mut profile.show_remote_cursor,
                    "Show remote cursor",
                    None,
                    palette,
                )
            },
            |ui| {
                kit::switch(
                    ui,
                    &mut profile.disable_smooth_scrolling,
                    "Disable smooth scrolling",
                    None,
                    palette,
                )
            },
        );
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_advanced(ui: &mut Ui, draft: &mut Option<VncProfile>, palette: &ThemePalette) {
    let mut profile = draft.clone().unwrap_or_default();
    ui.add_enabled_ui(profile.enabled, |ui| {
        kit::hint(
            ui,
            "Encoding priority (first supported encoding is preferred)",
        );
        for index in 0..profile.encodings.len() {
            let encoding = profile.encodings[index];
            let label = match encoding {
                Encoding::Tight => "Tight",
                Encoding::Zrle => "ZRLE",
                Encoding::CopyRect => "CopyRect",
                Encoding::Raw => "Raw",
            };
            let enabled = index > 0 && ui.is_enabled();
            let row_fill = if ui.is_enabled() {
                kit::WINDOW
            } else {
                kit::WINDOW.linear_multiply(0.5)
            };
            let move_up = ui
                .push_id(index, |ui| {
                    egui::Frame::none()
                        .fill(row_fill)
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 3.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [ui.available_width() - 100.0, 30.0],
                                    egui::Label::new(format!("{}. {}", index + 1, label)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_enabled(
                                            index > 0,
                                            kit::compact_button_widget_enabled("Move up", enabled),
                                        )
                                        .clicked()
                                    },
                                )
                                .inner
                            })
                            .inner
                        })
                        .inner
                })
                .inner;
            if move_up {
                profile.encodings.swap(index, index - 1);
                break;
            }
        }

        ui.add_space(12.0);
        #[cfg(target_os = "linux")]
        kit::drag_field(
            ui,
            palette,
            "TCP user timeout (seconds)",
            &mut profile.tcp_user_timeout_secs,
            0..=2_147_483,
            "",
            false,
            None,
        );
        #[cfg(not(target_os = "linux"))]
        kit::status_line(
            ui,
            "TCP user timeout is available for direct Linux connections only.",
        );
        #[cfg(target_os = "linux")]
        kit::hint(
            ui,
            "0 uses the operating system default; this is not an idle timeout.",
        );
        #[cfg(target_os = "linux")]
        kit::status_line(
            ui,
            "TCP user timeout only applies to direct connections, not SSH tunnels.",
        );
    });
    if draft.is_some() || profile.enabled {
        *draft = Some(profile);
    }
}

pub(crate) fn vnc_notes(ui: &mut Ui, palette: &ThemePalette) {
    let _ = palette;
    kit::status_line(
        ui,
        "Classic VNC clipboard supports Latin-1 text only. Click the remote desktop before pasting.",
    );
    ui.add_space(6.0);
    kit::status_line(
        ui,
        "Server bell messages are ignored. Dynamic resolution depends on server support.",
    );
}

#[derive(Default)]
pub(crate) struct PasswordDraft {
    pub value: zeroize::Zeroizing<String>,
    reveal: bool,
}

impl PasswordDraft {
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        reference: &mut Option<String>,
        palette: &ThemePalette,
    ) {
        let compact = ui.available_width() < 440.0;
        let has_reference = reference.is_some();
        let mut show_buttons = |ui: &mut Ui| {
            if has_reference {
                if kit::destructive_button(ui, "Forget saved password", palette).clicked() {
                    *reference = None;
                    self.value = Default::default();
                }
            }
            let toggle = if self.reveal { "Hide" } else { "Show" };
            if kit::compact_button(ui, toggle, palette).clicked() {
                self.reveal = !self.reveal;
            }
        };
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Password").size(12.0).color(kit::LABEL));
                if has_reference {
                    ui.label(
                        egui::RichText::new(crate::lucide::CHECK)
                            .size(12.0)
                            .color(kit::SUCCESS),
                    );
                    let status_width = (ui.available_width() - 240.0).max(40.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(status_width, 29.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(status_width);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("Saved in system credential store")
                                        .size(11.0)
                                        .color(kit::TEXT),
                                )
                                .wrap(),
                            );
                        },
                    );
                }
                if !compact {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        show_buttons(ui);
                    });
                }
            });
            if compact {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    show_buttons(ui);
                });
            }
        });

        ui.add_space(4.0);
        let hint = if reference.is_some() {
            "Enter a replacement password"
        } else {
            "Enter password"
        };
        let response = kit::text_field(ui, &mut self.value, hint, !self.reveal, palette);
        // egui keeps an undo copy even in password mode.
        let mut state = egui::TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
        state.clear_undoer();
        egui::TextEdit::store_state(ui.ctx(), response.id, state);
    }
}

#[cfg(test)]
mod tests {
    use super::{new_rdp_profile, new_vnc_profile, vnc_port};
    use crate::{
        config::RemoteDesktopSettings,
        remote_desktop::profile::{Clipboard, Quality},
    };

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
        let defaults = RemoteDesktopSettings::default();
        assert!(!new_vnc_profile("192.168.1.12", &defaults).tunnel_via_ssh);
        assert!(new_vnc_profile("100.94.20.30", &defaults).tunnel_via_ssh);
        assert!(new_vnc_profile("vnc.example.test", &defaults).tunnel_via_ssh);
    }

    #[test]
    fn new_remote_profiles_use_global_quality_and_clipboard_defaults() {
        let defaults = RemoteDesktopSettings {
            default_quality: Quality::Fast,
            default_clipboard: Clipboard::RemoteToLocal,
            default_scaling: super::Scaling::Native,
            default_auto_reconnect: false,
            ..RemoteDesktopSettings::default()
        };
        let rdp = new_rdp_profile(&defaults);
        let vnc = new_vnc_profile("192.168.1.12", &defaults);

        assert_eq!(rdp.quality, Quality::Fast);
        assert_eq!(rdp.clipboard, Clipboard::RemoteToLocal);
        assert_eq!(rdp.scaling, super::Scaling::Native);
        assert!(!rdp.auto_reconnect);
        assert_eq!(vnc.quality, Quality::Fast);
        assert_eq!(vnc.clipboard, Clipboard::RemoteToLocal);
        assert_eq!(vnc.scaling, super::Scaling::Native);
        assert!(!vnc.auto_reconnect);
    }
}
