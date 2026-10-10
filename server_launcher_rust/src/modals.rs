use crate::config::{CustomAction, CustomLink, ServerConfig};
use crate::theme::ThemePalette;
use egui::{Color32, Context, Key, RichText, Rounding, Stroke, Window};
use std::collections::HashMap;

pub struct ServerTemplate {
    pub name: &'static str,
    pub command: &'static str,
    pub stop_command: &'static str,
    pub port: u16,
    pub group: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    pub actions: &'static [(&'static str, &'static str)],
    pub links: &'static [(&'static str, &'static str)],
}

pub const SERVER_TEMPLATES: &[ServerTemplate] = &[
    ServerTemplate {
        name: "React (Vite)",
        command: "npm run dev",
        stop_command: "",
        port: 5173,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:5173")],
    },
    ServerTemplate {
        name: "Next.js",
        command: "npm run dev",
        stop_command: "",
        port: 3000,
        group: "Frontend",
        env: &[],
        actions: &[("Build", "npm run build"), ("Install", "npm install")],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "React Native (Metro)",
        command: "npx react-native start",
        stop_command: "",
        port: 8081,
        group: "Mobile",
        env: &[],
        actions: &[
            ("Android", "npx react-native run-android"),
            ("iOS", "npx react-native run-ios"),
            ("Install", "npm install"),
            ("Reset Cache", "npx react-native start --reset-cache"),
        ],
        links: &[
            ("Metro Bundler", "http://localhost:8081"),
            ("Metro Status", "http://localhost:8081/status"),
        ],
    },
    ServerTemplate {
        name: "React Native (Expo)",
        command: "npx expo start",
        stop_command: "",
        port: 8081,
        group: "Mobile",
        env: &[],
        actions: &[
            ("Android", "npx expo run:android"),
            ("iOS", "npx expo run:ios"),
            ("Web", "npx expo start --web"),
            ("Install", "npx expo install"),
        ],
        links: &[("Metro Bundler", "http://localhost:8081")],
    },
    ServerTemplate {
        name: "FastAPI (Uvicorn)",
        command: "uvicorn main:app --reload --port 8000",
        stop_command: "",
        port: 8000,
        group: "Backend",
        env: &[],
        actions: &[("Install reqs", "pip install -r requirements.txt")],
        links: &[("Swagger Docs", "http://localhost:8000/docs")],
    },
    ServerTemplate {
        name: "Flask",
        command: "flask run --port 5000 --debug",
        stop_command: "",
        port: 5000,
        group: "Backend",
        env: &[("FLASK_ENV", "development"), ("FLASK_DEBUG", "1")],
        actions: &[("Install reqs", "pip install -r requirements.txt")],
        links: &[("Localhost", "http://localhost:5000")],
    },
    ServerTemplate {
        name: "Django",
        command: "python manage.py runserver 0.0.0.0:8000",
        stop_command: "",
        port: 8000,
        group: "Backend",
        env: &[],
        actions: &[("Migrate", "python manage.py migrate")],
        links: &[("Admin Panel", "http://localhost:8000/admin")],
    },
    ServerTemplate {
        name: "Node / Express",
        command: "node server.js",
        stop_command: "",
        port: 3000,
        group: "Backend",
        env: &[],
        actions: &[("Install", "npm install")],
        links: &[("Localhost", "http://localhost:3000")],
    },
    ServerTemplate {
        name: "PostgreSQL",
        command: "postgres -D ./data",
        stop_command: "pg_ctl stop -D ./data",
        port: 5432,
        group: "DB",
        env: &[],
        actions: &[],
        links: &[],
    },
    ServerTemplate {
        name: "Redis",
        command: "redis-server",
        stop_command: "redis-cli shutdown",
        port: 6379,
        group: "DB",
        env: &[],
        actions: &[],
        links: &[],
    },
    ServerTemplate {
        name: "MongoDB",
        command: "mongod --dbpath ./data/db",
        stop_command: "",
        port: 27017,
        group: "DB",
        env: &[],
        actions: &[],
        links: &[],
    },
];

