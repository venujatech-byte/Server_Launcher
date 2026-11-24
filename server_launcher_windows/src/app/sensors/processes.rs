use super::*;

impl LauncherApp {
        pub(in crate::app) fn render_proc_properties_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if let Some((pid, res)) = self.proc_details_receiver.lock().unwrap().take() {
            if self.proc_properties_pid == pid {
                self.proc_properties_loading = false;
                match res {
                    Ok(data) => self.proc_properties_data = Some(data),
                    Err(e) => self.proc_properties_error = Some(e),
                }
            }
        }

        if !self.proc_properties_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.proc_properties_modal_open = false;
            return;
        }

        let mut is_open = self.proc_properties_modal_open;
        let screen = ctx.screen_rect();
        let modal_w = 560.0_f32.min(screen.width() - 40.0).max(380.0);
        let max_h = (screen.height() - 60.0).max(420.0);

        let window_id = egui::Id::new("proc_properties_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();

        egui::Window::new("Process Properties")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .default_height(480.0)
            .max_height(max_h)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, &format!("Properties: {} (PID {})", self.proc_properties_name, self.proc_properties_pid), &mut is_open);
                ui.add_space(8.0);

                ScrollArea::vertical().max_height((max_h - 100.0).max(100.0)).show(ui, |ui| {
                    if self.proc_properties_loading {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.add_space(8.0);
                            ui.label(RichText::new("Reading process metadata...").size(12.0).color(palette.text_muted));
                        });
                        ui.add_space(40.0);
                    } else if let Some(ref err) = self.proc_properties_error {
                        ui.add_space(16.0);
                        ui.label(RichText::new(format!("Error: {}", err)).size(12.0).color(palette.danger));
                        ui.add_space(16.0);
                    } else if let Some(ref details) = self.proc_properties_data {
                        // Summary status chips
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                            let (state_color, state_bg) = if details.state.starts_with('R') || details.state == "Running" {
                                (Color32::from_rgb(52, 211, 153), Color32::from_rgba_unmultiplied(16, 185, 129, 30))
                            } else if details.state.starts_with('T') || details.state.contains("stopped") {
                                (Color32::from_rgb(251, 191, 36), Color32::from_rgba_unmultiplied(245, 158, 11, 30))
                            } else {
                                (palette.text_secondary, palette.bg_button)
                            };
                            Frame::none().fill(state_bg).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(10.0, 3.5)).show(ui, |ui| {
                                ui.label(RichText::new(&details.state).size(11.0).strong().color(state_color));
                            });

                            Frame::none().fill(palette.bg_button).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(10.0, 3.5)).show(ui, |ui| {
                                ui.label(RichText::new(format!("PID {}", details.pid)).size(11.0).color(palette.text_primary));
                            });

                            if !details.user.is_empty() {
                                Frame::none().fill(palette.bg_button).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(10.0, 3.5)).show(ui, |ui| {
                                    ui.label(RichText::new(&details.user).size(11.0).color(palette.text_secondary));
                                });
                            }

                            Frame::none().fill(palette.bg_button).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(10.0, 3.5)).show(ui, |ui| {
                                ui.label(RichText::new(format!("{} threads", details.threads)).size(11.0).color(palette.text_secondary));
                            });
                        });

                        ui.add_space(10.0);

                        // Key metrics table
                        Frame::none()
                            .fill(Color32::from_rgb(22, 25, 35))
                            .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                egui::Grid::new("proc_props_details_grid")
                                    .num_columns(2)
                                    .spacing(egui::vec2(20.0, 6.0))
                                    .show(ui, |ui| {
                                        let mut prop_row = |name: &str, val: &str| {
                                            ui.label(RichText::new(name).size(11.5).color(palette.text_muted));
                                            ui.label(RichText::new(val).size(11.5).monospace().color(palette.text_primary));
                                            ui.end_row();
                                        };
                                        prop_row("Parent Process (PPID)", &details.ppid.map_or("-".to_string(), |p| p.to_string()));
                                        prop_row("Priority (Nice)", &details.nice.to_string());
                                        prop_row("CPU Usage", &format!("{}%", reported_sensor_value(Some(details.cpu_usage), 1)));
                                        prop_row("Memory (RSS)", &crate::sensors::format_bytes(details.rss_bytes));
                                        if details.vsz_bytes > 0 {
                                            prop_row("Virtual Memory (VSZ)", &crate::sensors::format_bytes(details.vsz_bytes));
                                        }
                                    });
                            });

                        ui.add_space(10.0);

                        // Paths and command line recessed code wells
                        let render_path_well = |ui: &mut egui::Ui, title: &str, content: &str| {
                            if content.is_empty() { return; }
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(title).size(11.0).strong().color(palette.text_secondary));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let btn = egui::Button::new(RichText::new(format!("{} Copy", crate::lucide::COPY)).size(10.0).color(palette.text_muted))
                                        .fill(palette.bg_button)
                                        .rounding(Rounding::same(4.0));
                                    if ui.add(btn).on_hover_text("Copy to clipboard").clicked() {
                                        ui.ctx().output_mut(|o| o.copied_text = content.to_string());
                                    }
                                });
                            });
                            ui.add_space(2.0);
                            Frame::none()
                                .fill(Color32::from_rgb(12, 14, 20))
                                .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.3)))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_max_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(content)
                                                .size(10.5)
                                                .monospace()
                                                .color(palette.text_primary),
                                        )
                                        .wrap(),
                                    );
                                });
                            ui.add_space(6.0);
                        };

                        render_path_well(ui, "Command Line", &details.cmdline);
                        render_path_well(ui, "Executable Binary", &details.exe_path);
                        render_path_well(ui, "Working Directory", &details.cwd);
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(RichText::new("Close").size(11.5).color(palette.text_primary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(999.0))
                            .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                            .min_size(egui::vec2(80.0, 26.0));
                        if ui.add(btn).clicked() {
                            is_open = false;
                        }
                    });
                });
            });

        self.proc_properties_modal_open = is_open;
    }

    pub(in crate::app) fn render_proc_memory_maps_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if let Some((pid, res)) = self.proc_maps_receiver.lock().unwrap().take() {
            if self.proc_maps_pid == pid {
                self.proc_maps_loading = false;
                match res {
                    Ok(maps) => self.proc_maps_data = Some(maps),
                    Err(e) => self.proc_maps_error = Some(e),
                }
            }
        }

        if !self.proc_maps_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.proc_maps_modal_open = false;
            return;
        }

        let mut is_open = self.proc_maps_modal_open;
        let screen = ctx.screen_rect();
        let modal_w = 780.0_f32.min(screen.width() - 40.0).max(450.0);
        let max_h = (screen.height() - 60.0).max(460.0);

        let window_id = egui::Id::new("proc_maps_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();

        egui::Window::new("Memory Maps")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .default_height(520.0)
            .max_height(max_h)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, &format!("Memory Maps: {} (PID {})", self.proc_maps_name, self.proc_maps_pid), &mut is_open);
                ui.add_space(8.0);

                // Search & Counter bar
                ui.horizontal(|ui| {
                    Frame::none()
                        .fill(Color32::from_rgb(22, 25, 35))
                        .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(palette.text_muted));
                                ui.add(egui::TextEdit::singleline(&mut self.proc_maps_search).hint_text("Filter address, permissions, or library...").desired_width(240.0).frame(false));
                                if !self.proc_maps_search.is_empty() {
                                    if ui.add(egui::Button::new(RichText::new(crate::lucide::X).size(10.0).color(palette.text_muted)).fill(Color32::TRANSPARENT).stroke(Stroke::NONE)).clicked() {
                                        self.proc_maps_search.clear();
                                    }
                                }
                            });
                        });

                    if let Some(ref maps) = self.proc_maps_data {
                        Frame::none().fill(palette.bg_button).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(9.0, 3.5)).show(ui, |ui| {
                            ui.label(RichText::new(format!("{} mappings", maps.len())).size(10.5).color(palette.text_muted));
                        });
                    }
                });

                ui.add_space(8.0);

                if self.proc_maps_loading {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(RichText::new("Reading memory mappings...").size(12.0).color(palette.text_muted));
                    });
                    ui.add_space(40.0);
                } else if let Some(ref err) = self.proc_maps_error {
                    ui.add_space(16.0);
                    ui.label(RichText::new(format!("Error: {}", err)).size(12.0).color(palette.danger));
                    ui.add_space(16.0);
                } else if let Some(ref maps) = self.proc_maps_data {
                    let filter = self.proc_maps_search.trim().to_lowercase();
                    let matching: Vec<&crate::procmgr::MemoryMapEntry> = maps
                        .iter()
                        .filter(|entry| {
                            filter.is_empty()
                                || entry.address.to_lowercase().contains(&filter)
                                || entry.pathname.to_lowercase().contains(&filter)
                                || entry.perms.contains(&filter)
                        })
                        .collect();

                    if matching.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("No matching memory mappings found.").size(11.5).color(palette.text_muted));
                        });
                        ui.add_space(20.0);
                    } else {
                        // Sticky Header
                        ui.horizontal(|ui| {
                            ui.add_sized([180.0, 18.0], egui::Label::new(RichText::new("Address Range").size(11.0).strong().color(palette.text_secondary)));
                            ui.add_sized([50.0, 18.0], egui::Label::new(RichText::new("Perms").size(11.0).strong().color(palette.text_secondary)));
                            ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new("Offset").size(11.0).strong().color(palette.text_secondary)));
                            ui.add_sized([55.0, 18.0], egui::Label::new(RichText::new("Device").size(11.0).strong().color(palette.text_secondary)));
                            ui.label(RichText::new("Mapping / Path").size(11.0).strong().color(palette.text_secondary));
                        });
                        ui.separator();

                        let row_height = 20.0_f32;
                        let total_rows = matching.len();
                        ScrollArea::vertical()
                            .max_height((max_h - 140.0).max(100.0))
                            .auto_shrink([false, false])
                            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                                for idx in row_range {
                                    if let Some(entry) = matching.get(idx) {
                                        ui.horizontal(|ui| {
                                            ui.add_sized([180.0, 18.0], egui::Label::new(RichText::new(&entry.address).size(10.5).monospace().color(palette.text_primary)));
                                            let perms_color = if entry.perms.contains('x') {
                                                Color32::from_rgb(52, 211, 153)
                                            } else if entry.perms.contains('w') {
                                                Color32::from_rgb(251, 191, 36)
                                            } else {
                                                palette.text_muted
                                            };
                                            ui.add_sized([50.0, 18.0], egui::Label::new(RichText::new(&entry.perms).size(10.5).monospace().color(perms_color)));
                                            ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new(&entry.offset).size(10.5).monospace().color(palette.text_muted)));
                                            ui.add_sized([55.0, 18.0], egui::Label::new(RichText::new(&entry.dev).size(10.5).monospace().color(palette.text_muted)));
                                            let path_display = if entry.pathname.is_empty() { "[anon]".to_string() } else { entry.pathname.clone() };
                                            ui.add(egui::Label::new(RichText::new(path_display).size(10.5).monospace().color(palette.text_primary)).truncate());
                                        });
                                    }
                                }
                            });
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(RichText::new("Close").size(11.5).color(palette.text_primary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(999.0))
                            .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                            .min_size(egui::vec2(80.0, 26.0));
                        if ui.add(btn).clicked() {
                            is_open = false;
                        }
                    });
                });
            });

        self.proc_maps_modal_open = is_open;
    }

    pub(in crate::app) fn render_proc_open_files_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if let Some((pid, res)) = self.proc_open_files_receiver.lock().unwrap().take() {
            if self.proc_open_files_pid == pid {
                self.proc_open_files_loading = false;
                match res {
                    Ok(files) => self.proc_open_files_data = Some(files),
                    Err(e) => self.proc_open_files_error = Some(e),
                }
            }
        }

        if !self.proc_open_files_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.proc_open_files_modal_open = false;
            return;
        }

        let mut is_open = self.proc_open_files_modal_open;
        let screen = ctx.screen_rect();
        let modal_w = 680.0_f32.min(screen.width() - 40.0).max(420.0);
        let max_h = (screen.height() - 60.0).max(440.0);

        let window_id = egui::Id::new("proc_open_files_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();

        egui::Window::new("Open Files")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .default_height(480.0)
            .max_height(max_h)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, &format!("Open Files: {} (PID {})", self.proc_open_files_name, self.proc_open_files_pid), &mut is_open);
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    Frame::none()
                        .fill(Color32::from_rgb(22, 25, 35))
                        .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SEARCH).size(11.0).color(palette.text_muted));
                                ui.add(egui::TextEdit::singleline(&mut self.proc_open_files_search).hint_text("Filter file descriptor or path...").desired_width(240.0).frame(false));
                                if !self.proc_open_files_search.is_empty() {
                                    if ui.add(egui::Button::new(RichText::new(crate::lucide::X).size(10.0).color(palette.text_muted)).fill(Color32::TRANSPARENT).stroke(Stroke::NONE)).clicked() {
                                        self.proc_open_files_search.clear();
                                    }
                                }
                            });
                        });

                    if let Some(ref files) = self.proc_open_files_data {
                        Frame::none().fill(palette.bg_button).rounding(Rounding::same(999.0)).inner_margin(egui::Margin::symmetric(9.0, 3.5)).show(ui, |ui| {
                            ui.label(RichText::new(format!("{} open files", files.len())).size(10.5).color(palette.text_muted));
                        });
                    }
                });

                ui.add_space(8.0);

                if self.proc_open_files_loading {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(RichText::new("Reading open file descriptors...").size(12.0).color(palette.text_muted));
                    });
                    ui.add_space(40.0);
                } else if let Some(ref err) = self.proc_open_files_error {
                    ui.add_space(16.0);
                    ui.label(RichText::new(format!("Error: {}", err)).size(12.0).color(palette.danger));
                    ui.add_space(16.0);
                } else if let Some(ref files) = self.proc_open_files_data {
                    let filter = self.proc_open_files_search.trim().to_lowercase();
                    let matching: Vec<&crate::procmgr::OpenFileEntry> = files
                        .iter()
                        .filter(|entry| {
                            filter.is_empty()
                                || entry.fd.contains(&filter)
                                || entry.path.to_lowercase().contains(&filter)
                        })
                        .collect();

                    if matching.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("No matching open files found.").size(11.5).color(palette.text_muted));
                        });
                        ui.add_space(20.0);
                    } else {
                        // Sticky Header
                        ui.horizontal(|ui| {
                            ui.add_sized([45.0, 18.0], egui::Label::new(RichText::new("FD").size(11.0).strong().color(palette.text_secondary)));
                            ui.label(RichText::new("Target Resource / Path").size(11.0).strong().color(palette.text_secondary));
                        });
                        ui.separator();

                        let row_height = 20.0_f32;
                        let total_rows = matching.len();
                        ScrollArea::vertical()
                            .max_height((max_h - 140.0).max(100.0))
                            .auto_shrink([false, false])
                            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                                for idx in row_range {
                                    if let Some(entry) = matching.get(idx) {
                                        ui.horizontal(|ui| {
                                            ui.add_sized([45.0, 18.0], egui::Label::new(RichText::new(&entry.fd).size(10.5).monospace().color(palette.accent_light)));
                                            ui.add(egui::Label::new(RichText::new(&entry.path).size(10.5).monospace().color(palette.text_primary)).truncate());
                                        });
                                    }
                                }
                            });
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(RichText::new("Close").size(11.5).color(palette.text_primary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(999.0))
                            .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                            .min_size(egui::vec2(80.0, 26.0));
                        if ui.add(btn).clicked() {
                            is_open = false;
                        }
                    });
                });
            });

        self.proc_open_files_modal_open = is_open;
    }

    pub(in crate::app) fn render_proc_affinity_modal(&mut self, ctx: &Context, palette: &crate::theme::ThemePalette) {
        if let Some((pid, res)) = self.proc_affinity_receiver.lock().unwrap().take() {
            if self.proc_affinity_pid == pid {
                self.proc_affinity_loading = false;
                match res {
                    Ok(cores) => {
                        let mut mask = vec![false; self.proc_affinity_total_cores];
                        for c in cores {
                            if c < self.proc_affinity_total_cores {
                                mask[c] = true;
                            }
                        }
                        if mask.iter().any(|&b| b) {
                            self.proc_affinity_cores = mask;
                        }
                    }
                    Err(e) => self.proc_affinity_error = Some(e),
                }
            }
        }

        if !self.proc_affinity_modal_open {
            return;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.proc_affinity_modal_open = false;
            return;
        }

        let mut is_open = self.proc_affinity_modal_open;
        let mut apply_affinity = false;
        let screen = ctx.screen_rect();
        let modal_w = 480.0_f32.min(screen.width() - 40.0).max(360.0);
        let max_h = (screen.height() - 60.0).max(400.0);

        let window_id = egui::Id::new("proc_affinity_modal_dialog");
        let drag_offset = ctx.data(|d| d.get_temp::<egui::Vec2>(window_id.with("modal_drag_offset"))).unwrap_or_default();

        egui::Window::new("Set CPU Affinity")
            .id(window_id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .current_pos(screen.center() + drag_offset)
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .default_height(380.0)
            .max_height(max_h)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(17, 19, 27))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 38, 54)))
                    .rounding(Rounding::same(16.0))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                crate::modals::render_modal_titlebar(ui, &format!("Set Affinity: {} (PID {})", self.proc_affinity_name, self.proc_affinity_pid), &mut is_open);
                ui.add_space(8.0);

                ui.label(RichText::new("Select which CPU cores this process is permitted to execute on:").size(11.5).color(palette.text_secondary));
                ui.add_space(6.0);

                // Quick buttons
                ui.horizontal(|ui| {
                    let btn_all = egui::Button::new(RichText::new("Select All").size(10.5).color(palette.text_primary))
                        .fill(palette.bg_button)
                        .rounding(Rounding::same(999.0))
                        .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)));
                    if ui.add(btn_all).clicked() {
                        for c in &mut self.proc_affinity_cores {
                            *c = true;
                        }
                    }

                    let btn_none = egui::Button::new(RichText::new("Deselect All").size(10.5).color(palette.text_muted))
                        .fill(palette.bg_button)
                        .rounding(Rounding::same(999.0))
                        .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)));
                    if ui.add(btn_none).clicked() {
                        for c in &mut self.proc_affinity_cores {
                            *c = false;
                        }
                    }
                });

                ui.add_space(8.0);

                if self.proc_affinity_loading {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label(RichText::new("Reading affinity mask...").size(11.5).color(palette.text_muted));
                    });
                    ui.add_space(30.0);
                } else {
                    Frame::none()
                        .fill(Color32::from_rgb(22, 25, 35))
                        .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(ui, |ui| {
                            ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                let total = self.proc_affinity_cores.len();
                                let cols = if total > 8 { 4 } else { 2 };
                                egui::Grid::new("proc_affinity_grid").num_columns(cols).spacing(egui::vec2(16.0, 8.0)).show(ui, |ui| {
                                    for i in 0..total {
                                        ui.checkbox(&mut self.proc_affinity_cores[i], format!("CPU {}", i));
                                        if (i + 1) % cols == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });
                            });
                        });
                }

                if let Some(ref err) = self.proc_affinity_error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Error: {}", err)).size(11.0).color(palette.danger));
                }

                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_apply = egui::Button::new(RichText::new("Apply Affinity").size(11.5).strong().color(palette.accent_text))
                            .fill(palette.accent)
                            .rounding(Rounding::same(999.0))
                            .stroke(Stroke::NONE)
                            .min_size(egui::vec2(110.0, 28.0));
                        if ui.add(btn_apply).clicked() {
                            apply_affinity = true;
                        }

                        let btn_cancel = egui::Button::new(RichText::new("Cancel").size(11.5).color(palette.text_secondary))
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(999.0))
                            .stroke(Stroke::new(1.0_f32, palette.border.gamma_multiply(0.4)))
                            .min_size(egui::vec2(80.0, 28.0));
                        if ui.add(btn_cancel).clicked() {
                            is_open = false;
                        }
                    });
                });
            });

        self.proc_affinity_modal_open = is_open;

        if apply_affinity {
            let sel_cores: Vec<usize> = self.proc_affinity_cores.iter().enumerate().filter(|(_, &b)| b).map(|(i, _)| i).collect();
            if sel_cores.is_empty() {
                self.proc_affinity_error = Some("At least one CPU core must be selected".to_string());
            } else {
                let pid = self.proc_affinity_pid;
                let expected_start = self.proc_affinity_start_marker;
                if let Some(ref hid) = self.proc_affinity_host_id {
                    if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == *hid).cloned() {
                        let host_name = host.name.clone();
                        let cores_clone = sel_cores.clone();
                        let results = self.remote_process_results.clone();
                        let ctx = ctx.clone();
                        self.sensors_top_apps_status_msg = Some((format!("Setting CPU affinity for PID {} on {}…", pid, host_name), false, Instant::now()));
                        std::thread::spawn(move || {
                            let result = crate::procmgr::set_remote_affinity(&host, pid, &cores_clone, expected_start);
                            let message = match result {
                                Ok(()) => (format!("Applied CPU affinity for PID {} on {}", pid, host_name), false),
                                Err(error) => (format!("Failed to set CPU affinity for PID {} on {}: {}", pid, host_name, error), true),
                            };
                            results.lock().unwrap().push_back(message);
                            ctx.request_repaint();
                        });
                    }
                } else {
                    match crate::procmgr::set_local_affinity(pid, &sel_cores, expected_start) {
                        Ok(_) => {
                            self.sensors_top_apps_status_msg = Some((format!("Applied CPU affinity for PID {}", pid), false, Instant::now()));
                        }
                        Err(e) => {
                            self.sensors_top_apps_status_msg = Some((format!("Failed to set affinity: {}", e), true, Instant::now()));
                        }
                    }
                }
                self.proc_affinity_modal_open = false;
            }
        }
    }

    pub(in crate::app) fn execute_proc_signal(&mut self, pid: u32, sig: &str, action_desc: &str, host_id: Option<&str>, expected_start: Option<u64>, ctx: &egui::Context) {
        if let Some(hid) = host_id {
            if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                let host_name = host.name.clone();
                let hid_clone = hid.to_string();
                let sig_str = sig.to_string();
                let snapshots_arc = self.remote_sensors_snapshots.clone();
                let last_fetch_arc = self.remote_sensors_last_fetch.clone();
                let errors_arc = self.remote_sensors_errors.clone();
                let results = self.remote_process_results.clone();
                let ctx = ctx.clone();

                self.sensors_top_apps_status_msg = Some((format!("Sending signal to PID {} on {}…", pid, host_name), false, Instant::now()));
                std::thread::spawn(move || {
                    let result = crate::procmgr::send_remote_signal(&host, pid, &sig_str, expected_start);
                    let message = match result {
                        Ok(()) => (format!("Sent SIG{} to remote process PID {} on {}", sig_str, pid, host_name), false),
                        Err(error) => (format!("Failed to signal PID {} on {}: {}", pid, host_name, if error.trim().is_empty() { "remote command failed" } else { error.trim() }), true),
                    };
                    results.lock().unwrap().push_back(message);
                    ctx.request_repaint();
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if let Ok(snap) = crate::sensors::query_remote_sensors(&host, true, true, false, false, false, false, false, false, false) {
                        snapshots_arc.lock().unwrap().insert(hid_clone.clone(), Arc::new(snap));
                        last_fetch_arc.lock().unwrap().insert(hid_clone.clone(), Instant::now());
                        errors_arc.lock().unwrap().remove(&hid_clone);
                    }
                });
            }
        } else {
            match crate::procmgr::send_local_signal(pid, sig, expected_start) {
                Ok(_) => {
                    self.sensors_top_apps_status_msg = Some((format!("{} process PID {}", action_desc, pid), false, Instant::now()));
                }
                Err(e) => {
                    self.sensors_top_apps_status_msg = Some((format!("Failed to send signal: {}", e), true, Instant::now()));
                }
            }
        }
        if sig == "TERM" || sig == "KILL" {
            if self.sensors_top_apps_selected_pid == Some(pid) {
                self.sensors_top_apps_selected_pid = None;
            }
        }
    }

    pub(in crate::app) fn render_sensors_top_apps_tab(&mut self, ui: &mut Ui, snap: &crate::sensors::SystemSensorsSnapshot, host_id: Option<&str>) {
        if self.sensors_top_apps_selected_pid.is_some_and(|pid| {
            !selected_process_present(&snap.top_apps, pid, self.sensors_top_apps_selected_start)
        }) {
            self.sensors_top_apps_selected_pid = None;
            self.sensors_top_apps_selected_start = None;
        }
        if let Some((message, is_error)) = self.remote_process_results.lock().unwrap().pop_front() {
            self.sensors_top_apps_status_msg = Some((message, is_error, Instant::now()));
        }
        let p = self.palette();
        let accent = self.sensors_accent_color(&p);

        // Fixed column widths and compact spacing matching Windows 11 Task Manager
        const COL_CPU_W: f32 = 78.0;
        const COL_MEM_W: f32 = 90.0;
        const COL_DISK_W: f32 = 86.0;
        const COL_NET_W: f32 = 76.0;
        const COL_GPU_W: f32 = 76.0;
        const COL_POWER_W: f32 = 92.0;
        const COL_SPACING: f32 = 8.0;

        let num_logical_cores = snap.cpu_cores_logical.max(1) as f32;
        // Total system CPU % normalization: 0 - 100% total system CPU matching Windows Task Manager
        let norm_factor = num_logical_cores;

        let mem_total_pct = (snap.mem_total_bytes > 0)
            .then(|| (snap.mem_used_bytes as f32 / snap.mem_total_bytes as f32) * 100.0);

        let total_disk_mb_s = (snap.total_disk_read_rate_bps + snap.total_disk_write_rate_bps) / (1024.0 * 1024.0);

        // Total GPU & Power values for header & rows
        let total_gpu_pct = crate::sensors::single_gpu_load(&snap.gpus);

        // Tint load cells with the selected app accent so custom themes stay consistent.
        let get_tm_cpu_bg = |norm_cpu: f32| -> Color32 {
            if norm_cpu < 0.1 {
                Color32::TRANSPARENT
            } else if norm_cpu < 2.0 {
                accent_with_alpha(accent, 35)
            } else if norm_cpu < 10.0 {
                accent_with_alpha(accent, 65)
            } else if norm_cpu < 25.0 {
                accent_with_alpha(accent, 110)
            } else if norm_cpu < 50.0 {
                accent_with_alpha(accent, 155)
            } else {
                accent_with_alpha(accent, 210)
            }
        };

        let get_tm_mem_bg = |bytes: u64| -> Color32 {
            if bytes < 50 * 1024 * 1024 {
                Color32::TRANSPARENT
            } else if bytes < 250 * 1024 * 1024 {
                accent_with_alpha(accent, 35)
            } else if bytes < 700 * 1024 * 1024 {
                accent_with_alpha(accent, 65)
            } else if bytes < 1800 * 1024 * 1024 {
                accent_with_alpha(accent, 110)
            } else {
                accent_with_alpha(accent, 155)
            }
        };

        let get_tm_disk_bg = |rate: f64| -> Color32 {
            if rate < 50_000.0 {
                Color32::TRANSPARENT
            } else if rate < 1_000_000.0 {
                accent_with_alpha(accent, 40)
            } else if rate < 10_000_000.0 {
                accent_with_alpha(accent, 85)
            } else {
                accent_with_alpha(accent, 140)
            }
        };

        let get_tm_gpu_bg = |gpu_pct: f32| -> Color32 {
            if gpu_pct < 0.1 {
                Color32::TRANSPARENT
            } else if gpu_pct < 2.0 {
                accent_with_alpha(accent, 35)
            } else if gpu_pct < 10.0 {
                accent_with_alpha(accent, 65)
            } else if gpu_pct < 25.0 {
                accent_with_alpha(accent, 110)
            } else {
                accent_with_alpha(accent, 175)
            }
        };


        // Text formatting helpers matching Image 2
        let format_tm_cpu = |norm_cpu: f32| -> String {
            if norm_cpu < 0.05 {
                "0%".to_string()
            } else if norm_cpu < 10.0 {
                format!("{:.1}%", norm_cpu)
            } else {
                format!("{:.0}%", norm_cpu)
            }
        };

        let format_tm_mem = |bytes: u64| -> String {
            if bytes < 1024 * 1024 * 1024 {
                format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
            }
        };

        let format_tm_disk = crate::sensors::format_rate;

        let mut kill_target_pid = None;
        let mut toggle_group_expand = None;
        let mut select_target_pid = None;
        let mut right_clicked_pid = None;
        let mut proc_action: Option<ProcAction> = None;

        Frame::none()
            .fill(p.bg_card)
            .rounding(Rounding::same(10.0))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                // ═════════════════════════════════════════════════════════════
                // WINDOWS TASK MANAGER TOOLBAR (Clean, Authentic Nocturnal Command)
                // ═════════════════════════════════════════════════════════════
                ui.horizontal(|ui| {
                    // Left: Title + Process count badge
                    ui.label(RichText::new("Processes").size(15.0).strong().color(p.text_primary));

                    let count_text = format!("{} processes", snap.top_apps.len());
                    Frame::none()
                        .fill(p.bg_button)
                        .rounding(Rounding::same(999.0))
                        .inner_margin(egui::Margin::symmetric(9.0, 3.5))
                        .show(ui, |ui| {
                            ui.label(RichText::new(count_text).size(10.5).color(p.text_muted));
                        });

                    // Right: Elegant Nocturnal Command actions aligned to the far right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        // 1. End Task Button (Far Right)
                        let has_selection = self.sensors_top_apps_selected_pid.is_some();
                        let btn_end = if has_selection {
                            let sel_pid = self.sensors_top_apps_selected_pid.unwrap();
                            egui::Button::new(
                                RichText::new(format!("{} End task (PID {})", crate::lucide::X, sel_pid))
                                    .size(11.0)
                                    .strong()
                                    .color(p.accent_text),
                            )
                            .fill(accent)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(0.0, 28.0))
                        } else {
                            egui::Button::new(
                                RichText::new(format!("{} End task", crate::lucide::X))
                                    .size(11.0)
                                    .color(p.text_muted),
                            )
                            .fill(p.bg_button.gamma_multiply(0.6))
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(999.0))
                            .min_size(egui::vec2(0.0, 28.0))
                        };
                        let btn_end_resp = ui.add(btn_end).on_hover_text(if has_selection {
                            "Click to terminate the selected process"
                        } else {
                            "Select a process from the list below, then click End task to terminate it"
                        });
                        if has_selection && btn_end_resp.clicked() {
                            kill_target_pid = self.sensors_top_apps_selected_pid;
                        }

                        // 2. Integrated Search Pill (Spacious, full placeholder, clear button)
                        Frame::none()
                            .fill(p.bg_card_active)
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    ui.label(RichText::new(crate::lucide::SEARCH).size(11.5).color(p.text_muted));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.sensors_search_query)
                                            .hint_text("Search by name or PID...")
                                            .desired_width(170.0)
                                            .frame(false),
                                    );
                                    if !self.sensors_search_query.is_empty() {
                                        if ui
                                            .add(
                                                egui::Button::new(RichText::new(crate::lucide::X).size(10.5).color(p.text_muted))
                                                    .fill(Color32::TRANSPARENT)
                                                    .stroke(Stroke::NONE),
                                            )
                                            .on_hover_text("Clear search filter")
                                            .clicked()
                                        {
                                            self.sensors_search_query.clear();
                                        }
                                    }
                                });
                            });

                        // 3. Dropdown controls with matching pill button geometry
                        ui.scope(|ui| {
                            ui.visuals_mut().widgets.inactive.rounding = Rounding::same(999.0);
                            ui.visuals_mut().widgets.inactive.bg_fill = p.bg_button;
                            ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                            ui.visuals_mut().widgets.hovered.rounding = Rounding::same(999.0);
                            ui.visuals_mut().widgets.active.rounding = Rounding::same(999.0);
                            ui.visuals_mut().widgets.open.rounding = Rounding::same(999.0);
                            ui.spacing_mut().button_padding = egui::vec2(10.0, 5.0);

                            // Refresh Interval Pill ComboBox
                            let interval_label = match self.sensors_refresh_interval {
                                0 => "Paused".to_string(),
                                1 => "1s (Default)".to_string(),
                                2 => "2s (Fast)".to_string(),
                                5 => "5s (Normal)".to_string(),
                                10 => "10s (Low)".to_string(),
                                n => format!("{}s", n),
                            };
                            egui::ComboBox::from_id_salt("tm_refresh_interval_combo")
                                .selected_text(RichText::new(format!("{} {}", crate::lucide::CLOCK, interval_label)).size(11.0).color(p.text_secondary))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.sensors_refresh_interval, 1, "1s (Default)");
                                    ui.selectable_value(&mut self.sensors_refresh_interval, 2, "2s (Fast)");
                                    ui.selectable_value(&mut self.sensors_refresh_interval, 5, "5s (Normal)");
                                    ui.selectable_value(&mut self.sensors_refresh_interval, 10, "10s (Low)");
                                    ui.selectable_value(&mut self.sensors_refresh_interval, 0, "Paused");
                                });
                        });
                    });
                });

                // Status message (e.g. process terminated)
                if let Some((ref msg, is_err, timestamp)) = self.sensors_top_apps_status_msg {
                    if timestamp.elapsed() < Duration::from_secs(4) {
                        ui.add_space(6.0);
                        Frame::none()
                            .fill(accent_with_alpha(accent, 25))
                            .rounding(Rounding::same(999.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let icon = if is_err { crate::lucide::ALERT_TRIANGLE } else { crate::lucide::CHECK };
                                    ui.label(RichText::new(icon).size(11.0).color(if is_err { p.text_muted } else { accent }));
                                    ui.label(RichText::new(msg).size(11.0).strong().color(p.text_primary));
                                });
                            });
                    }
                }

                ui.add_space(8.0);

                // Filter and group processes with intelligent caching to avoid redundant sorting and allocations
                let search_q = self.sensors_search_query.trim().to_lowercase();
                let cache_valid = self.sensors_cached_snap_timestamp == snap.timestamp
                    && self.sensors_cached_search == search_q
                    && self.sensors_cached_sort == self.sensors_top_apps_sort
                    && self.sensors_cached_sort_desc == self.sensors_top_apps_sort_desc;

                if !cache_valid {
                    let filtered_apps = filtered_processes(&snap.top_apps, &search_q);

                    let is_foreground = |name_lower: &str| -> bool {
                        if name_lower.starts_with('[')
                            || name_lower.starts_with("kworker")
                            || name_lower.starts_with("irq/")
                            || name_lower.starts_with("ksoftirqd")
                            || name_lower.starts_with("rcu_")
                            || name_lower.starts_with("migration")
                            || name_lower.starts_with("kswapd")
                            || name_lower.starts_with("jbd2")
                            || name_lower.starts_with("cpuhp")
                            || name_lower.starts_with("khugepaged")
                            || name_lower.starts_with("kcompactd")
                            || name_lower.starts_with("kdevtmpfs")
                            || name_lower.starts_with("kauditd")
                            || name_lower.starts_with("threadpool")
                            || name_lower.starts_with("dedicatedworker")
                            || name_lower.starts_with("chrome_")
                            || name_lower.starts_with("perfetto")
                            || name_lower.starts_with("compositor")
                            || name_lower.starts_with("js helper")
                            || name_lower.starts_with("v8 ")
                            || name_lower.starts_with("crashpad")
                            || name_lower.starts_with("sandbox")
                            || name_lower.starts_with("gmain")
                            || name_lower.starts_with("gdbus")
                        {
                            return false;
                        }
                        const BG_KEYWORDS: &[&str] = &[
                            "systemd", "dbus", "polkit", "accounts-daemon", "upower", "udevd", "cron", "crond",
                            "syslog", "rsyslog", "networkmanager", "wpa_supplicant", "bluetoothd", "cupsd",
                            "avahi-daemon", "colord", "boltd", "thermald", "auditd", "atd", "smartd", "gdm",
                            "lightdm", "sddm", "pipewire", "wireplumber", "pulseaudio", "rtkit-daemon",
                            "packagekitd", "kerneloops", "whoopsie", "unattended-upgr", "snapd", "containerd",
                            "dockerd", "libvirtd", "libinput-debug", "systemd-oomd", "systemd-resolved",
                            "systemd-journald", "systemd-logind", "systemd-udevd", "systemd-timesyncd",
                            "irqbalance", "acpid", "mcelog", "gssproxy", "rpcbind", "sshd"
                        ];
                        for kw in BG_KEYWORDS {
                            if name_lower.contains(kw) {
                                return false;
                            }
                        }
                        true
                    };

                    let mut groups_map: std::collections::BTreeMap<String, (String, bool, Vec<crate::sensors::ProcessSensorInfo>)> = std::collections::BTreeMap::new();
                    for app in filtered_apps {
                        let key = app.name.trim().to_lowercase();
                        // The collector has no reliable Windows foreground-app classification.
                        let is_app = !cfg!(windows) && is_foreground(&key);
                        let entry = groups_map.entry(key).or_insert_with(|| (app.name.clone(), is_app, Vec::new()));
                        entry.2.push(app.clone());
                    }

                    let mut all_groups: Vec<ProcessGroupSummary> = groups_map.into_iter().map(|(key, (disp_name, is_app, mut list))| {
                        list.sort_by(|a, b| {
                            b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal)
                                .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
                                .then_with(|| a.pid.cmp(&b.pid))
                        });
                        let sum_cpu: f32 = list.iter().map(|p| p.cpu_usage).sum();
                        let cpu_usage_available = list.iter().all(|process| process.cpu_usage_available);
                        let sum_mem: u64 = list.iter().map(|p| p.memory_bytes).sum();
                        let sum_disk_rate: f64 = list.iter().map(|p| p.disk_rate_bps).sum();
                        let disk_rate_available = list.iter().all(|process| process.disk_rate_available);
                        let sum_net_rate: f64 = list.iter().map(|p| p.net_rate_bps).sum();
                        let net_rate_available = list.iter().all(|process| process.net_rate_available);
                        let sum_net_bytes: u64 = list.iter().map(|p| p.net_rx_bytes.saturating_add(p.net_tx_bytes)).sum();
                        let sum_net_bytes_available = list.iter().all(|process| process.net_counters_available);
                        let pids: Vec<u32> = list.iter().map(|p| p.pid).collect();
                        // Linux does not expose per-process GPU attribution through this collector.
                        let sum_gpu = 0.0;
                        let sum_power_watts = 0.0;
                        ProcessGroupSummary {
                            key,
                            display_name: disp_name,
                            sum_cpu,
                            cpu_usage_available,
                            sum_mem,
                            sum_disk_rate,
                            disk_rate_available,
                            sum_net_rate,
                            net_rate_available,
                            sum_net_bytes,
                            sum_net_bytes_available,
                            sum_gpu,
                            sum_power_watts,
                            is_app,
                            pids,
                            children: list,
                        }
                    }).collect();

                    let desc = self.sensors_top_apps_sort_desc;
                    match self.sensors_top_apps_sort.as_str() {
                        "mem" => all_groups.sort_by(|a, b| {
                            let ord = if desc { b.sum_mem.cmp(&a.sum_mem) } else { a.sum_mem.cmp(&b.sum_mem) };
                            ord.then_with(|| b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                        "disk" => all_groups.sort_by(|a, b| {
                            let ord = if desc {
                                b.sum_disk_rate.partial_cmp(&a.sum_disk_rate).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                a.sum_disk_rate.partial_cmp(&b.sum_disk_rate).unwrap_or(std::cmp::Ordering::Equal)
                            };
                            ord.then_with(|| b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                        "name" => all_groups.sort_by(|a, b| {
                            if desc {
                                b.display_name.to_lowercase().cmp(&a.display_name.to_lowercase())
                            } else {
                                a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase())
                            }
                        }),
                        "net" => all_groups.sort_by(|a, b| {
                            let ord = if desc {
                                b.sum_net_rate.partial_cmp(&a.sum_net_rate).unwrap_or(std::cmp::Ordering::Equal)
                                    .then_with(|| b.sum_net_bytes.cmp(&a.sum_net_bytes))
                            } else {
                                a.sum_net_rate.partial_cmp(&b.sum_net_rate).unwrap_or(std::cmp::Ordering::Equal)
                                    .then_with(|| a.sum_net_bytes.cmp(&b.sum_net_bytes))
                            };
                            ord.then_with(|| b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                        "gpu" => all_groups.sort_by(|a, b| {
                            let ord = if desc {
                                b.sum_gpu.partial_cmp(&a.sum_gpu).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                a.sum_gpu.partial_cmp(&b.sum_gpu).unwrap_or(std::cmp::Ordering::Equal)
                            };
                            ord.then_with(|| b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                        "power" => all_groups.sort_by(|a, b| {
                            let ord = if desc {
                                b.sum_power_watts.partial_cmp(&a.sum_power_watts).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                a.sum_power_watts.partial_cmp(&b.sum_power_watts).unwrap_or(std::cmp::Ordering::Equal)
                            };
                            ord.then_with(|| b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                        _ => all_groups.sort_by(|a, b| {
                            let ord = if desc {
                                b.sum_cpu.partial_cmp(&a.sum_cpu).unwrap_or(std::cmp::Ordering::Equal)
                            } else {
                                a.sum_cpu.partial_cmp(&b.sum_cpu).unwrap_or(std::cmp::Ordering::Equal)
                            };
                            ord.then_with(|| b.sum_mem.cmp(&a.sum_mem))
                                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
                        }),
                    }

                    let (apps_groups, bg_groups): (Vec<ProcessGroupSummary>, Vec<ProcessGroupSummary>) = all_groups.into_iter().partition(|g| g.is_app);

                    self.sensors_cached_apps_groups = apps_groups;
                    self.sensors_cached_bg_groups = bg_groups;
                    self.sensors_cached_snap_timestamp = snap.timestamp;
                    self.sensors_cached_search = search_q;
                    self.sensors_cached_sort = self.sensors_top_apps_sort.clone();
                    self.sensors_cached_sort_desc = self.sensors_top_apps_sort_desc;
                }

                let apps_groups = &self.sensors_cached_apps_groups;
                let bg_groups = &self.sensors_cached_bg_groups;

                if apps_groups.is_empty() && bg_groups.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        if !self.sensors_top_apps_visited {
                            ui.spinner();
                            ui.add_space(6.0);
                            ui.label(RichText::new("Loading system processes...").size(12.0).color(p.text_muted));
                        } else {
                            ui.label(RichText::new("No processes match the search query.").size(12.0).color(p.text_muted));
                        }
                    });
                    ui.add_space(24.0);
                    return;
                }

                // Responsive Name column width accounting for all column widths and gaps
                let avail_w = ui.available_width();
                let total_fixed_w = COL_CPU_W + COL_MEM_W + COL_DISK_W + COL_NET_W + COL_GPU_W + COL_POWER_W + 6.0 * COL_SPACING + 24.0;
                let col_name_w = (avail_w - total_fixed_w).max(180.0);
                let row_w = col_name_w + COL_CPU_W + COL_MEM_W + COL_DISK_W + COL_NET_W + COL_GPU_W + COL_POWER_W + 6.0 * COL_SPACING;

                // ═════════════════════════════════════════════════════════════
                // WINDOWS TASK MANAGER PROCESS TABLE
                // ═════════════════════════════════════════════════════════════
                // Keep the column headings fixed while only process rows scroll.
                ScrollArea::horizontal()
                    .id_salt("top_apps_h_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                let mut new_sort = None;
                let (header_rect, _) = ui.allocate_exact_size(egui::vec2(row_w, 36.0), egui::Sense::hover());
                let mut header_x = header_rect.left();
                for (width, title, key, total) in [
                            (col_name_w, "Name", "name", String::new()),
                            (COL_CPU_W, "CPU", "cpu", snap.cpu_usage_available.then(|| format!("{:.0}%", snap.cpu_global_usage)).unwrap_or_else(|| "—".to_string())),
                            (COL_MEM_W, "Memory", "mem", mem_total_pct.map_or_else(|| "—".to_string(), |pct| format!("{pct:.0}%"))),
                            (COL_DISK_W, "Disk", "disk", snap.total_disk_rates_available.then(|| format_tm_disk(total_disk_mb_s * 1024.0 * 1024.0)).unwrap_or_else(|| "—".to_string())),
                            (COL_NET_W, "Network", "net", sensor_network_rate(snap, snap.total_rx_rate_bps + snap.total_tx_rate_bps)),
                            (COL_GPU_W, "GPU", "gpu", total_gpu_pct.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".to_string())),
                            (COL_POWER_W, "Power", "power", "—".to_string()),
                        ] {
                            let cell_rect = egui::Rect::from_min_size(
                                egui::pos2(header_x, header_rect.top()),
                                egui::vec2(width, header_rect.height()),
                            );
                            let sortable = !matches!(key, "gpu" | "power");
                            let is_sel = sortable && self.sensors_top_apps_sort == key;
                            let arrow = if is_sel {
                                if self.sensors_top_apps_sort_desc { crate::lucide::ARROW_DOWN } else { crate::lucide::ARROW_UP }
                            } else {
                                ""
                            };
                            let align = if key == "name" { egui::Align2::LEFT_CENTER } else { egui::Align2::RIGHT_CENTER };
                            let text_x = if key == "name" { cell_rect.left() + 8.0 } else { cell_rect.right() - 8.0 };

                            if !total.is_empty() {
                                ui.painter().text(
                                    egui::pos2(text_x, cell_rect.top() + 9.0),
                                    align,
                                    total,
                                    egui::FontId::proportional(11.5),
                                    p.text_primary,
                                );
                            }
                            ui.painter().text(
                                egui::pos2(text_x, cell_rect.bottom() - 9.0),
                                align,
                                format!("{title} {arrow}"),
                                egui::FontId::proportional(10.0),
                                if is_sel { p.text_primary } else { p.text_secondary },
                            );

                            let response = ui.interact(
                                cell_rect,
                                ui.id().with(("top_apps_header", key)),
                                if sortable { egui::Sense::click() } else { egui::Sense::hover() },
                            );
                            let response = if !sortable {
                                response.on_hover_text(if key == "gpu" {
                                    "Per-process GPU utilization is unavailable"
                                } else {
                                    "Per-process power is unavailable"
                                })
                            } else { response };
                            if sortable && response.clicked() {
                                new_sort = Some(key);
                            }
                            header_x += width + COL_SPACING;
                        }
                if let Some(col) = new_sort {
                    if self.sensors_top_apps_sort == col {
                        self.sensors_top_apps_sort_desc = !self.sensors_top_apps_sort_desc;
                    } else {
                        self.sensors_top_apps_sort = col.to_string();
                        self.sensors_top_apps_sort_desc = col != "name";
                    }
                }

                ScrollArea::vertical()
                    .id_salt("top_apps_v_scroll")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                                egui::Grid::new("top_apps_task_manager_grid")
                                    .num_columns(7)
                                    .spacing(egui::vec2(COL_SPACING, 4.0))
                                    .show(ui, |ui| {
                                        // Keep grid geometry for off-screen rows without building their widgets.
                                        let skip_row = |ui: &mut Ui, height: f32| {
                                            for width in [col_name_w, COL_CPU_W, COL_MEM_W, COL_DISK_W, COL_NET_W, COL_GPU_W, COL_POWER_W] {
                                                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                                            }
                                            ui.end_row();
                                        };
                                        let mut render_group_row = |ui: &mut Ui, grp: &ProcessGroupSummary, is_app_sec: bool| {
                                            let is_multi = grp.children.len() > 1;
                                            let is_expanded = self.sensors_top_apps_expanded_groups.contains(&grp.key);
                                            let children = if is_multi && is_expanded { grp.children.len() } else { 0 };
                                            let height = 22.0 + children as f32 * 24.0;
                                            let group_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(row_w, height));
                                            if !ui.is_rect_visible(group_rect) {
                                                skip_row(ui, 22.0);
                                                for _ in 0..children { skip_row(ui, 20.0); }
                                                return;
                                            }
                                            let is_group_selected = self.sensors_top_apps_selected_pid == Some(grp.pids[0])
                                                || (!is_expanded && grp.pids.iter().any(|pid| self.sensors_top_apps_selected_pid == Some(*pid)));

                                            let norm_cpu = grp.sum_cpu / norm_factor;
                                            let cpu_fill = if grp.cpu_usage_available { get_tm_cpu_bg(norm_cpu) } else { Color32::TRANSPARENT };
                                            let mem_fill = get_tm_mem_bg(grp.sum_mem);
                                            let disk_fill = if grp.disk_rate_available { get_tm_disk_bg(grp.sum_disk_rate) } else { Color32::TRANSPARENT };

                                            let row_start_pos = ui.cursor().min;
                                            let row_rect = egui::Rect::from_min_size(row_start_pos, egui::vec2(row_w, 22.0));
                                            let is_row_hovered = ui.rect_contains_pointer(row_rect);

                                            if is_row_hovered {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            }

                                            let row_bg = if is_group_selected {
                                                accent_with_alpha(accent, 60)
                                            } else if is_row_hovered {
                                                Color32::from_rgba_unmultiplied(255, 255, 255, 8)
                                            } else {
                                                Color32::TRANSPARENT
                                            };

                                            let mut row_clicked = false;

                                            // 1. Name & Expander
                                            let name_cell_resp = ui.allocate_ui_with_layout(egui::vec2(col_name_w, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                ui.set_min_width(col_name_w);
                                                ui.set_max_width(col_name_w);
                                                let cell_rect = ui.max_rect();
                                                if row_bg != Color32::TRANSPARENT {
                                                    let bg_rect = egui::Rect::from_min_max(cell_rect.min, egui::pos2(cell_rect.max.x + COL_SPACING, cell_rect.max.y));
                                                    ui.painter().rect_filled(
                                                        bg_rect,
                                                        Rounding { nw: 4.0, sw: 4.0, ne: 0.0, se: 0.0 },
                                                        row_bg,
                                                    );
                                                    if is_group_selected {
                                                        ui.painter().rect_filled(
                                                            egui::Rect::from_min_max(cell_rect.min, egui::pos2(cell_rect.min.x + 3.0, cell_rect.max.y)),
                                                            Rounding { nw: 4.0, sw: 4.0, ne: 0.0, se: 0.0 },
                                                            p.accent,
                                                        );
                                                    }
                                                }

                                                let mut exp_rect = egui::Rect::NOTHING;
                                                if is_multi {
                                                    let (chev_rect, chev_resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                                                    exp_rect = chev_rect;
                                                    if chev_resp.hovered() {
                                                        ui.painter().rect_filled(
                                                            chev_rect,
                                                            Rounding::same(4.0),
                                                            Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                                                        );
                                                    }
                                                    let chev = if is_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                    ui.painter().text(
                                                        chev_rect.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        chev,
                                                        egui::FontId::proportional(11.0),
                                                        if chev_resp.hovered() { p.text_primary } else { p.text_muted },
                                                    );
                                                    let tooltip = if is_expanded {
                                                        format!("Collapse {} subprocesses", grp.children.len())
                                                    } else {
                                                        format!("View {} subprocesses (Click to expand)", grp.children.len())
                                                    };
                                                    let chev_resp = chev_resp.on_hover_text(tooltip);
                                                    if chev_resp.clicked() {
                                                        toggle_group_expand = Some(grp.key.clone());
                                                    }
                                                    if chev_resp.secondary_clicked() {
                                                        right_clicked_pid = Some(grp.pids[0]);
                                                    }
                                                } else {
                                                    ui.add_space(20.0);
                                                }

                                                let icon = if is_app_sec { crate::lucide::MONITOR } else { crate::lucide::CPU };
                                                ui.label(RichText::new(icon).size(11.0).color(if is_app_sec { p.accent } else { p.text_muted }));

                                                // Name format matching Image 2: "Affinity Photo (2)", "Microsoft Edge (29)"
                                                let disp_str = if is_multi {
                                                    format!("{} ({})", grp.display_name, grp.children.len())
                                                } else {
                                                    grp.display_name.clone()
                                                };

                                                let char_count = disp_str.chars().count();
                                                let short_disp = if char_count > 34 {
                                                    format!("{}…", disp_str.chars().take(32).collect::<String>())
                                                } else {
                                                    disp_str.clone()
                                                };

                                                let label = ui.label(
                                                    RichText::new(short_disp)
                                                        .size(11.5)
                                                        .strong()
                                                        .color(if is_group_selected { p.accent_light } else { p.text_primary }),
                                                );
                                                if char_count > 34 {
                                                    label.on_hover_text(&disp_str);
                                                }

                                                exp_rect
                                            });

                                            let expander_rect = name_cell_resp.inner;

                                            // 2. CPU (Heatmap tinted cell, strictly bounded, right aligned)
                                            let (cpu_rect, cpu_resp) = ui.allocate_exact_size(egui::vec2(COL_CPU_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                let bg_rect = egui::Rect::from_min_max(cpu_rect.min, egui::pos2(cpu_rect.max.x + COL_SPACING, cpu_rect.max.y));
                                                ui.painter().rect_filled(bg_rect, Rounding::ZERO, row_bg);
                                            }
                                            if cpu_fill != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(cpu_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, cpu_fill);
                                            }
                                            let cpu_txt = if grp.cpu_usage_available { format_tm_cpu(norm_cpu) } else { "—".to_string() };
                                            let cpu_color = if cpu_fill != Color32::TRANSPARENT {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            };
                                            ui.painter().text(
                                                egui::pos2(cpu_rect.right() - 8.0, cpu_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                &cpu_txt,
                                                egui::FontId::monospace(11.0),
                                                cpu_color,
                                            );
                                            let _ = cpu_resp.on_hover_text(if grp.cpu_usage_available { format!("{:.1}% Total System CPU ({:.1}% single-core raw)", norm_cpu, grp.sum_cpu) } else { "CPU usage unavailable for one or more processes".into() });

                                            // 3. Memory (Heatmap tinted cell, strictly bounded, right aligned)
                                            let (mem_rect, _mem_resp) = ui.allocate_exact_size(egui::vec2(COL_MEM_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                let bg_rect = egui::Rect::from_min_max(mem_rect.min, egui::pos2(mem_rect.max.x + COL_SPACING, mem_rect.max.y));
                                                ui.painter().rect_filled(bg_rect, Rounding::ZERO, row_bg);
                                            }
                                            if mem_fill != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(mem_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, mem_fill);
                                            }
                                            let mem_txt = format_tm_mem(grp.sum_mem);
                                            let mem_color = if mem_fill != Color32::TRANSPARENT {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            };
                                            ui.painter().text(
                                                egui::pos2(mem_rect.right() - 8.0, mem_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                &mem_txt,
                                                egui::FontId::monospace(11.0),
                                                mem_color,
                                            );

                                            // 4. Disk (Heatmap tinted cell, strictly bounded, right aligned)
                                            let (disk_rect, disk_resp) = ui.allocate_exact_size(egui::vec2(COL_DISK_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                let bg_rect = egui::Rect::from_min_max(disk_rect.min, egui::pos2(disk_rect.max.x + COL_SPACING, disk_rect.max.y));
                                                ui.painter().rect_filled(bg_rect, Rounding::ZERO, row_bg);
                                            }
                                            if disk_fill != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(disk_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, disk_fill);
                                            }
                                            let disk_txt = if grp.disk_rate_available { format_tm_disk(grp.sum_disk_rate) } else { "—".to_string() };
                                            let disk_color = if disk_fill != Color32::TRANSPARENT {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            };
                                            ui.painter().text(
                                                egui::pos2(disk_rect.right() - 8.0, disk_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                &disk_txt,
                                                egui::FontId::monospace(11.0),
                                                disk_color,
                                            );
                                            let _ = disk_resp.on_hover_text(if grp.disk_rate_available { "Process disk read + write rate" } else { "Waiting for a prior process I/O sample" });

                                            // 5. Network (Strictly bounded, right aligned)
                                            let (net_rect, net_resp) = ui.allocate_exact_size(egui::vec2(COL_NET_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                let bg_rect = egui::Rect::from_min_max(net_rect.min, egui::pos2(net_rect.max.x + COL_SPACING, net_rect.max.y));
                                                ui.painter().rect_filled(bg_rect, Rounding::ZERO, row_bg);
                                            }
                                            let net_txt = if grp.sum_net_bytes_available && grp.net_rate_available { crate::sensors::format_rate(grp.sum_net_rate) } else { "—".to_string() };
                                            let net_color = if grp.sum_net_rate > 1024.0 { Color32::from_rgb(203, 213, 225) } else { Color32::from_rgb(160, 165, 175) };
                                            ui.painter().text(
                                                egui::pos2(net_rect.right() - 8.0, net_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                &net_txt,
                                                egui::FontId::monospace(11.0),
                                                net_color,
                                            );
                                            let _ = net_resp.on_hover_text(if !snap.process_network_supported {
                                                "Per-process network counters are unavailable from this host's sensor collector.".to_string()
                                            } else if !grp.sum_net_bytes_available {
                                                "Per-process network counters are unavailable for one or more processes.".to_string()
                                            } else {
                                                format!("{} received + sent across visible TCP sockets{}", crate::sensors::format_bytes(grp.sum_net_bytes), if grp.net_rate_available { "" } else { " • waiting for a prior rate sample" })
                                            });

                                            // 6. GPU (Heatmap tinted cell, strictly bounded, right aligned)
                                            let (gpu_rect, gpu_resp) = ui.allocate_exact_size(egui::vec2(COL_GPU_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                let bg_rect = egui::Rect::from_min_max(gpu_rect.min, egui::pos2(gpu_rect.max.x + COL_SPACING, gpu_rect.max.y));
                                                ui.painter().rect_filled(bg_rect, Rounding::ZERO, row_bg);
                                            }
                                            let gpu_fill = get_tm_gpu_bg(grp.sum_gpu);
                                            if gpu_fill != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(gpu_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, gpu_fill);
                                            }
                                            let gpu_txt = "—";
                                            let gpu_color = if gpu_fill != Color32::TRANSPARENT {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            };
                                            ui.painter().text(
                                                egui::pos2(gpu_rect.right() - 8.0, gpu_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                &gpu_txt,
                                                egui::FontId::monospace(11.0),
                                                gpu_color,
                                            );
                                            let _ = gpu_resp.on_hover_text("Per-process GPU utilization is unavailable");

                                            // 7. Power (Heatmap tinted cell, strictly bounded, right aligned, right-most rounded corners)
                                            let (pwr_rect, pwr_resp) = ui.allocate_exact_size(egui::vec2(COL_POWER_W, 22.0), egui::Sense::hover());
                                            if row_bg != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(
                                                    pwr_rect,
                                                    Rounding { nw: 0.0, sw: 0.0, ne: 4.0, se: 4.0 },
                                                    row_bg,
                                                );
                                            }
                                            let pwr_fill = Color32::TRANSPARENT;
                                            if pwr_fill != Color32::TRANSPARENT {
                                                ui.painter().rect_filled(pwr_rect.shrink2(egui::vec2(2.0, 1.0)), 2.0, pwr_fill);
                                            }
                                            let pwr_rating = "—";
                                            let pwr_color = if pwr_fill != Color32::TRANSPARENT {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            };
                                            ui.painter().text(
                                                egui::pos2(pwr_rect.right() - 8.0, pwr_rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                pwr_rating,
                                                egui::FontId::proportional(11.0),
                                                pwr_color,
                                            );
                                            let _ = pwr_resp.on_hover_text("Per-process power is not available from this collector".to_string());

                                            let row_interactive_rect = if is_multi && expander_rect.width() > 0.0 {
                                                egui::Rect::from_min_max(
                                                    egui::pos2(expander_rect.max.x, row_rect.min.y),
                                                    row_rect.max,
                                                )
                                            } else {
                                                row_rect
                                            };

                                            let row_resp = ui.interact(row_interactive_rect, egui::Id::new("grp_row_full_clk").with(&grp.key), egui::Sense::click());
                                            if row_resp.clicked() && toggle_group_expand.is_none() {
                                                row_clicked = true;
                                            }
                                            if is_multi && row_resp.double_clicked() {
                                                toggle_group_expand = Some(grp.key.clone());
                                            }
                                            if row_resp.secondary_clicked() {
                                                right_clicked_pid = Some(grp.pids[0]);
                                            }
                                            row_resp.context_menu(|ui| {
                                                render_process_context_menu(ui, grp.pids[0], &grp.display_name, if grp.cpu_usage_available { norm_cpu } else { f32::NAN }, grp.sum_mem, &mut proc_action, &p, snap.os_name.to_ascii_lowercase().contains("windows"));
                                            });

                                            if row_clicked {
                                                select_target_pid = Some(grp.pids[0]);
                                            }
                                            ui.end_row();

                                            // Expanded children subprocesses
                                            if is_multi && is_expanded {
                                                for child in &grp.children {
                                                    let child_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(row_w, 20.0));
                                                    if !ui.is_rect_visible(child_rect) { skip_row(ui, 20.0); continue; }
                                                    let is_child_sel = self.sensors_top_apps_selected_pid == Some(child.pid);
                                                    let child_norm_cpu = child.cpu_usage / norm_factor;
                                                    let mut child_clicked = false;

                                                    let child_start_pos = ui.cursor().min;
                                                    let child_row_rect = egui::Rect::from_min_size(child_start_pos, egui::vec2(row_w, 20.0));
                                                    let is_child_hovered = ui.rect_contains_pointer(child_row_rect);

                                                    if is_child_hovered {
                                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                    }

                                                    let c_row_bg = if is_child_sel {
                                                        accent_with_alpha(accent, 60)
                                                    } else if is_child_hovered {
                                                        Color32::from_rgba_unmultiplied(255, 255, 255, 8)
                                                    } else {
                                                        Color32::TRANSPARENT
                                                    };

                                                    // Child Name
                                                    let _c_name_resp = ui.allocate_ui_with_layout(egui::vec2(col_name_w, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                        ui.set_min_width(col_name_w);
                                                        ui.set_max_width(col_name_w);
                                                        let cell_rect = ui.max_rect();
                                                        if c_row_bg != Color32::TRANSPARENT {
                                                            let bg_rect = egui::Rect::from_min_max(cell_rect.min, egui::pos2(cell_rect.max.x + COL_SPACING, cell_rect.max.y));
                                                            ui.painter().rect_filled(bg_rect, Rounding { nw: 4.0, sw: 4.0, ne: 0.0, se: 0.0 }, c_row_bg);
                                                            if is_child_sel {
                                                                ui.painter().rect_filled(
                                                                    egui::Rect::from_min_max(cell_rect.min, egui::pos2(cell_rect.min.x + 3.0, cell_rect.max.y)),
                                                                    Rounding { nw: 4.0, sw: 4.0, ne: 0.0, se: 0.0 },
                                                                    p.accent,
                                                                );
                                                            }
                                                        }
                                                        ui.add_space(36.0); // Indent under parent
                                                        let child_txt = format!("↳ PID {}", child.pid);
                                                        ui.label(RichText::new(child_txt).size(10.5).monospace().color(p.text_primary));
                                                    });

                                                    // Child CPU
                                                    let (c_cpu_rect, _c_cpu_resp) = ui.allocate_exact_size(egui::vec2(COL_CPU_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        let bg_rect = egui::Rect::from_min_max(c_cpu_rect.min, egui::pos2(c_cpu_rect.max.x + COL_SPACING, c_cpu_rect.max.y));
                                                        ui.painter().rect_filled(bg_rect, Rounding::ZERO, c_row_bg);
                                                    }
                                                    ui.painter().text(
                                                        egui::pos2(c_cpu_rect.right() - 8.0, c_cpu_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        if child.cpu_usage_available { format_tm_cpu(child_norm_cpu) } else { "—".to_string() },
                                                        egui::FontId::monospace(10.5),
                                                        Color32::from_rgb(160, 165, 175),
                                                    );

                                                    // Child Memory
                                                    let (c_mem_rect, _c_mem_resp) = ui.allocate_exact_size(egui::vec2(COL_MEM_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        let bg_rect = egui::Rect::from_min_max(c_mem_rect.min, egui::pos2(c_mem_rect.max.x + COL_SPACING, c_mem_rect.max.y));
                                                        ui.painter().rect_filled(bg_rect, Rounding::ZERO, c_row_bg);
                                                    }
                                                    ui.painter().text(
                                                        egui::pos2(c_mem_rect.right() - 8.0, c_mem_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        format_tm_mem(child.memory_bytes),
                                                        egui::FontId::monospace(10.5),
                                                        Color32::from_rgb(160, 165, 175),
                                                    );

                                                    // Child Disk
                                                    let (c_disk_rect, c_disk_resp) = ui.allocate_exact_size(egui::vec2(COL_DISK_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        let bg_rect = egui::Rect::from_min_max(c_disk_rect.min, egui::pos2(c_disk_rect.max.x + COL_SPACING, c_disk_rect.max.y));
                                                        ui.painter().rect_filled(bg_rect, Rounding::ZERO, c_row_bg);
                                                    }
                                                    ui.painter().text(
                                                        egui::pos2(c_disk_rect.right() - 8.0, c_disk_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        if child.disk_rate_available { format_tm_disk(child.disk_rate_bps) } else { "—".to_string() },
                                                        egui::FontId::monospace(10.5),
                                                        if child.disk_rate_available { p.text_secondary } else { p.text_muted },
                                                    );
                                                    let _ = c_disk_resp.on_hover_text(if child.disk_rate_available { "Process disk read + write rate" } else { "Waiting for a prior process I/O sample" });

                                                    // Child Network
                                                    let (c_net_rect, c_net_resp) = ui.allocate_exact_size(egui::vec2(COL_NET_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        let bg_rect = egui::Rect::from_min_max(c_net_rect.min, egui::pos2(c_net_rect.max.x + COL_SPACING, c_net_rect.max.y));
                                                        ui.painter().rect_filled(bg_rect, Rounding::ZERO, c_row_bg);
                                                    }
                                                    let c_net_txt = if child.net_counters_available && child.net_rate_available { crate::sensors::format_rate(child.net_rate_bps) } else { "—".to_string() };
                                                    let c_net_color = if child.net_rate_bps > 1024.0 {
                                                        Color32::from_rgb(203, 213, 225)
                                                    } else {
                                                        Color32::from_rgb(140, 145, 155)
                                                    };
                                                    ui.painter().text(
                                                        egui::pos2(c_net_rect.right() - 8.0, c_net_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        &c_net_txt,
                                                        egui::FontId::monospace(10.5),
                                                        c_net_color,
                                                    );
                                                    let _ = c_net_resp.on_hover_text(if !snap.process_network_supported {
                                                        "Per-process network counters are unavailable from this host's sensor collector.".to_string()
                                                    } else if !child.net_counters_available {
                                                        "Per-process network counters are unavailable for this process.".to_string()
                                                    } else {
                                                        format!("{} received + sent across visible TCP sockets{}", crate::sensors::format_bytes(child.net_rx_bytes.saturating_add(child.net_tx_bytes)), if child.net_rate_available { "" } else { " • waiting for a prior rate sample" })
                                                    });

                                                    // Child GPU
                                                    let (c_gpu_rect, c_gpu_resp) = ui.allocate_exact_size(egui::vec2(COL_GPU_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        let bg_rect = egui::Rect::from_min_max(c_gpu_rect.min, egui::pos2(c_gpu_rect.max.x + COL_SPACING, c_gpu_rect.max.y));
                                                        ui.painter().rect_filled(bg_rect, Rounding::ZERO, c_row_bg);
                                                    }
                                                    let c_gpu_txt = "—";
                                                    ui.painter().text(
                                                        egui::pos2(c_gpu_rect.right() - 8.0, c_gpu_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        &c_gpu_txt,
                                                        egui::FontId::monospace(10.5),
                                                        p.text_muted,
                                                    );
                                                    let _ = c_gpu_resp.on_hover_text("Per-process GPU utilization is unavailable from this collector");

                                                    // Child Power
                                                    let (c_pwr_rect, c_pwr_resp) = ui.allocate_exact_size(egui::vec2(COL_POWER_W, 20.0), egui::Sense::hover());
                                                    if c_row_bg != Color32::TRANSPARENT {
                                                        ui.painter().rect_filled(c_pwr_rect, Rounding { nw: 0.0, sw: 0.0, ne: 4.0, se: 4.0 }, c_row_bg);
                                                    }
                                                    let c_pwr_rating = "—";
                                                    ui.painter().text(
                                                        egui::pos2(c_pwr_rect.right() - 8.0, c_pwr_rect.center().y),
                                                        egui::Align2::RIGHT_CENTER,
                                                        c_pwr_rating,
                                                        egui::FontId::proportional(10.5),
                                                        Color32::from_rgb(140, 145, 155),
                                                    );
                                                    let _ = c_pwr_resp.on_hover_text("Per-process power is not available from this collector");

                                                    let c_row_resp = ui.interact(child_row_rect, egui::Id::new("child_row_full_clk").with(child.pid), egui::Sense::click());
                                                    if c_row_resp.clicked() {
                                                        child_clicked = true;
                                                    }
                                                    if c_row_resp.secondary_clicked() {
                                                        right_clicked_pid = Some(child.pid);
                                                    }
                                                    c_row_resp.context_menu(|ui| {
                                                        render_process_context_menu(ui, child.pid, &grp.display_name, if child.cpu_usage_available { child_norm_cpu } else { f32::NAN }, child.memory_bytes, &mut proc_action, &p, snap.os_name.to_ascii_lowercase().contains("windows"));
                                                    });

                                                    if child_clicked {
                                                        select_target_pid = Some(child.pid);
                                                    }
                                                    ui.end_row();
                                                }
                                            }
                                        };

                                        // ═════════════════════════════════════
                                        // SECTION 1: APPS (Image 2)
                                        // ═════════════════════════════════════
                                        if !apps_groups.is_empty() {
                                            ui.allocate_ui_with_layout(egui::vec2(col_name_w, 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                let chev = if self.sensors_top_apps_section_apps_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                let btn = egui::Button::new(
                                                    RichText::new(format!("{} Likely applications ({})", chev, apps_groups.len()))
                                                        .size(12.5)
                                                        .strong()
                                                        .color(p.text_primary),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                                if ui.add(btn).clicked() {
                                                    self.sensors_top_apps_section_apps_expanded = !self.sensors_top_apps_section_apps_expanded;
                                                }
                                            });
                                            for _ in 0..6 {
                                                ui.label("");
                                            }
                                            ui.end_row();

                                            if self.sensors_top_apps_section_apps_expanded {
                                                for grp in apps_groups {
                                                    render_group_row(ui, grp, true);
                                                }
                                            }
                                        }

                                        // ═════════════════════════════════════
                                        // SECTION 2: BACKGROUND PROCESSES (Image 2)
                                        // ═════════════════════════════════════
                                        if !bg_groups.is_empty() {
                                            ui.allocate_ui_with_layout(egui::vec2(col_name_w, 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                let chev = if self.sensors_top_apps_section_bg_expanded { crate::lucide::CHEVRON_DOWN } else { crate::lucide::CHEVRON_RIGHT };
                                                let btn = egui::Button::new(
                                                    RichText::new(format!("{} {} ({})", chev, if cfg!(windows) { "Processes" } else { "Other processes" }, bg_groups.len()))
                                                        .size(12.5)
                                                        .strong()
                                                        .color(p.text_secondary),
                                                )
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE);
                                                if ui.add(btn).clicked() {
                                                    self.sensors_top_apps_section_bg_expanded = !self.sensors_top_apps_section_bg_expanded;
                                                }
                                            });
                                            for _ in 0..6 {
                                                ui.label("");
                                            }
                                            ui.end_row();

                                            if self.sensors_top_apps_section_bg_expanded {
                                                for grp in bg_groups {
                                                    render_group_row(ui, grp, false);
                                                }
                                            }
                                        }
                                    });
                    });
                });
            });

        // Global shortcuts for selected process when not typing into a search text field
        if let Some(sel_pid) = self.sensors_top_apps_selected_pid {
            let wants_keyboard = ui.ctx().wants_keyboard_input();
            if !wants_keyboard {
                let alt = ui.ctx().input(|i| i.modifiers.alt);
                let ctrl = ui.ctx().input(|i| i.modifiers.ctrl || i.modifiers.command);
                let sel_proc = snap.top_apps.iter().find(|p| p.pid == sel_pid);
                let sel_name = sel_proc.map(|p| p.name.clone()).unwrap_or_else(|| format!("PID {}", sel_pid));
                let sel_cpu = sel_proc.filter(|p| p.cpu_usage_available).map(|p| p.cpu_usage).unwrap_or(f32::NAN);
                let sel_mem = sel_proc.map(|p| p.memory_bytes).unwrap_or(0);

                let windows_process_host = snap.os_name.to_ascii_lowercase().contains("windows");
                if windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::T)) {
                    proc_action = Some(ProcAction::Terminate { pid: sel_pid });
                } else if windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::K)) {
                    proc_action = Some(ProcAction::ForceStop { pid: sel_pid });
                } else if !windows_process_host && alt && ui.ctx().input(|i| i.key_pressed(Key::Enter)) {
                    proc_action = Some(ProcAction::Properties { pid: sel_pid, name: sel_name, cpu: sel_cpu, mem: sel_mem });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::M)) {
                    proc_action = Some(ProcAction::MemoryMaps { pid: sel_pid, name: sel_name });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::O)) {
                    proc_action = Some(ProcAction::OpenFiles { pid: sel_pid, name: sel_name });
                } else if !windows_process_host && alt && ui.ctx().input(|i| i.key_pressed(Key::S)) {
                    proc_action = Some(ProcAction::SetAffinity { pid: sel_pid, name: sel_name });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::S)) {
                    proc_action = Some(ProcAction::Stop { pid: sel_pid });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::C)) {
                    proc_action = Some(ProcAction::Resume { pid: sel_pid });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::T)) {
                    proc_action = Some(ProcAction::Terminate { pid: sel_pid });
                } else if !windows_process_host && ctrl && ui.ctx().input(|i| i.key_pressed(Key::K)) {
                    proc_action = Some(ProcAction::ForceStop { pid: sel_pid });
                }
            }
        }

        // Apply state updates outside borrow closures
        if let Some(key) = toggle_group_expand {
            if self.sensors_top_apps_expanded_groups.contains(&key) {
                self.sensors_top_apps_expanded_groups.remove(&key);
            } else {
                self.sensors_top_apps_expanded_groups.insert(key);
            }
        }

        if let Some(pid) = right_clicked_pid {
            self.sensors_top_apps_selected_pid = Some(pid);
        } else if let Some(pid) = select_target_pid {
            if self.sensors_top_apps_selected_pid == Some(pid) {
                self.sensors_top_apps_selected_pid = None; // toggle deselect on left-click
            } else {
                self.sensors_top_apps_selected_pid = Some(pid);
            }
        }

        if right_clicked_pid.is_some() || select_target_pid.is_some() {
            self.sensors_top_apps_selected_start = self.sensors_top_apps_selected_pid.and_then(|pid| snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker));
        }

        if let Some(pid) = kill_target_pid {
            proc_action = Some(ProcAction::Terminate { pid });
        }

        if let Some(action) = proc_action {
            #[cfg(windows)]
            if host_id.is_none() {
                match action {
                    ProcAction::ForceStop { pid } => {
                        let start = snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker);
                        self.sensors_top_apps_status_msg = Some(match crate::procmgr::force_stop_windows_process(pid, start) {
                            Ok(()) => (format!("Stopped process {pid}"), false, Instant::now()),
                            Err(error) => (format!("Failed to stop process {pid}: {error}"), true, Instant::now()),
                        });
                    }
                    ProcAction::Terminate { pid } => {
                        let start = snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker);
                        self.sensors_top_apps_status_msg = Some(match crate::procmgr::graceful_stop_windows_process(pid, start) {
                            Ok(()) => (format!("Stop requested for process {pid}"), false, Instant::now()),
                            Err(error) => (format!("Failed to stop process {pid}: {error}"), true, Instant::now()),
                        });
                    }
                    _ => self.sensors_top_apps_status_msg = Some(("Linux process inspection, affinity, priority and pause actions are unavailable locally. Use Windows Task Manager.".into(), true, Instant::now())),
                }
                return;
            }
            match action {
                ProcAction::Properties { pid, name, cpu, mem } => {
                    self.proc_properties_modal_open = true;
                    self.proc_properties_pid = pid;
                    self.proc_properties_name = name.clone();
                    self.proc_properties_data = None;
                    self.proc_properties_error = None;
                    self.proc_properties_loading = true;

                    if let Some(hid) = host_id {
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            let rx = self.proc_details_receiver.clone();
                            let ctx_clone = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let res = crate::procmgr::fetch_remote_process_details(&host, pid, &name, cpu, mem);
                                *rx.lock().unwrap() = Some((pid, res));
                                ctx_clone.request_repaint();
                            });
                        } else {
                            self.proc_properties_loading = false;
                            self.proc_properties_error = Some("SSH Host not found".to_string());
                        }
                    } else {
                        let rx = self.proc_details_receiver.clone();
                        let ctx_clone = ui.ctx().clone();
                        let name_clone = name.clone();
                        std::thread::spawn(move || {
                            let details = crate::procmgr::fetch_local_process_details(pid, &name_clone, cpu, mem);
                            *rx.lock().unwrap() = Some((pid, Ok(details)));
                            ctx_clone.request_repaint();
                        });
                    }
                }
                ProcAction::MemoryMaps { pid, name } => {
                    self.proc_maps_modal_open = true;
                    self.proc_maps_pid = pid;
                    self.proc_maps_name = name;
                    self.proc_maps_data = None;
                    self.proc_maps_error = None;
                    self.proc_maps_search.clear();
                    self.proc_maps_loading = true;

                    if let Some(hid) = host_id {
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            let rx = self.proc_maps_receiver.clone();
                            let ctx_clone = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let res = crate::procmgr::fetch_remote_memory_maps(&host, pid);
                                *rx.lock().unwrap() = Some((pid, res));
                                ctx_clone.request_repaint();
                            });
                        } else {
                            self.proc_maps_loading = false;
                            self.proc_maps_error = Some("SSH Host not found".to_string());
                        }
                    } else {
                        let rx = self.proc_maps_receiver.clone();
                        let ctx_clone = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let res = crate::procmgr::fetch_local_memory_maps(pid);
                            *rx.lock().unwrap() = Some((pid, res));
                            ctx_clone.request_repaint();
                        });
                    }
                }
                ProcAction::OpenFiles { pid, name } => {
                    self.proc_open_files_modal_open = true;
                    self.proc_open_files_pid = pid;
                    self.proc_open_files_name = name;
                    self.proc_open_files_data = None;
                    self.proc_open_files_error = None;
                    self.proc_open_files_search.clear();
                    self.proc_open_files_loading = true;

                    if let Some(hid) = host_id {
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            let rx = self.proc_open_files_receiver.clone();
                            let ctx_clone = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let res = crate::procmgr::fetch_remote_open_files(&host, pid);
                                *rx.lock().unwrap() = Some((pid, res));
                                ctx_clone.request_repaint();
                            });
                        } else {
                            self.proc_open_files_loading = false;
                            self.proc_open_files_error = Some("SSH Host not found".to_string());
                        }
                    } else {
                        let rx = self.proc_open_files_receiver.clone();
                        let ctx_clone = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let res = crate::procmgr::fetch_local_open_files(pid);
                            *rx.lock().unwrap() = Some((pid, res));
                            ctx_clone.request_repaint();
                        });
                    }
                }
                ProcAction::SetPriority { pid, nice } => {
                    let expected_start = snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker);
                    if let Some(hid) = host_id {
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            let host_name = host.name.clone();
                            let results = self.remote_process_results.clone();
                            let ctx = ui.ctx().clone();
                            self.sensors_top_apps_status_msg = Some((format!("Setting priority for PID {} on {}…", pid, host_name), false, Instant::now()));
                            std::thread::spawn(move || {
                                let result = crate::procmgr::set_remote_priority(&host, pid, nice, expected_start);
                                let message = match result {
                                    Ok(()) => (format!("Set priority (nice {}) for PID {} on {}", nice, pid, host_name), false),
                                    Err(error) => (format!("Failed to set priority for PID {} on {}: {}", pid, host_name, error), true),
                                };
                                results.lock().unwrap().push_back(message);
                                ctx.request_repaint();
                            });
                        }
                    } else {
                        match crate::procmgr::set_local_priority(pid, nice, expected_start) {
                            Ok(_) => {
                                self.sensors_top_apps_status_msg = Some((format!("Set priority (nice {}) for PID {}", nice, pid), false, Instant::now()));
                            }
                            Err(e) => {
                                self.sensors_top_apps_status_msg = Some((format!("Failed to set priority: {}", e), true, Instant::now()));
                            }
                        }
                    }
                }
                ProcAction::SetAffinity { pid, name } => {
                    let total_cores = snap.cpu_cores_logical.max(1);
                    self.proc_affinity_modal_open = true;
                    self.proc_affinity_pid = pid;
                    self.proc_affinity_start_marker = snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker);
                    self.proc_affinity_name = name;
                    self.proc_affinity_host_id = host_id.map(|s| s.to_string());
                    self.proc_affinity_total_cores = total_cores;
                    self.proc_affinity_cores = vec![true; total_cores];
                    self.proc_affinity_error = None;
                    self.proc_affinity_loading = true;

                    if let Some(hid) = host_id {
                        if let Some(host) = self.ssh_hosts.iter().find(|h| h.id == hid).cloned() {
                            let rx = self.proc_affinity_receiver.clone();
                            let ctx_clone = ui.ctx().clone();
                            std::thread::spawn(move || {
                                let res = crate::procmgr::get_remote_affinity(&host, pid);
                                *rx.lock().unwrap() = Some((pid, res));
                                ctx_clone.request_repaint();
                            });
                        }
                    } else {
                        match crate::procmgr::get_local_affinity(pid) {
                            Ok(cores) => {
                                let mut mask = vec![false; total_cores];
                                for c in cores {
                                    if c < total_cores {
                                        mask[c] = true;
                                    }
                                }
                                if mask.iter().any(|&b| b) {
                                    self.proc_affinity_cores = mask;
                                }
                                self.proc_affinity_loading = false;
                            }
                            Err(e) => {
                                self.proc_affinity_error = Some(e);
                                self.proc_affinity_loading = false;
                            }
                        }
                    }
                }
                ProcAction::Stop { pid } => {
                    self.execute_proc_signal(pid, "STOP", "Sent SIGSTOP to", host_id, snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker), ui.ctx());
                }
                ProcAction::Resume { pid } => {
                    self.execute_proc_signal(pid, "CONT", "Sent SIGCONT to", host_id, snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker), ui.ctx());
                }
                ProcAction::Terminate { pid } => {
                    self.execute_proc_signal(pid, "TERM", "Sent SIGTERM to", host_id, snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker), ui.ctx());
                }
                ProcAction::ForceStop { pid } => {
                    self.execute_proc_signal(pid, "KILL", "Sent SIGKILL to", host_id, snap.top_apps.iter().find(|process| process.pid == pid).and_then(|process| process.start_marker), ui.ctx());
                }
            }
        }
    }
}
