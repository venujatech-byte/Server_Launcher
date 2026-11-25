use super::actions::format_bytes;
use super::model::{AppAction, AppEntry, AppFilter, AppSortCol, AppSource, ClearDataMode, RepoEntry};
use super::AppsManagerState;
use crate::theme::ThemePalette;
use eframe::egui::{self, Color32, Frame, RichText, Rounding, Stroke, Ui};

const ROW_HEIGHT: f32 = 34.0;

fn source_badge_colors(source: AppSource, p: &ThemePalette) -> (Color32, Color32) {
    match source {
        AppSource::Flatpak => (p.info_subtle, p.info),
        AppSource::Desktop | AppSource::Appx => (p.accent_subtle, p.accent),
        AppSource::ChromePwa => (p.success_subtle, p.success),
        AppSource::DnfRpm | AppSource::AptDpkg | AppSource::Pacman | AppSource::Snap => {
            (p.bg_button, p.text_secondary)
        }
        AppSource::Win32 | AppSource::Winget | AppSource::Scoop | AppSource::Choco => {
            (p.warning_subtle, p.warning)
        }
    }
}

pub fn render_apps_page(state: &mut AppsManagerState, ui: &mut Ui, p: &ThemePalette) {
    let ctx = ui.ctx().clone();
    if !state.initialized {
        state.initialized = true;
        state.trigger_discovery(&ctx);
    }
    state.poll_events(&ctx);

    let mut pending_action: Option<AppAction> = None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        render_top_bar(state, ui, p, &ctx, &mut pending_action);
        render_filter_and_sort_bar(state, ui, p);

        let drawer_h = if state.log_drawer_open { 165.0 } else { 32.0 };
        let main_h = (ui.available_height() - drawer_h - 8.0).max(180.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), main_h),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.set_height(main_h);
                if state.filter == AppFilter::Repos {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), main_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            render_repos_table(state, ui, p, main_h);
                        },
                    );
                } else {
                    let has_inspector = state.selected_key.is_some();
                    let inspector_w = if has_inspector { 340.0 } else { 0.0 };
                    let gap = if has_inspector { 10.0 } else { 0.0 };
                    let table_w = (ui.available_width() - inspector_w - gap).max(320.0);

                    ui.allocate_ui_with_layout(
                        egui::vec2(table_w, main_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            render_apps_table(state, ui, p, table_w, main_h, &mut pending_action);
                        },
                    );

                    if has_inspector {
                        ui.add_space(gap);
                        ui.allocate_ui_with_layout(
                            egui::vec2(inspector_w, main_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                render_inspector_panel(
                                    state,
                                    ui,
                                    p,
                                    inspector_w,
                                    main_h,
                                    &mut pending_action,
                                );
                            },
                        );
                    }
                }
            },
        );

        render_task_drawer(state, ui, p);
    });

    render_confirmation_modals(state, &ctx, p, &mut pending_action);

    if let Some(action) = pending_action {
        state.dispatch_action(action, &ctx);
    }
}