#[derive(Default)]
pub struct AddEditModalState {
    pub open: bool,
    pub is_edit: bool,
    pub original_key: String,

    pub selected_template_idx: usize,
    pub key: String,
    pub name: String,
    pub group: String,
    pub cwd: String,
    pub command: String,
    pub stop_command: String,
    pub port_str: String,
    pub own_console: bool,
    pub auto_restart: bool,
    pub env_text: String,
    pub actions: Vec<(String, String)>,
    pub links: Vec<(String, String)>,
    pub error_msg: Option<String>,
}

impl AddEditModalState {
    pub fn open_new(&mut self) {
        self.open = true;
        self.is_edit = false;
        self.original_key.clear();
        self.selected_template_idx = 0;
        self.key.clear();
        self.name.clear();
        self.group = "General".to_string();
        self.cwd = ".".to_string();
        self.command.clear();
        self.stop_command.clear();
        self.port_str = "0".to_string();
        self.own_console = false;
        self.auto_restart = false;
        self.env_text.clear();
        self.actions.clear();
        self.links.clear();
        self.error_msg = None;
    }

    pub fn open_import(&mut self, name: &str, port: u16, command: &str, cwd: &str) {
        self.open_new();
        self.name = name.to_string();
        self.key = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string();
        if self.key.is_empty() {
            self.key = format!("port_{}", port);
        }
        self.port_str = port.to_string();
        self.command = command.to_string();
        self.cwd = if cwd.is_empty() { ".".to_string() } else { cwd.to_string() };
        self.group = "Imported".to_string();
        if port > 0 {
            self.links.push(("Localhost".to_string(), format!("http://localhost:{}", port)));
        }
    }

    pub fn open_edit(&mut self, cfg: &ServerConfig) {
        self.open = true;
        self.is_edit = true;
        self.original_key = cfg.key.clone();
        self.selected_template_idx = 0;
        self.key = cfg.key.clone();
        self.name = cfg.name.clone();
        self.group = cfg.group.clone();
        self.cwd = cfg.cwd.clone();
        self.command = cfg.command.clone();
        self.stop_command = cfg.stop_command.clone();
        self.port_str = cfg.port.to_string();
        self.own_console = cfg.own_console;
        self.auto_restart = cfg.auto_restart;

        let mut env_lines = Vec::new();
        for (k, v) in &cfg.env {
            env_lines.push(format!("{}={}", k, v));
        }
        self.env_text = env_lines.join("\n");

        self.actions = cfg
            .actions
            .iter()
            .map(|a| (a.label.clone(), a.command.clone()))
            .collect();
        self.links = cfg
            .links
            .iter()
            .map(|l| (l.label.clone(), l.url.clone()))
            .collect();
        self.error_msg = None;
    }

    pub fn apply_template(&mut self, idx: usize) {
        if idx == 0 || idx > SERVER_TEMPLATES.len() {
            return;
        }
        let tpl = &SERVER_TEMPLATES[idx - 1];
        if self.name.trim().is_empty() {
            self.name = tpl.name.to_string();
        }
        if self.key.trim().is_empty() {
            self.key = tpl
                .name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .trim_matches('_')
                .to_string();
        }
        self.command = tpl.command.to_string();
        self.stop_command = tpl.stop_command.to_string();
        self.port_str = tpl.port.to_string();
        self.group = tpl.group.to_string();

        let env_lines: Vec<String> = tpl
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        self.env_text = env_lines.join("\n");

        self.actions = tpl
            .actions
            .iter()
            .map(|(l, c)| (l.to_string(), c.to_string()))
            .collect();
        self.links = tpl
            .links
            .iter()
            .map(|(l, u)| (l.to_string(), u.to_string()))
            .collect();
    }

