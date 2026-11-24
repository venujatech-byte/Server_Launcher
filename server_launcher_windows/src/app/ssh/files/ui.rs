use super::*;
use crate::{lucide, theme::ThemePalette};

#[derive(Clone, Copy)]
enum Action {
    Back,
    Forward,
    Up,
    Refresh,
    Open,
    Copy,
    Cut,
    Paste,
    Rename,
    Delete,
    NewFolder,
    NewFile,
    UploadFiles,
    UploadFolder,
    Download,
    Compress,
    ExtractHere,
    ExtractTo,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Up => "Up",
            Self::Refresh => "Refresh",
            Self::Open => "Open",
            Self::Copy => "Copy",
            Self::Cut => "Cut",
            Self::Paste => "Paste",
            Self::Rename => "Rename",
            Self::Delete => "Delete",
            Self::NewFolder => "New folder",
            Self::NewFile => "New file",
            Self::UploadFiles => "Upload files",
            Self::UploadFolder => "Upload folder",
            Self::Download => "Download",
            Self::Compress => "Compress to…",
            Self::ExtractHere => "Extract here",
            Self::ExtractTo => "Extract to folder…",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Back => lucide::ARROW_LEFT,
            Self::Forward => lucide::ARROW_RIGHT,
            Self::Up => lucide::ARROW_UP,
            Self::Refresh => lucide::REFRESH_CW,
            Self::Open => lucide::FOLDER_OPEN,
            Self::Copy => lucide::COPY,
            Self::Cut => "✂",
            Self::Paste => lucide::CLIPBOARD,
            Self::Rename => lucide::EDIT,
            Self::Delete => lucide::TRASH,
            Self::NewFolder | Self::UploadFolder => lucide::FOLDER,
            Self::NewFile => lucide::FILE_TEXT,
            Self::UploadFiles => lucide::UPLOAD,
            Self::Download => lucide::DOWNLOAD,
            Self::Compress => lucide::PACKAGE,
            Self::ExtractHere | Self::ExtractTo => lucide::FOLDER_OPEN,
        }
    }

    fn menu_group(self) -> u8 {
        match self {
            Self::Open | Self::Download => 0,
            Self::Cut | Self::Copy | Self::Paste | Self::Rename => 1,
            Self::Delete => 2,
            Self::Compress | Self::ExtractHere | Self::ExtractTo => 3,
            _ => 4,
        }
    }

    fn enabled(self, files: &RemoteFiles) -> bool {
        if files.worker.is_some() {
            return false;
        }
        let listing = files.listing.as_ref();
        let selected = files.selected.len();
        let single = (selected == 1)
            .then(|| files.selected.first())
            .flatten()
            .and_then(|name| listing?.entries.iter().find(|entry| &entry.name == name));
        match self {
            Self::Back => files.history_index > 0,
            Self::Forward => files.history_index + 1 < files.history.len(),
            Self::Up => files.directory().is_some_and(|path| path != "/"),
            Self::Refresh => true,
            Self::Open => {
                single.is_some_and(|entry| entry.kind == Kind::Folder || entry.opens_in_micro())
            }
            Self::ExtractHere | Self::ExtractTo => {
                single.is_some_and(|entry| entry.kind == Kind::Archive)
            }
            Self::Rename => selected == 1,
            Self::Download => {
                selected > 0
                    && listing.is_some_and(|listing| {
                        listing
                            .entries
                            .iter()
                            .filter(|entry| files.selected.contains(&entry.name))
                            .all(|entry| {
                                matches!(entry.kind, Kind::Folder | Kind::File | Kind::Archive)
                            })
                    })
            }
            Self::Copy | Self::Cut | Self::Delete | Self::Compress => selected > 0,
            Self::Paste => listing.is_some() && files.clipboard.is_some(),
            Self::NewFolder | Self::NewFile | Self::UploadFiles | Self::UploadFolder => {
                listing.is_some()
            }
        }
    }
}