fn render_top_bar(
    state: &mut AppsManagerState,
    ui: &mut Ui,
    p: &ThemePalette,
    ctx: &egui::Context,
    pending_action: &mut Option<AppAction>,
) {
    Frame::none()
        .fill(p.bg_card)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} Local Apps & Packages", crate::lucide::LAYOUT_GRID))
                        .size(15.0)
                        .strong()
                        .color(p.text_primary),
                );

                let count_label = if state.filter == AppFilter::Repos {
                    format!("{} repos", state.repos.len())
                } else {
                    format!("{} of {}", state.filtered_indices.len(), state.apps.len())
                };
                Frame::none()
                    .fill(p.bg_button)
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(count_label).size(11.0).color(p.text_muted));
                    });

                if state.loading || state.checking_updates {
                    ui.spinner();
                }
                if !state.status_text.is_empty() {
                    ui.label(RichText::new(&state.status_text).size(11.5).color(p.text_muted));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let updatable: Vec<AppEntry> = state
                        .apps
                        .iter()
                        .filter(|a| a.update_available.is_some() && a.can_update)
                        .cloned()
                        .collect();

                    if !updatable.is_empty() {
                        let btn = egui::Button::new(
                            RichText::new(format!("{} Update All ({})", crate::lucide::DOWNLOAD, updatable.len()))
                                .size(11.5)
                                .strong()
                                .color(p.accent_text),
                        )
                        .fill(p.success)
                        .rounding(Rounding::same(8.0));
                        if ui.add_enabled(!state.task_running, btn).clicked() {
                            *pending_action = Some(AppAction::UpdateAll(updatable));
                        }
                    }

                    let check_btn = egui::Button::new(
                        RichText::new(format!("{} Check Updates", crate::lucide::SPARKLES))
                            .size(11.5)
                            .color(p.text_primary),
                    )
                    .fill(p.bg_button)
                    .rounding(Rounding::same(8.0));
                    if ui.add_enabled(!state.checking_updates && !state.loading, check_btn).clicked() {
                        state.trigger_check_updates(ctx);
                    }

                    let refresh_btn = egui::Button::new(
                        RichText::new(format!("{} Refresh", crate::lucide::REFRESH_CW))
                            .size(11.5)
                            .color(p.text_primary),
                    )
                    .fill(p.bg_button)
                    .rounding(Rounding::same(8.0));
                    if ui.add_enabled(!state.loading, refresh_btn).clicked() {
                        state.trigger_discovery(ctx);
                    }

                    Frame::none()
                        .fill(p.bg_input)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::lucide::SEARCH).size(12.0).color(p.text_muted));
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut state.search_query)
                                        .hint_text("Filter by name, ID, or description…")
                                        .desired_width(220.0)
                                        .frame(false),
                                );
                                if resp.changed() {
                                    state.mark_filter_dirty();
                                }
                                if !state.search_query.is_empty()
                                    && ui
                                        .add(
                                            egui::Button::new(RichText::new(crate::lucide::X).size(11.0).color(p.text_muted))
                                                .frame(false),
                                        )
                                        .clicked()
                                {
                                    state.search_query.clear();
                                    state.mark_filter_dirty();
                                }
                            });
                        });
                });
            });
        });
}

fn render_filter_and_sort_bar(state: &mut AppsManagerState, ui: &mut Ui, p: &ThemePalette) {
    ui.horizontal(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for &f in AppFilter::all() {
                let active = state.filter == f;
                let count = if f == AppFilter::Repos {
                    state.repos.len()
                } else {
                    state.apps.iter().filter(|a| a.source.matches_filter(f)).count()
                };
                let bg = if active { p.accent } else { p.bg_button };
                let fg = if active { p.accent_text } else { p.text_secondary };
                let btn = egui::Button::new(
                    RichText::new(format!("{} ({})", f.label(), count))
                        .size(11.5)
                        .strong()
                        .color(fg),
                )
                .fill(bg)
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(14.0));
                if ui.add(btn).clicked() && state.filter != f {
                    state.filter = f;
                    state.mark_filter_dirty();
                }
            }
        });

        if state.filter != AppFilter::Repos {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let dir_lbl = if state.sort_desc { "Desc ▼" } else { "Asc ▲" };
                let dir_btn = egui::Button::new(RichText::new(dir_lbl).size(11.0).color(p.text_primary))
                    .fill(p.bg_button)
                    .rounding(Rounding::same(8.0));
                if ui.add(dir_btn).on_hover_text("Toggle sort direction").clicked() {
                    state.sort_desc = !state.sort_desc;
                    state.mark_filter_dirty();
                }

                for &col in AppSortCol::all().iter().rev() {
                    let active = state.sort_col == col;
                    let btn = egui::Button::new(
                        RichText::new(col.label())
                            .size(11.0)
                            .color(if active { p.accent_text } else { p.text_secondary }),
                    )
                    .fill(if active { p.accent } else { p.bg_button })
                    .rounding(Rounding::same(8.0));
                    if ui.add(btn).clicked() {
                        if state.sort_col == col {
                            state.sort_desc = !state.sort_desc;
                        } else {
                            state.sort_col = col;
                            state.sort_desc = matches!(col, AppSortCol::Size | AppSortCol::InstalledDate);
                        }
                        state.mark_filter_dirty();
                    }
                }
                ui.label(RichText::new("Sort:").size(11.0).color(p.text_muted));
            });
        }
    });
}