    pub fn to_server_config(&self) -> Result<ServerConfig, String> {
        let name = self.name.trim().to_string();
        let cwd = self.cwd.trim().to_string();
        let command = self.command.trim().to_string();

        if name.is_empty() {
            return Err("Server name is required.".to_string());
        }
        if cwd.is_empty() {
            return Err("Working directory is required.".to_string());
        }
        if command.is_empty() {
            return Err("Start command is required.".to_string());
        }

        let key = if self.key.trim().is_empty() {
            let derived = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .collect::<String>()
                .trim_matches('_')
                .to_string();
            if derived.is_empty() {
                format!("custom_{}", chrono::Local::now().timestamp())
            } else {
                derived
            }
        } else {
            self.key.trim().to_string()
        };

        let port = self.port_str.trim().parse::<u16>().unwrap_or(0);
        let group = if self.group.trim().is_empty() {
            "General".to_string()
        } else {
            self.group.trim().to_string()
        };

        // Parse environment variables
        let mut env = HashMap::new();
        for line in self.env_text.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                if let Some((k, v)) = line.split_once('=') {
                    env.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        // Actions
        let actions = self
            .actions
            .iter()
            .filter(|(l, c)| !l.trim().is_empty() && !c.trim().is_empty())
            .map(|(l, c)| CustomAction {
                label: l.trim().to_string(),
                command: c.trim().to_string(),
            })
            .collect();

        // Links
        let links = self
            .links
            .iter()
            .filter(|(l, u)| !l.trim().is_empty() && !u.trim().is_empty())
            .map(|(l, u)| CustomLink {
                label: l.trim().to_string(),
                url: u.trim().to_string(),
            })
            .collect();

        Ok(ServerConfig {
            key,
            name,
            cwd,
            command,
            stop_command: self.stop_command.trim().to_string(),
            port,
            group,
            own_console: self.own_console,
            auto_restart: self.auto_restart,
            env,
            actions,
            links,
        })
    }
}

#[allow(dead_code)]
pub enum ModalAction {
    SaveServer(ServerConfig, Option<String>),
    DeleteServer(String),
    SaveSshHost(crate::config::SshRemoteHost, Option<String>),
    DeleteSshHost(String),
    ExecutePaletteAction(PaletteAction),
    None,
}

#[derive(Clone, Debug)]
pub enum PaletteAction {
    StartServer(String),
    StopServer(String),
    RestartServer(String),
    SelectTab(String),
    RunAction { key: String, command: String },
    OpenLink(String),
    StartGroup(String),
    StopGroup(String),
    StartAll,
    StopAll,
    SetTheme(String),
}

#[derive(Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_idx: usize,
}