pub(super) fn explorer(
    ui: &mut Ui,
    id: egui::Id,
    host: &SshRemoteHost,
    p: &ThemePalette,
    files: &mut RemoteFiles,
) {
    use Action::*;
    let focus = id.with("list");
    let mut action = None;
    let mut navigate = None;
    let mut dropped = Vec::new();

    egui::TopBottomPanel::bottom(id.with("status"))
        .frame(
            Frame::none()
                .fill(p.bg_card)
                .inner_margin(egui::Margin::symmetric(14.0, 7.0)),
        )
        .show_inside(ui, |ui| status(ui, p, files));

    ui.add_enabled_ui(files.prompt.is_none(), |ui| {
        Frame::none()
            .fill(p.bg_card)
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().interact_size.y = 24.0;
                ui.spacing_mut().button_padding = egui::vec2(8.0, 3.0);
                ui.visuals_mut().widgets.inactive.bg_fill = p.bg_button;
                ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
                ui.visuals_mut().widgets.hovered.bg_fill = p.bg_button_hover;
                ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = p.bg_button_hover;
                ui.horizontal_wrapped(|ui| {
                    for item in [Back, Forward, Up, Refresh] {
                        command_button(ui, files, item, &mut action, p, true);
                    }
                    ui.separator();
                    ui.menu_button(
                        RichText::new(format!("{} New", lucide::PLUS)).size(11.5),
                        |ui| menu(ui, files, &[NewFolder, NewFile], &mut action, p),
                    );
                    for item in [Cut, Copy, Paste, Rename, Delete] {
                        command_button(ui, files, item, &mut action, p, true);
                    }
                    ui.separator();
                    ui.menu_button(
                        RichText::new(format!("{} Upload", lucide::UPLOAD)).size(11.5),
                        |ui| menu(ui, files, &[UploadFiles, UploadFolder], &mut action, p),
                    );
                    command_button(ui, files, Download, &mut action, p, false);
                    command_button(ui, files, Compress, &mut action, p, false);
                    ui.menu_button(
                        RichText::new(format!("{} Extract", lucide::FOLDER_OPEN)).size(11.5),
                        |ui| menu(ui, files, &[ExtractHere, ExtractTo], &mut action, p),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let address_width = (ui.available_width() - 280.0)
                        .max(160.0)
                        .min(ui.available_width());
                    ui.allocate_ui_with_layout(
                        egui::vec2(address_width, 28.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            address(ui, files, &mut navigate, p);
                        },
                    );
                    Frame::none()
                        .fill(p.bg_input)
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
                        .show(ui, |ui| {
                            ui.spacing_mut().interact_size.y = 21.0;
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(lucide::SEARCH).size(11.5).color(p.text_muted),
                                );
                                ui.add_sized(
                                    [145.0, 21.0],
                                    egui::TextEdit::singleline(&mut files.filter)
                                        .hint_text(
                                            RichText::new("Filter this folder").color(p.text_muted),
                                        )
                                        .font(egui::FontId::proportional(11.5))
                                        .vertical_align(egui::Align::Center)
                                        .margin(egui::Margin::ZERO)
                                        .frame(false),
                                );
                            });
                        });
                    ui.checkbox(
                        &mut files.hidden,
                        RichText::new("Hidden files")
                            .size(11.5)
                            .color(p.text_secondary),
                    );
                });
            });

        if let Some((message, true)) = &files.notice {
            ui.add_space(6.0);
            Frame::none()
                .fill(p.danger_subtle)
                .rounding(Rounding::same(6.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(message.trim()).size(11.0).color(p.danger));
                });
        }
        ui.add_space(8.0);
        details(ui, id, focus, p, files, &mut action);
        if ui.is_enabled() && files.directory().is_some() && files.worker.is_none() {
            dropped = ui.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect()
            });
        }
    });

    let keyboard = ui.is_enabled()
        && files.prompt.is_none()
        && ui.memory(|memory| memory.focused().is_none() || memory.has_focus(focus));
    if ui.is_enabled()
        && files.prompt.is_none()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::L))
    {
        files.address = Some((files.directory().unwrap_or(".").to_owned(), true));
    }
    if keyboard && files.address.is_none() && action.is_none() {
        action = ui.input_mut(|input| {
            use egui::{Key, Modifiers};
            for (item, modifiers, key) in [
                (Copy, Modifiers::COMMAND, Key::C),
                (Cut, Modifiers::COMMAND, Key::X),
                (Paste, Modifiers::COMMAND, Key::V),
                (Delete, Modifiers::NONE, Key::Delete),
                (Rename, Modifiers::NONE, Key::F2),
                (Refresh, Modifiers::NONE, Key::F5),
                (Up, Modifiers::NONE, Key::Backspace),
                (Open, Modifiers::NONE, Key::Enter),
            ] {
                if item.enabled(files) && input.consume_key(modifiers, key) {
                    return Some(item);
                }
            }
            input.events.iter().find_map(|event| match event {
                egui::Event::Copy if Copy.enabled(files) => Some(Copy),
                egui::Event::Cut if Cut.enabled(files) => Some(Cut),
                egui::Event::Paste(_) if Paste.enabled(files) => Some(Paste),
                _ => None,
            })
        });
    }

    let ctx = ui.ctx().clone();
    if !dropped.is_empty() {
        if let Some(directory) = files.directory() {
            files.start(
                host,
                &ctx,
                Job::Transfer(Transfer::Dropped(directory.to_owned(), dropped)),
            );
        }
    } else if let Some(path) = navigate {
        files.start(host, &ctx, Job::List(path));
    } else if let Some(action) = action.filter(|item| item.enabled(files)) {
        activate(files, host, &ctx, action);
    }
    prompt(&ctx, id, files, host);
}