fn cell_ui(ui: &mut Ui, width: f32, height: f32, align_right: bool, add_contents: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let layout = if align_right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add_contents(&mut child);
}

fn render_apps_table(
    state: &mut AppsManagerState,
    ui: &mut Ui,
    p: &ThemePalette,
    width: f32,
    height: f32,
    pending_action: &mut Option<AppAction>,
) {
    Frame::none()
        .fill(p.bg_card)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            let inner_w = (width - 20.0).max(260.0);
            ui.set_width(inner_w);
            ui.set_height((height - 16.0).max(120.0));

            let col_source_w = 96.0;
            let col_ver_w = 135.0;
            let col_size_w = 74.0;
            let col_date_w = 88.0;
            let col_actions_w = 112.0;
            let col_name_w = (inner_w - col_source_w - col_ver_w - col_size_w - col_date_w - col_actions_w - 28.0)
                .max(120.0);

            // Header row
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                sort_header(state, ui, p, "Name", AppSortCol::Name, col_name_w);
                sort_header(state, ui, p, "Source", AppSortCol::Source, col_source_w);
                sort_header(state, ui, p, "Version", AppSortCol::Version, col_ver_w);
                sort_header(state, ui, p, "Size", AppSortCol::Size, col_size_w);
                sort_header(state, ui, p, "Installed", AppSortCol::InstalledDate, col_date_w);
                cell_ui(ui, col_actions_w, 22.0, true, |ui| {
                    ui.label(RichText::new("Actions").size(11.0).strong().color(p.text_muted));
                });
            });
            ui.separator();

            let total_rows = state.filtered_indices.len();
            if total_rows == 0 {
                ui.centered_and_justified(|ui| {
                    let msg = if state.loading {
                        "Scanning local applications and packages…"
                    } else {
                        "No matching applications or packages found."
                    };
                    ui.label(RichText::new(msg).size(13.0).color(p.text_muted));
                });
                return;
            }

            let scroll_h = ui.available_height().max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("apps_virtual_table")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show_rows(ui, ROW_HEIGHT, total_rows, |ui, row_range| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for idx_pos in row_range {
                        let app_idx = state.filtered_indices[idx_pos];
                        let app = state.apps[app_idx].clone();
                        let is_selected = state
                            .selected_key
                            .as_ref()
                            .is_some_and(|(id, src)| id == &app.id && *src == app.source);

                        let row_bg = if is_selected {
                            p.bg_card_active
                        } else if idx_pos % 2 == 1 {
                            p.bg_subtle
                        } else {
                            Color32::TRANSPARENT
                        };

                        let row_w = ui.available_width();
                        let (row_rect, _) =
                            ui.allocate_exact_size(egui::vec2(row_w, ROW_HEIGHT - 2.0), egui::Sense::hover());
                        if row_bg != Color32::TRANSPARENT {
                            ui.painter().rect_filled(row_rect, Rounding::same(6.0), row_bg);
                        }

                        // Left info area is clickable to select row (never overlaps action buttons!)
                        let info_w = (row_w - col_actions_w - 8.0).max(120.0);
                        let info_rect = egui::Rect::from_min_size(
                            row_rect.min,
                            egui::vec2(info_w, row_rect.height()),
                        );
                        let info_resp = ui.interact(
                            info_rect,
                            egui::Id::new(("app_row_sel", idx_pos)),
                            egui::Sense::click(),
                        );
                        if info_resp.hovered() && !is_selected {
                            ui.painter().rect_filled(row_rect, Rounding::same(6.0), p.bg_card_hover);
                        }
                        if info_resp.clicked() {
                            state.selected_key = Some((app.id.clone(), app.source));
                            *pending_action = Some(AppAction::ResolveDetails(app.clone()));
                        }

                        let mut row_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(row_rect.shrink2(egui::vec2(4.0, 2.0)))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        row_ui.spacing_mut().item_spacing.x = 4.0;

                        // Name + Update badge
                        cell_ui(&mut row_ui, col_name_w, 26.0, false, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&app.name).size(12.5).strong().color(p.text_primary),
                                )
                                .truncate(),
                            );
                            if let Some(ref new_v) = app.update_available {
                                Frame::none()
                                    .fill(p.success_subtle)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(format!("-> {}", new_v))
                                                .size(10.0)
                                                .color(p.success),
                                        );
                                    });
                            }
                        });

                        // Source badge
                        cell_ui(&mut row_ui, col_source_w, 26.0, false, |ui| {
                            let (sbg, sfg) = source_badge_colors(app.source, p);
                            Frame::none()
                                .fill(sbg)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(app.source.label()).size(10.5).color(sfg));
                                });
                        });

                        // Version
                        cell_ui(&mut row_ui, col_ver_w, 26.0, false, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&app.version).size(11.5).color(p.text_secondary),
                                )
                                .truncate(),
                            );
                        });

                        // Size
                        cell_ui(&mut row_ui, col_size_w, 26.0, false, |ui| {
                            ui.label(RichText::new(&app.size_display).size(11.5).color(p.text_muted));
                        });

                        // Installed Date
                        cell_ui(&mut row_ui, col_date_w, 26.0, false, |ui| {
                            let d = app.installed_date.as_deref().unwrap_or("—");
                            ui.label(RichText::new(d).size(11.0).color(p.text_muted));
                        });

                        // Actions (isolated from row click!)
                        cell_ui(&mut row_ui, col_actions_w, 26.0, true, |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if app.can_uninstall {
                                let btn = egui::Button::new(
                                    RichText::new(crate::lucide::TRASH).size(12.0).color(p.danger),
                                )
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                                if ui
                                    .add_enabled(!state.task_running, btn)
                                    .on_hover_text("Uninstall")
                                    .clicked()
                                {
                                    state.confirm_uninstall = Some(app.clone());
                                }
                            }
                            if app.can_update {
                                let btn = egui::Button::new(
                                    RichText::new(crate::lucide::DOWNLOAD).size(12.0).color(p.accent),
                                )
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                                if ui
                                    .add_enabled(!state.task_running, btn)
                                    .on_hover_text("Update")
                                    .clicked()
                                {
                                    *pending_action = Some(AppAction::Update(app.clone()));
                                }
                            }
                            if app.can_launch {
                                let btn = egui::Button::new(
                                    RichText::new(crate::lucide::PLAY).size(12.0).color(p.success),
                                )
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0));
                                if ui.add(btn).on_hover_text("Launch").clicked() {
                                    *pending_action = Some(AppAction::Launch(app.clone()));
                                }
                            }
                        });
                    }
                });
        });
}