pub fn render_add_edit_modal(ctx: &Context, state: &mut AddEditModalState, palette: &ThemePalette) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    let mut action = ModalAction::None;
    let window_title = if state.is_edit {
        "Edit Server"
    } else {
        "Add Server"
    };

    let mut is_open = state.open;
    Window::new(window_title)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(560.0)
        .default_height(640.0)
        .show(ctx, |ui| {
            // Header
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(if state.is_edit {
                            "Edit server"
                        } else {
                            "Add a new server"
                        })
                        .size(16.0)
                        .strong()
                        .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Saved to servers.json - no code changes needed.")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new("✕").size(13.0).color(palette.text_secondary))
                        .clicked()
                    {
                        state.open = false;
                    }
                });
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 55.0)
                .show(ui, |ui| {
                    // Template Picker (only for new servers)
                    if !state.is_edit {
                        egui::Frame::none()
                            .fill(palette.bg_subtle)
                            .stroke(Stroke::new(1.0_f32, palette.border))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("⚡ Pick a Template (Fast Setup)")
                                        .size(12.5)
                                        .strong()
                                        .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(
                                        "Auto-populates command, port, actions, and links for popular stacks.",
                                    )
                                    .size(10.5)
                                    .color(palette.text_muted),
                                );
                                ui.add_space(6.0);

                                ui.horizontal(|ui| {
                                    let mut template_names = vec!["-- Select a template --"];
                                    for t in SERVER_TEMPLATES {
                                        template_names.push(t.name);
                                    }

                                    let cur_name = template_names[state.selected_template_idx];
                                    egui::ComboBox::from_id_salt("template_select")
                                        .selected_text(cur_name)
                                        .width(220.0)
                                        .show_ui(ui, |ui| {
                                            for (i, name) in template_names.iter().enumerate() {
                                                ui.selectable_value(
                                                    &mut state.selected_template_idx,
                                                    i,
                                                    *name,
                                                );
                                            }
                                        });

                                    let apply_btn = egui::Button::new(
                                        RichText::new("Apply Template")
                                            .size(11.0)
                                            .strong()
                                            .color(palette.accent_text),
                                    )
                                    .fill(palette.accent)
                                    .rounding(Rounding::same(4.0));

                                    if ui.add(apply_btn).clicked() {
                                        state.apply_template(state.selected_template_idx);
                                    }
                                });
                            });

                        ui.add_space(10.0);
                    }

                    // Server Name *
                    ui.label(RichText::new("Server name *").size(12.0).strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut state.name)
                            .hint_text("e.g. Frontend App")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    // Unique Key (optional override)
                    ui.label(
                        RichText::new("Unique Key (optional override)")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                    ui.add_enabled(
                        !state.is_edit,
                        egui::TextEdit::singleline(&mut state.key)
                            .hint_text("auto-generated if empty")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    // Server Group
                    ui.label(
                        RichText::new("Server group (Frontend / Backend / DB)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Tag servers for group start/stop. Pick preset or type custom:")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.group)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        for preset in &["Frontend", "Backend", "DB", "General"] {
                            let btn = egui::Button::new(
                                RichText::new(*preset).size(10.5).color(palette.text_secondary),
                            )
                            .fill(palette.bg_button)
                            .rounding(Rounding::same(4.0));
                            if ui.add(btn).clicked() {
                                state.group = preset.to_string();
                            }
                        }
                    });
                    ui.add_space(8.0);

                    // Working Directory *
                    ui.label(RichText::new("Working directory (path) *").size(12.0).strong());
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.cwd)
                                .hint_text("e.g. /path/to/project or .")
                                .desired_width(ui.available_width() - 85.0),
                        );
                        let browse_btn = egui::Button::new(
                            RichText::new("Browse").size(11.5).color(palette.text_secondary),
                        )
                        .fill(palette.bg_button)
                        .rounding(Rounding::same(4.0));
                        if ui.add(browse_btn).clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                state.cwd = folder.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.add_space(8.0);

                    // Start Command *
                    ui.label(RichText::new("Start command *").size(12.0).strong());
                    ui.label(
                        RichText::new("e.g.  npm run dev   |   python app.py   |   node server.js")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.command)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    // Stop Command (optional)
                    ui.label(
                        RichText::new("Stop command (optional - runs before stopping)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("e.g.  pg_ctl stop   - leave empty for plain kill")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.stop_command)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    // Port
                    ui.label(
                        RichText::new("Port (0 = none, used to detect running state)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut state.port_str)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    // Own console checkbox
                    ui.checkbox(
                        &mut state.own_console,
                        RichText::new("Run in own console window (external terminal)")
                            .size(12.0)
                            .color(Color32::from_rgb(220, 225, 235)),
                    );
                    ui.label(
                        RichText::new("Opens in a native terminal emulator (gnome-terminal, konsole, cmd.exe, etc.)")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(8.0);

                    // Auto-restart on crash checkbox
                    ui.checkbox(
                        &mut state.auto_restart,
                        RichText::new("Auto-restart on crash")
                            .size(12.0)
                            .color(Color32::from_rgb(220, 225, 235)),
                    );
                    ui.label(
                        RichText::new("Automatically restarts this server if the process exits unexpectedly or crashes")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(8.0);

                    // Environment Variables
                    ui.label(
                        RichText::new("Environment variables (KEY=VALUE, one per line)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut state.env_text)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(10.0);

                    // Extra Action Buttons
                    ui.label(
                        RichText::new("Extra action buttons (sub-commands)")
                            .size(12.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Run inside working directory. e.g. label: 'Build', command: 'npm run build'")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(2.0);

                    let mut remove_action_idx = None;
                    for (i, (lbl, cmd)) in state.actions.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("label:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(lbl).desired_width(90.0));
                            ui.label(RichText::new("cmd:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(cmd).desired_width(ui.available_width() - 40.0));
                            if ui
                                .button(RichText::new("✕").size(11.0).color(Color32::from_rgb(230, 80, 80)))
                                .clicked()
                            {
                                remove_action_idx = Some(i);
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if let Some(i) = remove_action_idx {
                        state.actions.remove(i);
                    }

                    if ui
                        .button(
                            RichText::new("+ Add action button")
                                .size(11.0)
                                .color(Color32::from_rgb(108, 92, 231)),
                        )
                        .clicked()
                    {
                        state.actions.push((String::new(), String::new()));
                    }

                    ui.add_space(10.0);

                    // Open Links
                    ui.label(RichText::new("Open links (label : url)").size(12.0).strong());
                    ui.label(
                        RichText::new("e.g. label: 'Localhost', url: 'http://localhost:3000'")
                            .size(10.5)
                            .color(Color32::from_rgb(123, 131, 148)),
                    );
                    ui.add_space(2.0);

                    let mut remove_link_idx = None;
                    for (i, (lbl, url)) in state.links.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("label:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(lbl).desired_width(90.0));
                            ui.label(RichText::new("url:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            ui.add(egui::TextEdit::singleline(url).desired_width(ui.available_width() - 40.0));
                            if ui
                                .button(RichText::new("✕").size(11.0).color(Color32::from_rgb(230, 80, 80)))
                                .clicked()
                            {
                                remove_link_idx = Some(i);
                            }
                        });
                        ui.add_space(2.0);
                    }
                    if let Some(i) = remove_link_idx {
                        state.links.remove(i);
                    }

                    if ui
                        .button(
                            RichText::new("+ Add link")
                                .size(11.0)
                                .color(palette.accent),
                        )
                        .clicked()
                    {
                        state.links.push((String::new(), String::new()));
                    }

                    ui.add_space(12.0);
                });

            if let Some(err) = &state.error_msg {
                ui.add_space(4.0);
                ui.label(RichText::new(err).color(palette.danger));
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Bottom Buttons: Cancel & Save
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("Save")
                            .size(12.0)
                            .strong()
                            .color(palette.accent_text),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(4.0))
                    .min_size(egui::vec2(80.0, 28.0));

                    if ui.add(btn_save).clicked() {
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

                    let btn_cancel = egui::Button::new(
                        RichText::new("Cancel")
                            .size(12.0)
                            .color(palette.text_secondary),
                    )
                    .fill(palette.bg_button)
                    .rounding(Rounding::same(4.0))
                    .min_size(egui::vec2(80.0, 28.0));

                    if ui.add(btn_cancel).clicked() {
                        state.open = false;
                    }
                });
            });
        });

    state.open = is_open;
    action
}

pub fn render_command_palette(
    ctx: &Context,
    state: &mut CommandPaletteState,
    items: &[(String, PaletteAction)],
    palette: &ThemePalette,
) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    let mut action = ModalAction::None;

    // Filter items
    let q = state.query.to_lowercase();
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let filtered: Vec<_> = items
        .iter()
        .filter(|(label, _)| {
            let l_lower = label.to_lowercase();
            tokens.iter().all(|tok| l_lower.contains(tok))
        })
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

    let mut is_open = state.open;
    Window::new("⚡ Quick Launch (Ctrl+P)")
        .open(&mut is_open)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .default_width(540.0)
        .default_height(400.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⚡ Quick Launch")
                        .size(14.0)
                        .strong()
                        .color(palette.text_primary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new("✕").size(12.0).color(palette.text_secondary))
                        .clicked()
                    {
                        state.open = false;
                    }
                    ui.label(
                        RichText::new("Esc to close")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                });
            });

            ui.add_space(8.0);

            let res = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Search servers, actions, groups, or links…")
                    .desired_width(f32::INFINITY),
            );
            res.request_focus();

            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(280.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.add_space(10.0);
                        ui.label(RichText::new("No matching actions").weak());
                    } else {
                        for (i, (label, act)) in filtered.iter().enumerate() {
                            let selected = i == state.selected_idx;
                            let text = if selected {
                                RichText::new(format!("❯  {}", label))
                                    .color(palette.accent_light)
                                    .strong()
                            } else {
                                RichText::new(format!("   {}", label))
                                    .color(palette.text_secondary)
                            };

                            let resp = ui.selectable_label(selected, text);
                            if resp.clicked() {
                                action = ModalAction::ExecutePaletteAction((*act).clone());
                                state.open = false;
                            }
                        }
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Type to filter • ↑/↓ navigate • Enter select")
                        .size(10.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(RichText::new("Close").size(11.0).color(palette.text_secondary))
                        .clicked()
                    {
                        state.open = false;
                    }
                });
            });
        });

    state.open = is_open;
    action
}