fn command_button(
    ui: &mut Ui,
    files: &RemoteFiles,
    item: Action,
    action: &mut Option<Action>,
    p: &ThemePalette,
    icon_only: bool,
) {
    let enabled = item.enabled(files);
    let text = if icon_only {
        item.icon().to_owned()
    } else {
        format!("{} {}", item.icon(), item.label())
    };
    let color = if !enabled {
        p.text_muted
    } else if matches!(item, Action::Delete) {
        p.danger
    } else {
        p.text_primary
    };
    let button = egui::Button::new(
        RichText::new(text)
            .size(if icon_only { 13.0 } else { 11.5 })
            .color(color),
    )
    .rounding(Rounding::same(5.0))
    .stroke(Stroke::NONE)
    .min_size(egui::vec2(26.0, 24.0));
    if ui
        .add_enabled(enabled, button)
        .on_hover_text(item.label())
        .clicked()
    {
        *action = Some(item);
    }
}

fn menu(
    ui: &mut Ui,
    files: &RemoteFiles,
    items: &[Action],
    action: &mut Option<Action>,
    p: &ThemePalette,
) {
    ui.set_width(178.0);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
    ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.visuals_mut().widgets.hovered.weak_bg_fill = p.bg_button_hover;
    let mut previous_group = None;
    for &item in items {
        let group = item.menu_group();
        if previous_group.is_some_and(|previous| previous != group) {
            ui.separator();
        }
        previous_group = Some(group);
        let enabled = item.enabled(files);
        let color = if !enabled {
            p.text_muted
        } else if matches!(item, Action::Delete) {
            p.danger
        } else {
            p.text_primary
        };
        let label = RichText::new(format!("{}  {}", item.icon(), item.label()))
            .size(11.5)
            .color(color);
        let width = ui.available_width();
        let button = egui::Button::new(label)
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::NONE)
            .min_size(egui::vec2(width, 24.0));
        if ui.add_enabled(enabled, button).clicked() {
            *action = Some(item);
            ui.close_menu();
        }
    }
}