fn sort_header(
    state: &mut AppsManagerState,
    ui: &mut Ui,
    p: &ThemePalette,
    label: &str,
    col: AppSortCol,
    width: f32,
) {
    let active = state.sort_col == col;
    let arrow = if active {
        if state.sort_desc { " ▼" } else { " ▲" }
    } else {
        ""
    };
    cell_ui(ui, width, 22.0, false, |ui| {
        let btn = egui::Button::new(
            RichText::new(format!("{}{}", label, arrow))
                .size(11.0)
                .strong()
                .color(if active { p.accent } else { p.text_muted }),
        )
        .frame(false);
        if ui.add(btn).clicked() {
            if state.sort_col == col {
                state.sort_desc = !state.sort_desc;
            } else {
                state.sort_col = col;
                state.sort_desc = matches!(col, AppSortCol::Size | AppSortCol::InstalledDate);
            }
            state.mark_filter_dirty();
        }
    });
}

fn render_repos_table(state: &AppsManagerState, ui: &mut Ui, p: &ThemePalette, height: f32) {
    let q = state.search_query.trim().to_lowercase();
    let filtered: Vec<&RepoEntry> = state
        .repos
        .iter()
        .filter(|r| {
            q.is_empty()
                || r.id.to_lowercase().contains(&q)
                || r.name.to_lowercase().contains(&q)
                || r.source.to_lowercase().contains(&q)
                || r.url_or_details.to_lowercase().contains(&q)
        })
        .collect();

    Frame::none()
        .fill(p.bg_card)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            let inner_w = ui.available_width();
            ui.set_width(inner_w);
            ui.set_height((height - 20.0).max(100.0));

            let col_name_w = 220.0;
            let col_src_w = 90.0;
            let col_stat_w = 90.0;
            let col_url_w = (inner_w - col_name_w - col_src_w - col_stat_w - 24.0).max(160.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                cell_ui(ui, col_name_w, 22.0, false, |ui| {
                    ui.label(RichText::new("ID / Name").size(11.0).strong().color(p.text_muted));
                });
                cell_ui(ui, col_src_w, 22.0, false, |ui| {
                    ui.label(RichText::new("Source").size(11.0).strong().color(p.text_muted));
                });
                cell_ui(ui, col_stat_w, 22.0, false, |ui| {
                    ui.label(RichText::new("Status").size(11.0).strong().color(p.text_muted));
                });
                cell_ui(ui, col_url_w, 22.0, false, |ui| {
                    ui.label(RichText::new("URL / Details").size(11.0).strong().color(p.text_muted));
                });
            });
            ui.separator();

            if filtered.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("No repositories found.").size(13.0).color(p.text_muted));
                });
                return;
            }

            let scroll_h = ui.available_height().max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("repos_virtual_table")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show_rows(ui, ROW_HEIGHT, filtered.len(), |ui, row_range| {
                    for idx in row_range {
                        let repo = filtered[idx];
                        let bg = if idx % 2 == 1 { p.bg_subtle } else { Color32::TRANSPARENT };
                        let row_w = ui.available_width();
                        let (row_rect, _) =
                            ui.allocate_exact_size(egui::vec2(row_w, ROW_HEIGHT - 2.0), egui::Sense::hover());
                        if bg != Color32::TRANSPARENT {
                            ui.painter().rect_filled(row_rect, Rounding::same(6.0), bg);
                        }
                        let mut row_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(row_rect.shrink2(egui::vec2(4.0, 2.0)))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        row_ui.spacing_mut().item_spacing.x = 4.0;
                        cell_ui(&mut row_ui, col_name_w, 26.0, false, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&repo.name).size(12.0).strong().color(p.text_primary),
                                )
                                .truncate(),
                            );
                        });
                        cell_ui(&mut row_ui, col_src_w, 26.0, false, |ui| {
                            ui.label(RichText::new(&repo.source).size(11.5).color(p.accent));
                        });
                        cell_ui(&mut row_ui, col_stat_w, 26.0, false, |ui| {
                            let (sbg, sfg, lbl) = if repo.enabled {
                                (p.success_subtle, p.success, "Enabled")
                            } else {
                                (p.danger_subtle, p.danger, "Disabled")
                            };
                            Frame::none()
                                .fill(sbg)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(lbl).size(10.5).color(sfg));
                                });
                        });
                        cell_ui(&mut row_ui, col_url_w, 26.0, false, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&repo.url_or_details).size(11.5).color(p.text_secondary),
                                )
                                .truncate(),
                            );
                        });
                    }
                });
        });
}