#[derive(Clone, Default)]
pub struct SshHostModalState {
    pub open: bool,
    pub is_edit: bool,
    pub editing_id: Option<String>,
    pub name: String,
    pub host: String,
    pub port_str: String,
    pub user: String,
    pub key_path: String,
    pub password: String,
    pub show_password: bool,
    pub remote_cwd: String,
    pub test_result: Option<Result<String, String>>,
    pub is_testing: bool,
}

impl SshHostModalState {
    pub fn open_add(&mut self) {
        self.open = true;
        self.is_edit = false;
        self.editing_id = None;
        self.name = String::new();
        self.host = String::new();
        self.port_str = "22".to_string();
        self.user = "ubuntu".to_string();
        self.key_path = String::new();
        self.password = String::new();
        self.show_password = false;
        self.remote_cwd = String::new();
        self.test_result = None;
        self.is_testing = false;
    }

    pub fn open_edit(&mut self, h: &crate::config::SshRemoteHost) {
        self.open = true;
        self.is_edit = true;
        self.editing_id = Some(h.id.clone());
        self.name = h.name.clone();
        self.host = h.host.clone();
        self.port_str = h.port.to_string();
        self.user = h.user.clone();
        self.key_path = h.key_path.clone();
        self.password = h.password.clone();
        self.show_password = false;
        self.remote_cwd = h.remote_cwd.clone();
        self.test_result = None;
        self.is_testing = false;
    }
}