fn address(ui: &mut Ui, files: &mut RemoteFiles, navigate: &mut Option<String>, p: &ThemePalette) {
    Frame::none()
        .fill(p.bg_input)
        .rounding(Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().interact_size.y = 21.0;
            if let Some((text, focus)) = files.address.as_mut() {
                let response = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(11.5))
                        .frame(false),
                );
                if *focus {
                    response.request_focus();
                    *focus = false;
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    files.address = None;
                } else if response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    && files.worker.is_none()
                {
                    if !text.is_empty() {
                        *navigate = Some(text.clone());
                    }
                    files.address = None;
                }
                return;
            }
            let path = files.directory().unwrap_or(".");
            let mut parent = String::new();
            let mut edit = false;
            ui.horizontal(|ui| {
                if path.starts_with('/') && ui.small_button(RichText::new("/").size(11.5)).clicked()
                {
                    *navigate = Some("/".into());
                }
                for segment in path.split('/').filter(|segment| !segment.is_empty()) {
                    parent.push('/');
                    parent.push_str(segment);
                    ui.label("›");
                    let label = segment.escape_debug().to_string();
                    if ui.small_button(RichText::new(label).size(11.5)).clicked()
                        && path.starts_with('/')
                    {
                        *navigate = Some(parent.clone());
                    }
                }
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().max(0.0), 21.0),
                    egui::Sense::click(),
                );
                if response.on_hover_text("Edit path · Ctrl+L").clicked() {
                    edit = true;
                }
                if rect.width() < 50.0 {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "…",
                        egui::FontId::proportional(12.0),
                        p.text_muted,
                    );
                }
            });
            if edit {
                files.address = Some((path.to_owned(), true));
            }
        });
}