fn render_inspector_panel(
    state: &mut AppsManagerState,
    ui: &mut Ui,
    p: &ThemePalette,
    width: f32,
    height: f32,
    pending_action: &mut Option<AppAction>,
) {
    let Some(ref key) = state.selected_key else {
        return;
    };
    let Some(app) = state
        .apps
        .iter()
        .find(|a| a.id == key.0 && a.source == key.1)
        .cloned()
    else {
        state.selected_key = None;
        return;
    };

    Frame::none()
        .fill(p.bg_card)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            let inner_w = (width - 28.0).max(200.0);
            ui.set_width(inner_w);
            ui.set_height((height - 24.0).max(120.0));

            // Header bar with app name and close button
            ui.horizontal(|ui| {
                ui.set_width(inner_w);
                ui.add(
                    egui::Label::new(RichText::new(&app.name).size(15.5).strong().color(p.text_primary))
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(crate::lucide::X).size(12.0).color(p.text_muted))
                                .frame(false),
                        )
                        .clicked()
                    {
                        state.selected_key = None;
                    }
                });
            });

            ui.add_space(4.0);
            let (sbg, sfg) = source_badge_colors(app.source, p);
            ui.horizontal_wrapped(|ui| {
                Frame::none()
                    .fill(sbg)
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(app.source.label()).size(10.5).color(sfg));
                    });
                ui.label(RichText::new(&app.version).size(11.5).color(p.text_secondary));
                if !app.size_display.is_empty() && app.size_display != "—" {
                    ui.label(RichText::new(format!("· {}", app.size_display)).size(11.5).color(p.text_muted));
                }
            });

            if let Some(ref new_ver) = app.update_available {
                ui.add_space(6.0);
                Frame::none()
                    .fill(p.success_subtle)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("Update available: {}", new_ver))
                                .size(11.5)
                                .strong()
                                .color(p.success),
                        );
                    });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            let scroll_h = (ui.available_height() - 78.0).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("apps_inspector_scroll")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(inner_w);

                    // Description Card (always shown prominently)
                    ui.label(RichText::new("Description").size(11.0).strong().color(p.text_muted));
                    ui.add_space(3.0);
                    Frame::none()
                        .fill(p.bg_subtle)
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_width((inner_w - 20.0).max(160.0));
                            let desc = if app.description.trim().is_empty() {
                                "No description provided for this package."
                            } else {
                                app.description.trim()
                            };
                            ui.add(egui::Label::new(RichText::new(desc).size(12.0).color(p.text_primary)).wrap());
                        });
                    ui.add_space(10.0);

                    inspector_field(ui, p, "Package / App ID", &app.id);
                    inspector_field(
                        ui,
                        p,
                        "Installed Date",
                        app.installed_date.as_deref().unwrap_or("Unknown"),
                    );
                    if !app.publisher_or_origin.is_empty() {
                        inspector_field(ui, p, "Origin / Publisher", &app.publisher_or_origin);
                    }

                    let loc_text = app
                        .install_location
                        .as_deref()
                        .or(app.desktop_file.as_deref())
                        .unwrap_or("Not resolved / system-wide");
                    inspector_field(ui, p, "Install Location", loc_text);

                    // Data Paths
                    ui.label(RichText::new("App Data Folders").size(11.0).strong().color(p.text_muted));
                    ui.add_space(2.0);
                    if app.data_paths.is_empty() {
                        ui.label(
                            RichText::new("No verified user data folders found.")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    } else {
                        for dp in &app.data_paths {
                            let sz = dp.size_bytes.map(format_bytes).unwrap_or_else(|| "…".to_string());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("• {} ({}): {}", dp.label, sz, dp.path.display()))
                                        .size(11.0)
                                        .color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        }
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons pinned at bottom of inspector
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                if app.can_launch {
                    let btn = egui::Button::new(
                        RichText::new(format!("{} Launch", crate::lucide::PLAY))
                            .size(12.0)
                            .color(p.accent_text),
                    )
                    .fill(p.success)
                    .rounding(Rounding::same(8.0));
                    if ui.add(btn).clicked() {
                        *pending_action = Some(AppAction::Launch(app.clone()));
                    }
                }

                if app.can_update {
                    let btn = egui::Button::new(
                        RichText::new(format!("{} Update", crate::lucide::DOWNLOAD))
                            .size(12.0)
                            .color(p.accent_text),
                    )
                    .fill(p.accent)
                    .rounding(Rounding::same(8.0));
                    if ui.add_enabled(!state.task_running, btn).clicked() {
                        *pending_action = Some(AppAction::Update(app.clone()));
                    }
                }

                let has_loc = app.install_location.is_some() || app.desktop_file.is_some();
                let open_btn = egui::Button::new(
                    RichText::new(format!("{} Open Folder", crate::lucide::FOLDER_OPEN))
                        .size(12.0)
                        .color(p.text_primary),
                )
                .fill(p.bg_button)
                .rounding(Rounding::same(8.0));
                if ui.add_enabled(has_loc, open_btn).clicked() {
                    *pending_action = Some(AppAction::OpenLocation(app.clone()));
                }

                if !app.data_paths.is_empty() {
                    let clear_btn = egui::Button::new(
                        RichText::new(format!("{} Clear Data", crate::lucide::LAYERS))
                            .size(12.0)
                            .color(p.warning),
                    )
                    .fill(p.warning_subtle)
                    .rounding(Rounding::same(8.0));
                    if ui.add_enabled(!state.task_running, clear_btn).clicked() {
                        state.confirm_clear_data = Some((app.clone(), ClearDataMode::CacheOnly));
                    }
                }

                if app.can_uninstall {
                    let uninst_btn = egui::Button::new(
                        RichText::new(format!("{} Uninstall", crate::lucide::TRASH))
                            .size(12.0)
                            .color(p.danger),
                    )
                    .fill(p.danger_subtle)
                    .rounding(Rounding::same(8.0));
                    if ui.add_enabled(!state.task_running, uninst_btn).clicked() {
                        state.confirm_uninstall = Some(app.clone());
                    }
                }
            });
        });
}

