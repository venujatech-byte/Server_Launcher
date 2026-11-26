use super::*;

impl LauncherApp {
    pub(in crate::app) fn render_sudo_popup(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if !self.sudo_popup.open {
            return;
        }

        let unlock = matches!(self.sudo_popup.pending, Some(PendingSudoAction::UnlockSensorAccess));
        let title = if unlock { "Unlock hardware sensors" } else { "Administrator Authentication" };
        let mut is_open = self.sudo_popup.open;
        let mut submit = false;
        let mut cancel = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let busy = self.sudo_popup.pending.as_ref().is_some_and(|action| self.systemd_actions_in_flight.lock().unwrap().contains(&action.key(self.sudo_popup.host_id.as_deref())));
        let screen = ctx.screen_rect();
        let window_id = egui::Id::new("sudo_auth_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();
        egui::Window::new(title)
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_width(420.0_f32.min((screen.width() - 48.0).max(0.0)))
            .frame(
                egui::Frame::none()
                    .fill(palette.bg_card)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, title, &mut is_open);
                ui.add_space(8.0);
                ui.add_space(4.0);

                if let Some(pending) = &self.sudo_popup.pending {
                    match pending {
                        PendingSudoAction::UnlockSensorAccess => {
                            ui.label(RichText::new("Read protected hardware metrics").size(15.0).strong().color(palette.text_primary));
                            ui.label(RichText::new("CPU package power, Intel GPU counters, and RAM module details depend on hardware support.").size(12.0).color(palette.text_secondary));
                        }
                        PendingSudoAction::SetChargeLimit { battery, limit } => { ui.label(format!("Set battery {} charge limit to {}% on the selected host.", battery, limit)); }
                        PendingSudoAction::ServiceAction { unit, action, .. } => {
                            let act_color = match *action {
                                "start" => palette.success,
                                "stop" => palette.danger,
                                _ => palette.accent,
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Action:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(action.to_uppercase()).size(12.0).strong().color(act_color));
                                ui.label(RichText::new("on unit").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(unit).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::CreateService { def } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Create service:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(&def.name).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::SaveUnitFile { path, .. } | PendingSudoAction::CreateDropIn { path, .. } | PendingSudoAction::ReadUnitFile { path, .. } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Access unit file:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(path).size(12.0).strong().color(palette.text_primary));
                            });
                        }
                        PendingSudoAction::ReadJournal { unit, .. } | PendingSudoAction::QueryEnabled { unit, .. } | PendingSudoAction::InspectUnit { unit, .. } => {
                            ui.label(RichText::new(format!("Read protected service data: {}", unit)).size(12.0).color(palette.text_primary));
                        }
                        PendingSudoAction::DeleteDropIn { path, .. } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Delete system drop-in:").size(12.0).color(palette.text_secondary));
                                ui.label(RichText::new(path).size(12.0).strong().color(palette.danger));
                            });
                        }
                    }
                }

                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Administrator access is required on {}. Enter its sudo password:", self.sudo_popup.host_id.as_ref().and_then(|id| self.ssh_hosts.iter().find(|host| &host.id == id)).map(|host| host.name.as_str()).unwrap_or("this computer")))
                        .size(11.0)
                        .color(palette.text_muted),
                );

                ui.add_space(6.0);

                if let Some(err) = &self.sudo_popup.error {
                    Frame::none()
                        .fill(palette.bg_subtle)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(err).size(11.0).color(palette.danger));
                        });
                    ui.add_space(6.0);
                }

                ui.vertical(|ui| {
                    ui.label(RichText::new("Password").size(12.0).strong().color(palette.text_primary));
                    let edit = egui::TextEdit::singleline(&mut self.sudo_popup.password_input)
                        .password(true)
                        .hint_text("Enter sudo password")
                        .desired_width(ui.available_width());
                    let resp = ui.add_enabled(!busy, edit);
                    if self.sudo_popup.just_opened {
                        resp.request_focus();
                        self.sudo_popup.just_opened = false;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                });

                ui.add_space(6.0);
                ui.add_enabled(!busy, egui::Checkbox::new(&mut self.sudo_popup.remember_password, "Remember for this session"));

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("Cancel").size(11.5).color(palette.text_secondary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(4.0)),
                    ).clicked() {
                        cancel = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if busy { "Checking…" } else if unlock { "Unlock sensors" } else { "Authenticate" };
                        let btn_auth = egui::Button::new(RichText::new(label).size(11.5).strong().color(palette.accent_text))
                            .fill(palette.accent)
                            .rounding(Rounding::same(7.0))
                            .min_size(egui::vec2(0.0, 28.0));
                        if ui.add_enabled(!busy && !self.sudo_popup.password_input.is_empty(), btn_auth).clicked() {
                            submit = true;
                        }
                        if busy { ui.spinner(); }
                    });
                });
            });

        if cancel || !is_open {
            self.sudo_popup.open = false;
            self.sudo_popup.pending = None;
            self.sudo_popup.host_id = None;
            self.sudo_popup.error = None;
            self.sudo_popup.password_input.clear();
        } else if submit && !busy {
            if let Some(pending) = self.sudo_popup.pending.clone() {
                self.execute_pending_sudo_action(pending);
            }
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // SETTINGS MODAL: Themes, Boot Autostart, Server Import/Export, Shortcuts
    // ═════════════════════════════════════════════════════════════════════════
    pub(in crate::app) fn render_settings_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if !self.settings_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.settings_modal_open = false;
            return;
        }

        let mut is_open = self.settings_modal_open;
        let screen = ctx.screen_rect();
        let modal_w = 640.0_f32.min((screen.width() - 76.0).max(0.0));
        let max_h = (screen.height() - 72.0).max(0.0);

        let mut change_theme: Option<ThemeMode> = None;
        let mut change_accent: Option<Option<String>> = None;
        let mut toggle_boot = false;
        let mut do_import = false;
        let mut do_export = false;
        let mut close_requested = false;

        let window_id = egui::Id::new("settings_modal_dialog");
        egui::Window::new("Settings & Preferences")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(true)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .fixed_size(egui::vec2(modal_w, max_h.min(580.0)))
            .frame(
                egui::Frame::none()
                    .fill(palette.bg_card)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(18.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 12.0),
                        blur: 32.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(180),
                    })
                    .inner_margin(egui::Margin::symmetric(18.0, 16.0)),
            )
            .show(ctx, |ui| {
                *ui.visuals_mut() = palette.egui_visuals();
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                ui.spacing_mut().interact_size.y = 26.0;
                ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
                ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(11.5));
                ui.visuals_mut().widgets.inactive.weak_bg_fill = palette.bg_button;
                // Header with Modal Icon Pill, Title & Close Button
                ui.horizontal(|ui| {
                    ui.set_height(38.0);
                    ui.set_min_height(38.0);
                    Frame::none()
                        .fill(palette.accent_subtle)
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(crate::lucide::SETTINGS).size(14.0).color(palette.accent));
                        });
                    ui.add_space(4.0);
                    let title_width = (ui.available_width() - 34.0).max(0.0);
                    ui.allocate_ui_with_layout(egui::vec2(title_width, 38.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add(egui::Label::new(RichText::new("Settings & Preferences").size(15.0).strong().color(palette.text_primary)).truncate());
                        ui.add(egui::Label::new(RichText::new("Appearance, startup and backups").size(10.5).color(palette.text_muted)).truncate());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_close = egui::Button::new(RichText::new(crate::lucide::X).size(12.0).color(palette.text_muted))
                            .fill(palette.bg_button)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(26.0, 26.0));
                        if ui.add(btn_close).hover_tip("Close settings (Esc)").clicked() {
                            close_requested = true;
                        }
                    });
                });

                ui.add_space(10.0);

                // Segmented Tab Navigation Bar
                Frame::none()
                    .fill(palette.bg_input)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(5.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let tabs = SettingsTab::all();
                            let avail_w = ui.available_width() - ((tabs.len() - 1) as f32 * 4.0);
                            let tab_w = (avail_w / tabs.len() as f32).max(0.0);

                            for &tab in tabs {
                                let is_active = self.settings_active_tab == tab;
                                let (bg, fg) = if is_active {
                                    (palette.accent, palette.accent_text)
                                } else {
                                    (Color32::TRANSPARENT, palette.text_secondary)
                                };

                                let btn = egui::Button::new(
                                    RichText::new(if tab_w >= 108.0 { format!("{} {}", tab.icon(), tab.label()) }
                                        else if tab == SettingsTab::Backup { "Backup".to_string() }
                                        else { tab.label().to_string() })
                                        .size(11.0)
                                        .strong()
                                        .color(fg),
                                )
                                .fill(bg)
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(8.0))
                                .wrap_mode(egui::TextWrapMode::Truncate);
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(tab_w, 28.0), egui::Sense::hover());
                                if ui.put(rect, btn).hover_tip(tab.label()).clicked() {
                                    self.settings_active_tab = tab;
                                }
                            }
                        });
                    });

                ui.add_space(10.0);

                // Tab Content inside ScrollArea
                ScrollArea::vertical()
                    .id_salt(("settings_tab_content_scroll", self.settings_active_tab.label()))
                    .auto_shrink([false, false])
                    .max_height((ui.available_height() - 42.0 - 2.0 * ui.spacing().item_spacing.y).max(0.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 12.0;

                        match self.settings_active_tab {
                            SettingsTab::Appearance => {
                                // Base theme selection
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(14.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("{} Base theme", crate::lucide::PALETTE)).size(13.0).strong().color(palette.text_primary));

                                        ui.add_space(2.0);
                                        ui.label(RichText::new("Select obsidian dark, catppuccin, dracula, nord, light, or dynamic GTK4 system theme.").size(10.5).color(palette.text_muted));
                                        ui.add_space(8.0);

                                        // Theme Pills
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                            for &thm in ThemeMode::all() {
                                                let is_active = self.theme == thm;
                                                let (bg, fg) = if is_active {
                                                    (palette.accent, palette.accent_text)
                                                } else {
                                                    (palette.bg_button, palette.text_secondary)
                                                };
                                                let btn = egui::Button::new(RichText::new(format!("{} {}", thm.emoji(), thm.name())).size(10.5).strong().color(fg))
                                                    .fill(bg)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(Rounding::same(999.0))
                                                    .min_size(egui::vec2(0.0, 24.0));
                                                if ui.add(btn).clicked() {
                                                    change_theme = Some(thm);
                                                }
                                            }
                                        });
                                    });

                                // Accent colour selection
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(14.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("{} Accent colour", crate::lucide::SPARKLES)).size(13.0).strong().color(palette.text_primary));

                                        ui.add_space(2.0);
                                        ui.label(RichText::new("Choose the highlight colour for navigation, buttons and charts.").size(10.5).color(palette.text_muted));
                                        ui.add_space(10.0);

                                        // Follow System / GTK4 Accent Button
                                        let is_system_selected = self.custom_accent_color.as_deref() == Some("system")
                                            || self.custom_accent_color.as_deref() == Some("gtk4");
                                        let sys_bg = if is_system_selected { palette.accent } else { palette.bg_button };
                                        let sys_fg = if is_system_selected { palette.accent_text } else { palette.text_primary };
                                        let gtk4_color = crate::theme::current_gtk4_accent_color();
                                        let gtk4_hex = crate::theme::format_hex_color(gtk4_color);

                                        let btn_sys = egui::Button::new(
                                            RichText::new(format!("{} Follow Desktop / GTK4 Accent (Live: {})", crate::lucide::MONITOR, gtk4_hex))
                                                .size(11.0)
                                                .strong()
                                                .color(sys_fg),
                                        )
                                        .fill(sys_bg)
                                        .stroke(Stroke::NONE)
                                        .rounding(Rounding::same(999.0))
                                        .min_size(egui::vec2(ui.available_width(), 28.0));

                                        if ui.add(btn_sys).hover_tip("Automatically sync with desktop GNOME/GTK4 accent color in real-time").clicked() {
                                            change_accent = Some(Some("system".to_string()));
                                        }

                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Accent colours").size(10.0).monospace().strong().color(palette.text_muted));
                                        ui.add_space(6.0);

                                        // Keep swatches inside the body and stack them when two columns cannot fit.
                                        let presets = crate::theme::ACCENT_PRESETS;
                                        let swatch_columns = if ui.available_width() >= 440.0 { 2 } else { 1 };
                                        let swatch_width = (ui.available_width() - 8.0 * (swatch_columns - 1) as f32) / swatch_columns as f32;
                                        egui::Grid::new("accent_swatches_grid")
                                            .num_columns(swatch_columns)
                                            .spacing(egui::vec2(8.0, 6.0))
                                            .min_col_width(swatch_width)
                                            .max_col_width(swatch_width)
                                            .show(ui, |ui| {
                                                for (i, p_set) in presets.iter().enumerate() {
                                                    let is_active = if let Some(ref c_str) = self.custom_accent_color {
                                                        c_str.eq_ignore_ascii_case(p_set.hex)
                                                    } else {
                                                        false
                                                    };

                                                    let bg_card = if is_active {
                                                        palette.accent_subtle
                                                    } else {
                                                        palette.bg_card
                                                    };
                                                    let border_stroke = if is_active {
                                                        Stroke::new(1.5_f32, p_set.color)
                                                    } else {
                                                        Stroke::NONE
                                                    };

                                                    let swatch = Frame::none()
                                                        .fill(bg_card)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(8.0))
                                                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                                        .show(ui, |ui| {
                                                            ui.set_width((swatch_width - 20.0).max(0.0));
                                                            ui.horizontal(|ui| {
                                                                ui.spacing_mut().item_spacing.x = 8.0;

                                                                // Color dot
                                                                let (resp, painter) = ui.allocate_painter(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                                                painter.circle_filled(resp.rect.center(), 7.0, p_set.color);
                                                                if is_active {
                                                                    painter.circle_stroke(resp.rect.center(), 7.0, Stroke::new(1.5_f32, Color32::WHITE));
                                                                }

                                                                ui.vertical(|ui| {
                                                                    ui.set_width((ui.available_width() - 50.0).max(0.0));
                                                                    ui.add(egui::Label::new(RichText::new(p_set.name).size(11.0).strong().color(palette.text_primary)).truncate()).hover_tip(p_set.name);
                                                                    ui.label(RichText::new(p_set.hex).size(9.5).monospace().color(palette.text_muted));
                                                                });

                                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                    if is_active {
                                                                        ui.label(RichText::new("✓").size(12.0).strong().color(palette.accent));
                                                                    } else {
                                                                        let btn_apply = egui::Button::new(RichText::new("Use").size(10.0).color(palette.text_secondary))
                                                                            .fill(palette.bg_button)
                                                                            .stroke(Stroke::NONE)
                                                                            .rounding(Rounding::same(999.0))
                                                                            .min_size(egui::vec2(42.0, 20.0));
                                                                        if ui.add(btn_apply).clicked() {
                                                                            change_accent = Some(Some(p_set.hex.to_string()));
                                                                        }
                                                                    }
                                                                });
                                                            });
                                                        });

                                                    if is_active {
                                                        ui.painter().rect_stroke(swatch.response.rect.shrink(0.75), Rounding::same(8.0), border_stroke);
                                                    }
                                                    if (i + 1) % swatch_columns == 0 {
                                                        ui.end_row();
                                                    }
                                                }
                                            });

                                        ui.add_space(8.0);

                                        // Custom Hex input row
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            ui.label(RichText::new("Custom Hex:").size(11.0).color(palette.text_secondary));

                                            let te = egui::TextEdit::singleline(&mut self.settings_custom_hex_input)
                                                .text_color(palette.text_secondary)
                                                .hint_text(RichText::new("#3584e4").color(palette.text_muted))
                                                .margin(egui::vec2(6.0, 4.0)).min_size(egui::vec2(0.0, 26.0))
                                                .desired_width(85.0)
                                                .font(egui::TextStyle::Monospace);
                                            ui.add(te);

                                            let btn_apply_hex = egui::Button::new(RichText::new("Apply").size(10.5).strong().color(palette.accent_text))
                                                .fill(palette.accent)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(55.0, 22.0));
                                            if ui.add(btn_apply_hex).clicked() {
                                                if let Some(_) = crate::theme::parse_hex_color(&self.settings_custom_hex_input) {
                                                    let formatted = self.settings_custom_hex_input.trim().to_string();
                                                    change_accent = Some(Some(formatted));
                                                }
                                            }

                                            let btn_reset = egui::Button::new(RichText::new("Reset to Theme Default").size(10.5).color(palette.text_muted))
                                                .fill(palette.bg_button)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(140.0, 22.0));
                                            if ui.add(btn_reset).hover_tip("Clear custom accent and restore the native theme palette").clicked() {
                                                change_accent = Some(None);
                                            }
                                        });

                                        ui.add_space(10.0);

                                        // Live Component Preview Box
                                        Frame::none()
                                            .fill(palette.bg_input)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(RichText::new("LIVE PREVIEW:").size(9.5).monospace().strong().color(palette.text_muted));
                                                    ui.add_space(6.0);

                                                    // Chip
                                                    Frame::none()
                                                        .fill(palette.accent_subtle)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0))
                                                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                                        .show(ui, |ui| {
                                                            ui.label(RichText::new("● 8080 Active").size(10.0).strong().color(palette.accent));
                                                        });

                                                    ui.add_space(4.0);

                                                    // Button
                                                    let btn_demo = egui::Button::new(RichText::new("Action Button").size(10.5).strong().color(palette.accent_text))
                                                        .fill(palette.accent)
                                                        .stroke(Stroke::NONE)
                                                        .rounding(Rounding::same(999.0))
                                                        .min_size(egui::vec2(85.0, 22.0));
                                                    ui.add(btn_demo);

                                                    ui.add_space(4.0);

                                                    // Sparkline preview
                                                    let demo_pts = [15.0_f32, 22.0, 48.0, 35.0, 60.0, 42.0, 85.0, 70.0];
                                                    draw_core_waveform(ui, 70.0, 20.0, &demo_pts, palette.accent);
                                                });
                                            });
                                    });
                            }
                            SettingsTab::Autostart => {
                                // AUTOSTART CARD
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(16.0))
                                    .show(ui, |ui| {
                                        let is_boot = crate::autostart::is_app_autostart_enabled();

                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(format!("{} Start at login", crate::lucide::ROCKET)).size(14.0).strong().color(palette.text_primary));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let mut boot_toggle = is_boot;
                                                if render_custom_toggle_switch(ui, &mut boot_toggle, palette.accent) {
                                                    toggle_boot = true;
                                                }
                                            });
                                        });

                                        ui.add_space(4.0);
                                        ui.label(RichText::new("Open Server Launcher automatically when you sign in.").size(11.0).color(palette.text_muted));
                                        ui.add_space(12.0);

                                        let (status_txt, status_bg, status_fg) = if is_boot {
                                            (format!("{} Startup launch enabled", crate::lucide::CHECK), palette.success_subtle, palette.success)
                                        } else {
                                            (format!("{} Startup launch disabled", crate::lucide::CIRCLE), palette.bg_subtle, palette.text_muted)
                                        };

                                        Frame::none()
                                            .fill(status_bg)
                                            .stroke(Stroke::NONE)
                                            .rounding(Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new(status_txt).size(11.0).strong().color(status_fg));
                                            });
                                    });
                                ui.add_space(12.0);
                                let previous = (self.ssh_idle, self.ssh_probe_timeouts);
                                Frame::none().fill(palette.bg_subtle).rounding(12.0)
                                    .inner_margin(16.0).show(ui, |ui| {
                                        use crate::config::SshIdleMode;
                                        ui.label(RichText::new("SSH command timeout").size(14.0).strong().color(palette.text_primary));
                                        ui.label("Applies to commands launched from the SSH Servers page.");
                                        let modes = [
                                            (SshIdleMode::PageOrWindowInactive, "When the SSH page or window is inactive"),
                                            (SshIdleMode::WindowInactive, "When the window is inactive"),
                                            (SshIdleMode::Never, "Keep connections open"),
                                        ];
                                        let selected = modes.iter().find(|(mode, _)| *mode == self.ssh_idle.mode).unwrap().1;
                                        egui::ComboBox::from_id_salt("ssh_idle_mode")
                                            .selected_text(selected).show_ui(ui, |ui| {
                                                for (mode, label) in modes {
                                                    ui.selectable_value(&mut self.ssh_idle.mode, mode, label);
                                                }
                                            });
                                        ui.add_enabled(
                                            self.ssh_idle.mode != SshIdleMode::Never,
                                            egui::DragValue::new(&mut self.ssh_idle.minutes).range(1..=120).suffix(" min"),
                                        ).hover_tip("Continuous time inactive before closing the local SSH connection");
                                        ui.separator();
                                        ui.label(RichText::new("SSH probe timeouts").size(14.0).strong().color(palette.text_primary));
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label("Connection test");
                                            ui.add(egui::DragValue::new(&mut self.ssh_probe_timeouts.connection_test_seconds)
                                                .range(5..=120).suffix(" s"));
                                            ui.label("Port scan");
                                            ui.add(egui::DragValue::new(&mut self.ssh_probe_timeouts.port_scan_seconds)
                                                .range(5..=120).suffix(" s"));
                                        });
                                        ui.label("Applies the next time a test or scan starts.");
                                    });
                                if previous != (self.ssh_idle, self.ssh_probe_timeouts) {
                                    self.persist_config();
                                }
                            }
                            SettingsTab::Backup => {
                                // SERVERS IMPORT / EXPORT CARD
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(16.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("{} Configuration Data & Backups", crate::lucide::PACKAGE)).size(14.0).strong().color(palette.text_primary));
                                        ui.add_space(4.0);
                                        ui.label(RichText::new("Export or import a complete backup including servers, SSH hosts, ADB devices, scripts, saved commands, and preferences.").size(11.0).color(palette.text_muted));
                                        ui.add_space(14.0);

                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;

                                            let btn_import = egui::Button::new(RichText::new(format!("{} Import Backup", crate::lucide::DOWNLOAD)).size(11.5).strong().color(palette.accent_text))
                                                .fill(palette.accent)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(160.0, 32.0));
                                            if ui.add(btn_import).hover_tip("Select and import a backup or servers.json file").clicked() {
                                                do_import = true;
                                            }

                                            let btn_export = egui::Button::new(RichText::new(format!("{} Export Full Backup", crate::lucide::UPLOAD)).size(11.5).strong().color(palette.text_primary))
                                                .fill(palette.bg_button)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(999.0))
                                                .min_size(egui::vec2(160.0, 32.0));
                                            if ui.add(btn_export).hover_tip("Export all servers, SSH hosts, ADB devices, scripts, commands, and settings").clicked() {
                                                do_export = true;
                                            }
                                        });

                                        if let Some((ref msg, is_err, _)) = self.import_status_msg {
                                            ui.add_space(10.0);
                                            let color = if is_err { palette.danger } else { palette.success };
                                            Frame::none()
                                                .fill(palette.bg_subtle)
                                                .stroke(Stroke::NONE)
                                                .rounding(Rounding::same(6.0))
                                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new(msg).size(11.0).color(color));
                                                });
                                        }
                                    });
                            }
                            SettingsTab::Shortcuts => {
                                // KEYBOARD SHORTCUTS MATRIX
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(16.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(format!("{} Keyboard shortcuts", crate::lucide::KEYBOARD)).size(14.0).strong().color(palette.text_primary));
                                        ui.add_space(4.0);
                                        ui.label(RichText::new("Shortcuts for navigation and server controls.").size(11.0).color(palette.text_muted));
                                        ui.add_space(12.0);

                                        let shortcuts = [
                                            ("Ctrl+P / Ctrl+K", "Quick Launch & Command Palette"),
                                            ("Ctrl+,", "Open Settings & Preferences"),
                                            ("Ctrl+A", "Start All Servers"),
                                            ("Ctrl+X", "Stop All Servers"),
                                            ("Ctrl+F", "Focus Instance Search Bar"),
                                            ("Esc", "Close Modals / Dismiss Dialogs"),
                                        ];

                                        for (keys, desc) in shortcuts {
                                            ui.horizontal_top(|ui| {
                                                Frame::none()
                                                    .fill(palette.bg_subtle)
                                                    .stroke(Stroke::NONE)
                                                    .rounding(Rounding::same(6.0))
                                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                                    .show(ui, |ui| {
                                                        ui.set_width(110.0);
                                                        ui.label(RichText::new(keys).size(10.5).monospace().strong().color(palette.accent));
                                                    });
                                                ui.add_space(6.0);
                                                ui.add(egui::Label::new(RichText::new(desc).size(11.5).color(palette.text_secondary)).wrap());
                                            });
                                            ui.add_space(3.0);
                                        }
                                    });
                            }
                            SettingsTab::RemoteDesktop => {
                                let previous = self.remote_desktop_settings.clone();
                                let saved_pins = crate::remote_desktop::trust::saved_pins(
                                    &self.config_path,
                                    &self.ssh_hosts,
                                );
                                let mut remove_pin = None;
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(16.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Remote desktop viewer").size(14.0).strong().color(palette.text_primary));
                                        ui.add_space(4.0);
                                        ui.checkbox(&mut self.remote_desktop_settings.show_live_fps, "Show live FPS by default");
                                        ui.label(RichText::new("Displays the recent rate of framebuffer updates in each new viewer window.").size(10.5).color(palette.text_muted));
                                        ui.add_space(8.0);
                                        ui.checkbox(&mut self.remote_desktop_settings.warn_unencrypted_vnc, "Warn before direct VNC connections to public or unknown hosts");
                                        ui.label(RichText::new("Direct classic VNC does not encrypt screen updates or input. The warning offers an SSH tunnel before connecting.").size(10.5).color(palette.text_muted));
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Screenshot folder").size(12.0).strong().color(palette.text_primary));
                                        ui.horizontal(|ui| {
                                            if ui.button("Choose folder…").clicked() {
                                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                                    self.remote_desktop_settings.screenshot_folder = Some(path);
                                                }
                                            }
                                            if self.remote_desktop_settings.screenshot_folder.is_some()
                                                && ui.button("Use default").clicked()
                                            {
                                                self.remote_desktop_settings.screenshot_folder = None;
                                            }
                                        });
                                        ui.label(
                                            RichText::new(self.remote_desktop_settings.screenshot_folder.as_ref()
                                                .map_or_else(|| "Default: Pictures/ServerLauncher".to_owned(), |path| path.display().to_string()))
                                                .size(10.5).monospace().color(palette.text_muted),
                                        );
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Recording folder").size(12.0).strong().color(palette.text_primary));
                                        ui.horizontal(|ui| {
                                            if ui.button("Choose folder…").clicked() {
                                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                                    self.remote_desktop_settings.recording_folder = Some(path);
                                                }
                                            }
                                            if self.remote_desktop_settings.recording_folder.is_some()
                                                && ui.button("Use default").clicked()
                                            {
                                                self.remote_desktop_settings.recording_folder = None;
                                            }
                                        });
                                        ui.label(
                                            RichText::new(self.remote_desktop_settings.recording_folder.as_ref()
                                                .map_or_else(|| "Default: Pictures/ServerLauncher".to_owned(), |path| path.display().to_string()))
                                                .size(10.5).monospace().color(palette.text_muted),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Recording frame rate").size(12.0).strong().color(palette.text_primary));
                                            egui::ComboBox::from_id_salt("remote_recording_fps")
                                                .selected_text(format!("{} FPS", self.remote_desktop_settings.recording_fps))
                                                .show_ui(ui, |ui| {
                                                    for fps in [15, 24, 30, 60] {
                                                        ui.selectable_value(
                                                            &mut self.remote_desktop_settings.recording_fps,
                                                            fps,
                                                            format!("{fps} FPS"),
                                                        );
                                                    }
                                                });
                                        });
                                        ui.add_space(12.0);
                                        ui.label(RichText::new("Trusted RDP certificates").size(13.0).strong().color(palette.text_primary));
                                        match &saved_pins {
                                            Ok(pins) if pins.is_empty() => {
                                                ui.label(RichText::new("No saved certificate fingerprints.").size(10.5).color(palette.text_muted));
                                            }
                                            Ok(pins) => {
                                                for (index, pin) in pins.iter().enumerate() {
                                                    Frame::none()
                                                        .fill(palette.bg_card)
                                                        .rounding(Rounding::same(8.0))
                                                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                                        .show(ui, |ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.vertical(|ui| {
                                                                    ui.label(RichText::new(&pin.name).size(11.5).color(palette.text_primary));
                                                                    ui.add(egui::Label::new(RichText::new(&pin.fingerprint).size(9.5).monospace().color(palette.text_muted)).wrap());
                                                                });
                                                                if ui.button("Remove trust").clicked() {
                                                                    remove_pin = Some(index);
                                                                }
                                                            });
                                                        });
                                                }
                                            }
                                            Err(_) => {
                                                ui.label(RichText::new("Could not read saved certificate fingerprints.").size(10.5).color(palette.danger));
                                            }
                                        }
                                    });
                                if let (Some(index), Ok(pins)) = (remove_pin, saved_pins) {
                                    if let Some(pin) = pins.get(index) {
                                        if crate::remote_desktop::trust::remove_saved_pin(&pin.path).is_err() {
                                            self.import_status_msg = Some(("Could not remove the saved RDP certificate fingerprint.".into(), true, std::time::Instant::now()));
                                        }
                                    }
                                }
                                if previous != self.remote_desktop_settings {
                                    self.persist_config();
                                }
                            }
                            SettingsTab::About => {
                                crate::updater::render(ui, palette);
                                ui.add_space(12.0);
                                // DIAGNOSTICS & SYSTEM INFO
                                Frame::none()
                                    .fill(palette.bg_subtle)
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(16.0))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.label(RichText::new(format!("{} System Diagnostics", crate::lucide::INFO)).size(14.0).strong().color(palette.text_primary));
                                        ui.add_space(10.0);

                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new("Host LAN IP:").size(11.0).color(palette.text_muted));
                                            ui.label(RichText::new(&self.lan_ip).size(11.0).monospace().strong().color(palette.text_primary));
                                            ui.add_space(12.0);
                                            ui.label(RichText::new("Managed Servers:").size(11.0).color(palette.text_muted));
                                            ui.label(RichText::new(format!("{}", self.services.len())).size(11.0).strong().color(palette.text_primary));
                                        });

                                        ui.add_space(8.0);
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new("Active Config File:").size(11.0).color(palette.text_muted));
                                            ui.add(egui::Label::new(RichText::new(self.config_path.display().to_string()).size(10.5).monospace().color(palette.text_secondary)).truncate()).hover_tip(self.config_path.display().to_string());
                                        });
                                    });
                            }
                        }
                    });

                ui.add_space(12.0);

                // Footer Done Button
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_done = egui::Button::new(RichText::new("Done").size(12.0).strong().color(palette.accent_text))
                            .fill(palette.accent)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(90.0, 30.0));
                        if ui.add(btn_done).clicked() {
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            is_open = false;
        }

        // Apply any actions after UI pass (avoids borrow conflict)
        if let Some(acc) = change_accent {
            self.custom_accent_color = acc;
            let pal = self.palette();
            ctx.set_visuals(pal.egui_visuals());
            self.last_applied_palette = Some(pal);
            self.persist_config();
        }
        if let Some(thm) = change_theme {
            if thm != self.theme {
                self.theme = thm;
                let pal = self.palette();
                ctx.set_visuals(pal.egui_visuals());
                self.last_applied_palette = Some(pal);
                self.persist_config();
            }
        }
        if toggle_boot {
            let is_boot = crate::autostart::is_app_autostart_enabled();
            let new_state = !is_boot;
            let _ = crate::autostart::set_app_autostart(new_state);
            self.app_autostart = new_state;
            self.persist_config();
        }
        if do_import {
            self.import_servers_from_file_dialog();
            let pal = self.palette();
            ctx.set_visuals(pal.egui_visuals());
            self.last_applied_palette = Some(pal);
        }
        if do_export {
            self.export_servers_to_file_dialog();
        }

        self.settings_modal_open = is_open;
    }
}