fn details(
    ui: &mut Ui,
    id: egui::Id,
    focus: egui::Id,
    p: &ThemePalette,
    files: &mut RemoteFiles,
    action: &mut Option<Action>,
) {
    use Action::*;
    ui.interact(
        egui::Rect::NOTHING,
        focus,
        egui::Sense::focusable_noninteractive(),
    );
    let listing = files.listing.clone();
    let filter = files.filter.to_lowercase();
    let mut order: Vec<_> = listing
        .iter()
        .flat_map(|listing| &listing.entries)
        .filter(|entry| {
            (files.hidden || !entry.name.starts_with('.'))
                && entry.name.to_lowercase().contains(&filter)
        })
        .collect();
    order.sort_by(|a, b| {
        let folders_first = (b.kind == Kind::Folder).cmp(&(a.kind == Kind::Folder));
        let ordering = match files.sort {
            1 => a.modified.cmp(&b.modified),
            2 => a.kind.cmp(&b.kind),
            3 => a.size.cmp(&b.size),
            4 => a.mode.cmp(&b.mode),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
        .then_with(|| a.name.cmp(&b.name));
        folders_first.then(if files.descending {
            ordering.reverse()
        } else {
            ordering
        })
    });
    let widths = [
        (ui.available_width() - 435.0).max(190.0),
        139.0,
        108.0,
        75.0,
        93.0,
    ];
    let table_width: f32 = widths.iter().sum();

    egui::ScrollArea::horizontal()
        .id_salt(id.with("columns"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (index, title) in ["Name", "Date modified", "Type", "Size", "Permissions"]
                    .into_iter()
                    .enumerate()
                {
                    let mark = if files.sort != index {
                        ""
                    } else if files.descending {
                        " ↓"
                    } else {
                        " ↑"
                    };
                    let (rect, response) = ui
                        .allocate_exact_size(egui::vec2(widths[index], 26.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        Rounding::ZERO,
                        if response.hovered() {
                            p.bg_button_hover
                        } else {
                            p.bg_card
                        },
                    );
                    let inset = if index == 0 { 30.0 } else { 8.0 };
                    ui.painter().text(
                        rect.left_center() + egui::vec2(inset, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{title}{mark}"),
                        egui::FontId::proportional(11.5),
                        if files.sort == index {
                            p.text_primary
                        } else {
                            p.text_secondary
                        },
                    );
                    if response.clicked() {
                        if files.sort == index {
                            files.descending = !files.descending;
                        } else {
                            files.sort = index;
                            files.descending = false;
                        }
                    }
                }
            });
            ui.separator();
            let row_height = 26.0;
            egui::ScrollArea::vertical()
                .id_salt(id.with("rows"))
                .auto_shrink([false, false])
                .show_rows(
                    ui,
                    row_height - ui.spacing().item_spacing.y,
                    order.len() + 1,
                    |ui, range| {
                        for index in range {
                            if index == order.len() {
                                let height =
                                    (ui.clip_rect().bottom() - ui.cursor().top()).max(row_height);
                                let (rect, empty) = ui.allocate_exact_size(
                                    egui::vec2(table_width, height),
                                    egui::Sense::click(),
                                );
                                if empty.clicked() {
                                    files.selected.clear();
                                    ui.memory_mut(|memory| memory.request_focus(focus));
                                }
                                empty.context_menu(|ui| {
                                    menu(
                                        ui,
                                        files,
                                        &[
                                            Paste,
                                            NewFolder,
                                            NewFile,
                                            UploadFiles,
                                            UploadFolder,
                                            Refresh,
                                        ],
                                        action,
                                        p,
                                    )
                                });
                                if order.is_empty() && files.worker.is_none() {
                                    let message = if files.listing.is_none() {
                                        "No directory loaded"
                                    } else if files.filter.is_empty() {
                                        "This folder is empty or has hidden items"
                                    } else {
                                        "No matching items"
                                    };
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        message,
                                        egui::FontId::proportional(13.0),
                                        p.text_muted,
                                    );
                                }
                                continue;
                            }
                            let entry = order[index];
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(table_width, row_height - ui.spacing().item_spacing.y),
                                egui::Sense::click(),
                            );
                            if response.clicked() || response.secondary_clicked() {
                                ui.memory_mut(|memory| memory.request_focus(focus));
                                let modifiers = ui.input(|input| input.modifiers);
                                if response.secondary_clicked() {
                                    if !files.selected.contains(&entry.name) {
                                        files.selected.clear();
                                        files.selected.insert(entry.name.clone());
                                        files.anchor = Some(entry.name.clone());
                                    }
                                } else if modifiers.shift {
                                    let anchor = files
                                        .anchor
                                        .as_ref()
                                        .and_then(|name| {
                                            order.iter().position(|entry| &entry.name == name)
                                        })
                                        .unwrap_or(index);
                                    if !modifiers.command {
                                        files.selected.clear();
                                    }
                                    files.selected.extend(
                                        order[anchor.min(index)..=anchor.max(index)]
                                            .iter()
                                            .map(|entry| entry.name.clone()),
                                    );
                                } else {
                                    if !modifiers.command {
                                        files.selected.clear();
                                    }
                                    if !files.selected.insert(entry.name.clone()) {
                                        files.selected.remove(&entry.name);
                                    }
                                    files.anchor = Some(entry.name.clone());
                                }
                            }
                            if response.double_clicked()
                                && (entry.kind == Kind::Folder || entry.opens_in_micro())
                            {
                                files.selected.clear();
                                files.selected.insert(entry.name.clone());
                                *action = Some(Open);
                            }
                            response.context_menu(|ui| {
                                menu(
                                    ui,
                                    files,
                                    &[
                                        Open,
                                        Download,
                                        Cut,
                                        Copy,
                                        Rename,
                                        Delete,
                                        Compress,
                                        ExtractHere,
                                        ExtractTo,
                                    ],
                                    action,
                                    p,
                                )
                            });
                            response
                                .clone()
                                .on_hover_text(entry.name.escape_debug().to_string());
                            if files.selected.contains(&entry.name) || response.hovered() {
                                let fill = if files.selected.contains(&entry.name) {
                                    p.bg_card_active
                                } else {
                                    p.bg_button_hover
                                };
                                ui.painter().rect_filled(rect, Rounding::same(4.0), fill);
                            }
                            paint_row(ui, rect, widths, entry, p);
                        }
                    },
                );
        });
}