fn inspector_field(ui: &mut Ui, p: &ThemePalette, label: &str, value: &str) {
    ui.label(RichText::new(label).size(10.5).strong().color(p.text_muted));
    ui.add(egui::Label::new(RichText::new(value).size(12.0).color(p.text_primary)).wrap());
    ui.add_space(6.0);
}

fn render_task_drawer(state: &mut AppsManagerState, ui: &mut Ui, p: &ThemePalette) {
    Frame::none()
        .fill(p.bg_card)
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if state.task_running {
                    ui.spinner();
                }
                let title = if state.task_title.is_empty() {
                    "Task Output".to_string()
                } else {
                    state.task_title.clone()
                };
                ui.label(RichText::new(title).size(11.5).strong().color(p.text_primary));
                ui.label(
                    RichText::new(format!("({} lines)", state.task_logs.len()))
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let toggle_lbl = if state.log_drawer_open { "Hide Log" } else { "Show Log" };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(toggle_lbl).size(11.0).color(p.text_secondary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(6.0)),
                        )
                        .clicked()
                    {
                        state.log_drawer_open = !state.log_drawer_open;
                    }
                    if !state.task_logs.is_empty()
                        && ui
                            .add(
                                egui::Button::new(RichText::new("Clear").size(11.0).color(p.text_muted))
                                    .fill(p.bg_button)
                                    .rounding(Rounding::same(6.0)),
                            )
                            .clicked()
                    {
                        state.task_logs.clear();
                    }
                });
            });

            if state.log_drawer_open {
                ui.add_space(4.0);
                Frame::none()
                    .fill(p.terminal_bg)
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("apps_task_log_scroll")
                            .max_height(115.0)
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if state.task_logs.is_empty() {
                                    ui.label(
                                        RichText::new("No task output yet.")
                                            .monospace()
                                            .size(11.0)
                                            .color(p.text_muted),
                                    );
                                } else {
                                    for line in &state.task_logs {
                                        let color = if line.starts_with("[error]") || line.starts_with("[stderr]") {
                                            p.log_stderr
                                        } else if line.starts_with("==>") {
                                            p.accent
                                        } else {
                                            p.log_stdout
                                        };
                                        ui.label(RichText::new(line).monospace().size(11.0).color(color));
                                    }
                                }
                            });
                    });
            }
        });
}