pub fn render_ssh_host_modal(ctx: &Context, state: &mut SshHostModalState, palette: &ThemePalette) -> ModalAction {
    if !state.open {
        return ModalAction::None;
    }

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.open = false;
        return ModalAction::None;
    }

    let mut action = ModalAction::None;
    let title = if state.is_edit { "Edit SSH Remote PC" } else { "Add SSH Remote PC" };

    let mut is_open = state.open;
    Window::new(title)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(480.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);

            // Display Name
            ui.label(RichText::new("Display Name:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Dev Cloud Server, Staging VPS")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Host & Port row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Host / IP Address:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.host)
                            .hint_text("e.g. 192.168.1.100 or vps.example.com")
                            .desired_width(ui.available_width() - 90.0),
                    );
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("Port:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.port_str)
                            .hint_text("22")
                            .desired_width(75.0),
                    );
                });
            });
            ui.add_space(8.0);

            // Username
            ui.label(RichText::new("SSH Username:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.user)
                    .hint_text("e.g. ubuntu, root, ec2-user")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Private Key File
            ui.label(RichText::new("SSH Private Key Path (optional if using agent/default):").size(12.0).strong().color(palette.text_primary));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.key_path)
                        .hint_text("e.g. ~/.ssh/id_ed25519 or /home/user/.ssh/id_rsa")
                        .desired_width(ui.available_width() - 85.0),
                );
                if ui.button("Browse...").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        state.key_path = path.to_string_lossy().to_string();
                    }
                }
            });
            ui.add_space(8.0);

            // SSH Password
            ui.horizontal(|ui| {
                ui.label(RichText::new("SSH Password (optional if using key/agent):").size(12.0).strong().color(palette.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let eye_label = if state.show_password { "👁 Hide" } else { "👁 Show" };
                    if ui.small_button(eye_label).clicked() {
                        state.show_password = !state.show_password;
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut state.password)
                    .password(!state.show_password)
                    .hint_text("Enter SSH password (for password-based authentication)")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Default Remote Directory
            ui.label(RichText::new("Default Remote Directory (optional):").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.remote_cwd)
                    .hint_text("e.g. /var/www or /home/ubuntu/app")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(10.0);

            // Test connection result banner
            if let Some(res) = &state.test_result {
                match res {
                    Ok(msg) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✓").size(13.0).color(palette.success));
                            ui.label(RichText::new(msg).size(11.5).color(palette.success));
                        });
                    }
                    Err(err) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✗").size(13.0).color(palette.danger));
                            ui.label(RichText::new(err).size(11.5).color(palette.danger));
                        });
                    }
                }
                ui.add_space(8.0);
            }

            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                // Test Connection button
                let test_btn = egui::Button::new(RichText::new("⚡ Test Connection").size(11.5).color(palette.accent_light))
                    .fill(palette.bg_button)
                    .stroke(Stroke::new(1.0_f32, palette.accent));
                if ui.add(test_btn).clicked() {
                    let port = state.port_str.trim().parse::<u16>().unwrap_or(22);
                    let host_obj = crate::config::SshRemoteHost {
                        id: "test".to_string(),
                        name: state.name.clone(),
                        host: state.host.clone(),
                        port,
                        user: state.user.clone(),
                        key_path: state.key_path.clone(),
                        password: state.password.clone(),
                        remote_cwd: state.remote_cwd.clone(),
                    };
                    state.test_result = Some(crate::remote::test_ssh_connection(&host_obj));
                }

                if state.is_edit {
                    if let Some(id_to_del) = state.editing_id.clone() {
                        let del_btn = egui::Button::new(RichText::new("Delete Host").size(11.5).color(palette.danger))
                            .fill(palette.danger_subtle);
                        if ui.add(del_btn).clicked() {
                            action = ModalAction::DeleteSshHost(id_to_del);
                            state.open = false;
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save_btn = egui::Button::new(RichText::new("Save Host").size(12.0).strong().color(palette.accent_text))
                        .fill(palette.accent)
                        .rounding(Rounding::same(5.0))
                        .min_size(egui::vec2(85.0, 28.0));

                    if ui.add(save_btn).clicked() && !state.host.trim().is_empty() {
                        let port = state.port_str.trim().parse::<u16>().unwrap_or(22);
                        let name = if state.name.trim().is_empty() {
                            state.host.trim().to_string()
                        } else {
                            state.name.trim().to_string()
                        };
                        let id = state.editing_id.clone().unwrap_or_else(|| {
                            format!("ssh_{}", uuid_short())
                        });

                        let host_obj = crate::config::SshRemoteHost {
                            id,
                            name,
                            host: state.host.trim().to_string(),
                            port,
                            user: if state.user.trim().is_empty() { "ubuntu".to_string() } else { state.user.trim().to_string() },
                            key_path: state.key_path.trim().to_string(),
                            password: state.password.trim().to_string(),
                            remote_cwd: state.remote_cwd.trim().to_string(),
                        };

                        action = ModalAction::SaveSshHost(host_obj, state.editing_id.clone());
                        state.open = false;
                    }

                    if ui.button(RichText::new("Cancel").size(12.0).color(palette.text_secondary)).clicked() {
                        state.open = false;
                    }
                });
            });
        });

    state.open = is_open;
    action
}

fn uuid_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}", d.as_millis() % 0xffffff)
}