fn paint_row(ui: &Ui, rect: egui::Rect, widths: [f32; 5], entry: &Entry, p: &ThemePalette) {
    let icon = match entry.kind {
        Kind::Folder => lucide::FOLDER,
        Kind::Archive => lucide::PACKAGE,
        Kind::Symlink => lucide::LINK,
        _ => lucide::FILE_TEXT,
    };
    let color = if entry.kind == Kind::Folder {
        p.warning
    } else {
        p.text_secondary
    };
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(14.0),
        color,
    );
    let size = if entry.kind == Kind::Folder {
        "—".into()
    } else {
        crate::sensors::format_bytes(entry.size)
    };
    let name = entry.name.escape_debug().to_string();
    let mut x = rect.left();
    for (index, value) in [
        name.as_str(),
        entry.modified.as_str(),
        entry.kind.label(),
        size.as_str(),
        entry.mode.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        let inset = if index == 0 { 30.0 } else { 8.0 };
        let width = widths[index];
        let clip = egui::Rect::from_min_max(
            egui::pos2(x + inset, rect.top()),
            egui::pos2(x + width - 6.0, rect.bottom()),
        );
        let color = if index == 0 {
            p.text_primary
        } else {
            p.text_secondary
        };
        let mut job = egui::text::LayoutJob::simple(
            value.to_owned(),
            egui::FontId::proportional(11.5),
            color,
            clip.width(),
        );
        job.wrap.max_rows = 1;
        let galley = ui.fonts(|fonts| fonts.layout_job(job));
        ui.painter()
            .with_clip_rect(clip.intersect(ui.clip_rect()))
            .galley(
                egui::pos2(clip.left(), rect.center().y - galley.size().y * 0.5),
                galley,
                color,
            );
        x += width;
    }
}

fn activate(files: &mut RemoteFiles, host: &SshRemoteHost, ctx: &egui::Context, action: Action) {
    use Action::*;
    let directory = files.directory().unwrap_or(".").to_owned();
    let paths = || {
        files
            .selected
            .iter()
            .map(|name| model::join(&directory, name))
            .collect::<Vec<_>>()
    };
    let single = || files.selected.first().cloned().unwrap_or_default();
    let job = match action {
        Back | Forward => {
            let index = if matches!(action, Back) {
                files.history_index - 1
            } else {
                files.history_index + 1
            };
            files.history_target = Some(index);
            Job::List(files.history[index].clone())
        }
        Up => Job::List(
            directory
                .rsplit_once('/')
                .map_or(
                    "/",
                    |(parent, _)| if parent.is_empty() { "/" } else { parent },
                )
                .to_owned(),
        ),
        Refresh => Job::List(directory),
        Open => {
            let name = single();
            let path = model::join(&directory, &name);
            if files.listing.as_ref().is_some_and(|listing| {
                listing
                    .entries
                    .iter()
                    .any(|entry| entry.name == name && entry.opens_in_micro())
            }) {
                Job::Micro(path, false)
            } else {
                Job::List(path)
            }
        }
        Copy | Cut => {
            files.clipboard = Some((matches!(action, Cut), paths()));
            files.notice = Some(("Selection added to the remote clipboard.".into(), false));
            return;
        }
        Paste => {
            let Some((cut, paths)) = files.clipboard.clone() else {
                return;
            };
            Job::Paste(paths, directory, cut)
        }
        UploadFiles => Job::Transfer(Transfer::UploadFiles(directory)),
        UploadFolder => Job::Transfer(Transfer::UploadFolder(directory)),
        Download => {
            let single_file = files.selected.len() == 1
                && files.listing.as_ref().is_some_and(|listing| {
                    listing.entries.iter().any(|entry| {
                        files.selected.contains(&entry.name) && entry.kind != Kind::Folder
                    })
                });
            Job::Transfer(Transfer::Download(paths(), single_file))
        }
        ExtractHere => Job::Extract(model::join(&directory, &single()), directory, false),
        NewFolder | NewFile | Rename | Delete | Compress | ExtractTo => {
            let (prompt, name) = match action {
                NewFolder => (Prompt::Create(true), String::new()),
                NewFile => (Prompt::Create(false), String::new()),
                Rename => (Prompt::Rename(model::join(&directory, &single())), single()),
                Delete => (Prompt::Delete(paths()), String::new()),
                Compress => (Prompt::Compress(paths()), "Archive".into()),
                ExtractTo => {
                    let archive = single();
                    let stem = model::archive_extension(&archive).map_or(archive.as_str(), |ext| {
                        &archive[..archive.len() - ext.len()]
                    });
                    let folder = if stem.is_empty() {
                        "Extracted files"
                    } else {
                        stem
                    };
                    (
                        Prompt::Extract(model::join(&directory, &archive)),
                        folder.to_owned(),
                    )
                }
                _ => unreachable!(),
            };
            files.prompt = Some(prompt);
            files.name = name;
            files.format = 0;
            return;
        }
    };
    files.start(host, ctx, job);
}

