use crate::config::ServerConfig;
use egui::{Color32, Context, Key, RichText, Window};

#[derive(Default)]
pub struct AddEditModalState {
    pub open: bool,
    pub is_edit: bool,
    pub original_key: String,

    pub key: String,
    pub name: String,
    pub cwd: String,
    pub command: String,
    pub stop_command: String,
    pub port_str: String,
    pub group: String,
    pub error_msg: Option<String>,
}

impl AddEditModalState {
    pub fn open_new(&mut self) {
        self.open = true;
        self.is_edit = false;
        self.original_key.clear();
        self.key.clear();
        self.name.clear();
        self.cwd = ".".to_string();
        self.command.clear();
        self.stop_command.clear();
        self.port_str = "0".to_string();
        self.group = "General".to_string();
        self.error_msg = None;
    }

    pub fn open_edit(&mut self, cfg: &ServerConfig) {
        self.open = true;
        self.is_edit = true;
        self.original_key = cfg.key.clone();
        self.key = cfg.key.clone();
        self.name = cfg.name.clone();
        self.cwd = cfg.cwd.clone();
        self.command = cfg.command.clone();
        self.stop_command = cfg.stop_command.clone();
        self.port_str = cfg.port.to_string();
        self.group = cfg.group.clone();
        self.error_msg = None;
    }

    pub fn to_server_config(&self) -> Result<ServerConfig, String> {
        let key = self.key.trim().to_string();
        let name = self.name.trim().to_string();
        let command = self.command.trim().to_string();

        if key.is_empty() {
            return Err("Key cannot be empty.".to_string());
        }
        if name.is_empty() {
            return Err("Name cannot be empty.".to_string());
        }
        if command.is_empty() {
            return Err("Start command cannot be empty.".to_string());
        }

        let port = self.port_str.trim().parse::<u16>().unwrap_or(0);
        let group = if self.group.trim().is_empty() {
            "General".to_string()
        } else {
            self.group.trim().to_string()
        };

        Ok(ServerConfig {
            key,
            name,
            cwd: if self.cwd.trim().is_empty() {
                ".".to_string()
            } else {
                self.cwd.trim().to_string()
            },
            command,
            stop_command: self.stop_command.trim().to_string(),
            port,
            group,
            env: Default::default(),
            actions: Default::default(),
            links: Default::default(),
        })
    }
}

#[allow(dead_code)]
pub enum ModalAction {
    SaveServer(ServerConfig, Option<String>), // (config, old_key if edit)
    DeleteServer(String),
    ExecutePaletteAction(PaletteAction),
    None,
}

#[derive(Clone, Debug)]
pub enum PaletteAction {
    StartServer(String),
    StopServer(String),
    SelectTab(String),
    StartGroup(String),
    StopGroup(String),
}

#[derive(Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
}

pub fn render_add_edit_modal(ctx: &Context, state: &mut AddEditModalState) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    let mut action = ModalAction::None;
    let title = if state.is_edit {
        "Edit Server"
    } else {
        "Add New Server"
    };

    Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(450.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            egui::Grid::new("server_form_grid")
                .num_columns(2)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Unique Key:");
                    ui.add_enabled(!state.is_edit, egui::TextEdit::singleline(&mut state.key));
                    ui.end_row();

                    ui.label("Display Name:");
                    ui.text_edit_singleline(&mut state.name);
                    ui.end_row();

                    ui.label("Group:");
                    ui.text_edit_singleline(&mut state.group);
                    ui.end_row();

                    ui.label("Working Directory:");
                    ui.text_edit_singleline(&mut state.cwd);
                    ui.end_row();

                    ui.label("Start Command:");
                    ui.text_edit_singleline(&mut state.command);
                    ui.end_row();

                    ui.label("Stop Command (opt):");
                    ui.text_edit_singleline(&mut state.stop_command);
                    ui.end_row();

                    ui.label("Port (0 = none):");
                    ui.text_edit_singleline(&mut state.port_str);
                    ui.end_row();
                });

            if let Some(err) = &state.error_msg {
                ui.add_space(6.0);
                ui.label(RichText::new(err).color(Color32::from_rgb(230, 80, 80)));
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save Server").strong()).clicked() {
                    match state.to_server_config() {
                        Ok(cfg) => {
                            let old_key = if state.is_edit {
                                Some(state.original_key.clone())
                            } else {
                                None
                            };
                            action = ModalAction::SaveServer(cfg, old_key);
                            state.open = false;
                        }
                        Err(err) => {
                            state.error_msg = Some(err);
                        }
                    }
                }

                if ui.button("Cancel").clicked() {
                    state.open = false;
                }
            });
        });

    action
}

pub fn render_command_palette(
    ctx: &Context,
    state: &mut CommandPaletteState,
    items: &[(String, PaletteAction)],
) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    let mut action = ModalAction::None;

    // Filter items
    let q = state.query.to_lowercase();
    let filtered: Vec<_> = items
        .iter()
        .filter(|(label, _)| label.to_lowercase().contains(&q))
        .collect();

    // Key navigation
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::ArrowDown)) && !filtered.is_empty() {
        state.selected_idx = (state.selected_idx + 1) % filtered.len();
    }
    if ctx.input(|i| i.key_pressed(Key::ArrowUp)) && !filtered.is_empty() {
        state.selected_idx = if state.selected_idx == 0 {
            filtered.len() - 1
        } else {
            state.selected_idx - 1
        };
    }

    if ctx.input(|i| i.key_pressed(Key::Enter)) && !filtered.is_empty() {
        let idx = state.selected_idx.min(filtered.len() - 1);
        action = ModalAction::ExecutePaletteAction(filtered[idx].1.clone());
        state.open = false;
        return action;
    }

    Window::new("⚡ Quick Launch (Ctrl+P)")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
        .default_width(520.0)
        .show(ctx, |ui| {
            let res = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Search servers, actions, or groups…")
                    .desired_width(f32::INFINITY),
            );
            res.request_focus();

            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(RichText::new("No matching actions").weak());
                    } else {
                        for (i, (label, act)) in filtered.iter().enumerate() {
                            let selected = i == state.selected_idx;
                            let text = if selected {
                                RichText::new(format!("❯  {}", label))
                                    .color(Color32::from_rgb(108, 92, 231))
                                    .strong()
                            } else {
                                RichText::new(format!("   {}", label))
                            };

                            let resp = ui.selectable_label(selected, text);
                            if resp.clicked() {
                                action = ModalAction::ExecutePaletteAction(act.clone());
                                state.open = false;
                            }
                        }
                    }
                });
        });

    action
}