fn render_confirmation_modals(
    state: &mut AppsManagerState,
    ctx: &egui::Context,
    p: &ThemePalette,
    pending_action: &mut Option<AppAction>,
) {
    let screen = ctx.screen_rect();
    let modal_frame = Frame::none()
        .fill(p.bg_card)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(14.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 18.0,
            spread: 0.0,
            color: Color32::from_black_alpha(140),
        })
        .inner_margin(egui::Margin::same(18.0));

    if let Some(app) = state.confirm_uninstall.clone() {
        let modal_w = 440.0_f32.min(screen.width() - 40.0).max(320.0);
        let mut open = true;
        egui::Window::new(RichText::new("Confirm Uninstall").size(16.0).strong())
            .id(egui::Id::new("apps_confirm_uninstall_modal"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .frame(modal_frame)
            .show(ctx, |ui| {
                *ui.visuals_mut() = p.egui_visuals();
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::lucide::ALERT_TRIANGLE).size(18.0).color(p.danger));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("Uninstall {}?", app.name))
                                .size(14.0)
                                .strong()
                                .color(p.text_primary),
                        );
                        ui.label(
                            RichText::new("This will remove the application or package from your system.")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    });
                });

                Frame::none()
                    .fill(p.bg_subtle)
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width((modal_w - 60.0).max(240.0));
                        ui.horizontal(|ui| {
                            let (sbg, sfg) = source_badge_colors(app.source, p);
                            Frame::none()
                                .fill(sbg)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(app.source.label()).size(10.5).color(sfg));
                                });
                            ui.label(RichText::new(&app.id).size(11.5).monospace().color(p.text_secondary));
                        });
                        if let Some(ref desk) = app.desktop_file {
                            ui.add_space(4.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("Target: {}", desk))
                                        .size(11.0)
                                        .monospace()
                                        .color(p.text_muted),
                                )
                                .wrap(),
                            );
                        }
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Cancel").color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(8.0)),
                        )
                        .clicked()
                    {
                        state.confirm_uninstall = None;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{} Uninstall", crate::lucide::TRASH))
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(p.danger)
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            *pending_action = Some(AppAction::Uninstall(app));
                            state.confirm_uninstall = None;
                        }
                    });
                });
            });
        if !open {
            state.confirm_uninstall = None;
        }
    }

    if let Some((app, mut mode)) = state.confirm_clear_data.clone() {
        let modal_w = 460.0_f32.min(screen.width() - 40.0).max(320.0);
        let mut open = true;
        let mut keep_open = true;
        egui::Window::new(RichText::new("Clear App Data").size(16.0).strong())
            .id(egui::Id::new("apps_confirm_clear_data_modal"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .min_width(modal_w)
            .max_width(modal_w)
            .frame(modal_frame)
            .show(ctx, |ui| {
                *ui.visuals_mut() = p.egui_visuals();
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::lucide::LAYERS).size(18.0).color(p.warning));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("Clear data for {}", app.name))
                                .size(14.0)
                                .strong()
                                .color(p.text_primary),
                        );
                        ui.label(
                            RichText::new("Select whether to remove only cache files or all local app data.")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                    });
                });

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut mode, ClearDataMode::CacheOnly, "Cache Only");
                    ui.selectable_value(
                        &mut mode,
                        ClearDataMode::AllAppData,
                        "All App Data (Cache + Config)",
                    );
                });

                let targets: Vec<_> = app
                    .data_paths
                    .iter()
                    .filter(|dp| match mode {
                        ClearDataMode::CacheOnly => dp.is_cache,
                        ClearDataMode::AllAppData => true,
                    })
                    .collect();

                let mut total_bytes: u64 = 0;
                Frame::none()
                    .fill(p.bg_subtle)
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width((modal_w - 60.0).max(240.0));
                        if targets.is_empty() {
                            ui.label(
                                RichText::new("No matching folders for this mode.")
                                    .size(11.5)
                                    .color(p.warning),
                            );
                        } else {
                            for dp in &targets {
                                let sz = dp.size_bytes.unwrap_or(0);
                                total_bytes = total_bytes.saturating_add(sz);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!(
                                            "• {} ({}): {}",
                                            dp.label,
                                            format_bytes(sz),
                                            dp.path.display()
                                        ))
                                        .size(11.0)
                                        .monospace()
                                        .color(p.text_secondary),
                                    )
                                    .wrap(),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("Total size to delete: {}", format_bytes(total_bytes)))
                                    .size(12.0)
                                    .strong()
                                    .color(p.warning),
                            );
                        }
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Cancel").color(p.text_primary))
                                .fill(p.bg_button)
                                .rounding(Rounding::same(8.0)),
                        )
                        .clicked()
                    {
                        keep_open = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !targets.is_empty(),
                                egui::Button::new(
                                    RichText::new(format!("{} Delete Data", crate::lucide::TRASH))
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(p.danger)
                                .rounding(Rounding::same(8.0)),
                            )
                            .clicked()
                        {
                            *pending_action = Some(AppAction::ClearData {
                                app: app.clone(),
                                mode,
                            });
                            keep_open = false;
                        }
                    });
                });
            });

        if !open || !keep_open {
            state.confirm_clear_data = None;
        } else {
            state.confirm_clear_data = Some((app, mode));
        }
    }
}