fn prompt(ctx: &egui::Context, id: egui::Id, files: &mut RemoteFiles, host: &SshRemoteHost) {
    let Some(prompt) = files.prompt.clone() else {
        return;
    };
    let (title, button) = match &prompt {
        Prompt::Create(true) => ("New folder", "Create"),
        Prompt::Create(false) => ("New file", "Create"),
        Prompt::Rename(_) => ("Rename item", "Rename"),
        Prompt::Delete(_) => ("Permanently delete items", "Delete"),
        Prompt::Compress(_) => ("Create archive", "Compress"),
        Prompt::Extract(_) => ("Extract to new folder", "Extract"),
        Prompt::InstallMicro(_) => ("Micro is not installed", "Install and open"),
    };
    let formats = [".zip", ".tar", ".tar.gz", ".rar"];
    let mut dismiss = false;
    let mut submit = false;
    egui::Window::new(title)
        .id(id.with("prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            if let Prompt::InstallMicro(path) = &prompt {
                ui.label("Install Micro on the SSH host and open this file?");
                ui.monospace(path.escape_debug().to_string());
                ui.label("The SSH terminal will show any package manager or sudo prompts.");
            } else if let Prompt::Delete(paths) = &prompt {
                ui.label("The selected items will be removed from the remote host.");
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for path in paths {
                            ui.monospace(path.escape_debug().to_string());
                        }
                    });
            } else {
                ui.label(if matches!(prompt, Prompt::Compress(_)) {
                    "Archive name"
                } else {
                    "Name"
                });
                ui.text_edit_singleline(&mut files.name);
                if matches!(prompt, Prompt::Compress(_)) {
                    egui::ComboBox::from_id_salt(id.with("format"))
                        .selected_text(formats[files.format])
                        .show_ui(ui, |ui| {
                            for (index, format) in formats.iter().enumerate() {
                                ui.selectable_value(&mut files.format, index, *format);
                            }
                        });
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                dismiss = ui.button("Cancel").clicked()
                    || ui.input(|input| input.key_pressed(egui::Key::Escape));
                submit = ui
                    .add_enabled(
                        matches!(prompt, Prompt::Delete(_) | Prompt::InstallMicro(_))
                            || model::valid_name(&files.name),
                        egui::Button::new(button),
                    )
                    .clicked();
            });
        });
    if dismiss {
        files.prompt = None;
    }
    if !submit {
        return;
    }
    let Some(directory) = files.directory() else {
        return;
    };
    let target = model::join(directory, &files.name);
    let job = match prompt {
        Prompt::Create(folder) => Job::Create(target, folder),
        Prompt::Rename(source) => Job::Rename(source, target),
        Prompt::Delete(paths) => Job::Delete(paths),
        Prompt::Compress(paths) => {
            let ext = formats[files.format];
            let target = if target.to_ascii_lowercase().ends_with(ext) {
                target
            } else {
                format!("{target}{ext}")
            };
            Job::Compress(paths, target)
        }
        Prompt::Extract(source) => Job::Extract(source, target, true),
        Prompt::InstallMicro(path) => Job::Micro(path, true),
    };
    files.prompt = None;
    files.start(host, ctx, job);
}

fn status(ui: &mut Ui, p: &ThemePalette, files: &mut RemoteFiles) {
    if let Some(progress) = &files.progress {
        let bytes = crate::sensors::format_bytes(progress.bytes);
        let fraction = progress
            .total_bytes
            .filter(|total| *total > 0)
            .map(|total| (progress.bytes as f32 / total as f32).clamp(0.0, 1.0));
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} · {}", progress.direction, progress.name))
                    .size(12.0)
                    .strong()
                    .color(p.text_primary),
            );
            if progress.count > 1 {
                ui.label(
                    RichText::new(format!(
                        "Item {} of {}",
                        progress.completed + 1,
                        progress.count
                    ))
                    .size(11.0)
                    .color(p.text_muted),
                );
            }
        });
        if let Some(fraction) = fraction {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .show_percentage(),
            );
        } else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Transfer in progress");
            });
        }
        ui.horizontal_wrapped(|ui| {
            let size = progress.total_bytes.map_or(bytes.clone(), |total| {
                format!("{bytes} / {}", crate::sensors::format_bytes(total))
            });
            ui.label(RichText::new(size).size(11.0).color(p.text_secondary));
            ui.separator();
            let speed = progress.speed.map_or_else(
                || "Measuring speed…".to_owned(),
                |speed| format!("{}/s", crate::sensors::format_bytes(speed)),
            );
            ui.label(
                RichText::new(speed)
                    .size(11.0)
                    .strong()
                    .color(p.text_primary),
            );
            if let Some(next) = &progress.next {
                ui.separator();
                ui.label(
                    RichText::new(format!("Next: {next}"))
                        .size(11.0)
                        .color(p.text_muted),
                );
            }
            if progress.count > 1 {
                ui.label(
                    RichText::new(format!(
                        "{} complete · {} queued",
                        progress.completed,
                        progress.count - progress.completed - 1
                    ))
                    .size(11.0)
                    .color(p.text_muted),
                );
            }
        });
        ui.separator();
    }
    ui.horizontal_wrapped(|ui| {
        let query = files.filter.to_lowercase();
        let items = files.listing.iter().flat_map(|listing| &listing.entries);
        let total = items.clone().count();
        let visible = items
            .filter(|entry| {
                (files.hidden || !entry.name.starts_with('.'))
                    && (query.is_empty() || entry.name.to_lowercase().contains(&query))
            })
            .count();
        let count = if visible == total {
            format!("{total} items")
        } else {
            format!("{visible} of {total} items")
        };
        ui.label(RichText::new(count).size(11.0).color(p.text_secondary));
        if !files.selected.is_empty() {
            let size = files
                .listing
                .iter()
                .flat_map(|listing| &listing.entries)
                .filter(|entry| entry.kind != Kind::Folder && files.selected.contains(&entry.name))
                .fold(0u64, |sum, entry| sum.saturating_add(entry.size));
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} selected · {}",
                    files.selected.len(),
                    crate::sensors::format_bytes(size)
                ))
                .size(11.0)
                .color(p.text_secondary),
            );
        }
        if let Some(worker) = &files.worker {
            ui.separator();
            if files.progress.is_none() {
                ui.spinner();
                ui.label(
                    RichText::new(if files.transfer {
                        "Waiting for transfer…"
                    } else {
                        "Working…"
                    })
                    .size(11.0)
                    .color(p.text_secondary),
                );
            }
            if files.transfer && ui.small_button("Cancel transfer").clicked() {
                worker
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        } else if let Some((message, false)) = &files.notice {
            ui.separator();
            ui.label(RichText::new(message).size(11.0).color(p.text_secondary));
        }
    });
}
