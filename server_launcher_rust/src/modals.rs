use crate::commands::CommandModalState;
use crate::config::{CustomAction, CustomLink, CustomScript, SavedCommand, ServerConfig};
use crate::scripts::ScriptModalState;
use crate::systemd::NewServiceDefinition;
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
    pub autostart: bool,
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
        self.autostart = false;
        self.env_text.clear();
        self.actions.clear();
        self.links.clear();
        self.error_msg = None;
    }

    #[allow(dead_code)]
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
        self.autostart = cfg.autostart;

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
            autostart: self.autostart,
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
    ToggleAppAutostart,
    ToggleServerAutostart(String),
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

    let screen = ctx.screen_rect();
    let modal_w = 560.0_f32.min(screen.width() - 40.0).max(380.0);
    let max_h = (screen.height() - 40.0).max(400.0);

    let mut is_open = state.open;
    Window::new(window_title)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(640.0)
        .max_height(max_h)
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
                .max_width(ui.available_width())
                .max_height(ui.available_height() - 55.0)
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());

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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                        let browse_w = 75.0;
                        let input_w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(80.0);
                        ui.add_sized(
                            [input_w, 20.0],
                            egui::TextEdit::singleline(&mut state.cwd)
                                .hint_text("e.g. /path/to/project or .")
                                .clip_text(true),
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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                            .clip_text(true)
                            .desired_width(ui.available_width()),
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
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Automatically restarts this server if the process exits unexpectedly or crashes")
                            .size(10.5)
                            .color(palette.text_muted),
                    );
                    ui.add_space(8.0);

                    // Run at startup checkbox
                    ui.checkbox(
                        &mut state.autostart,
                        RichText::new("🚀 Run server at startup")
                            .size(12.0)
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new("Automatically launches this server whenever Server Launcher opens")
                            .size(10.5)
                            .color(palette.text_muted),
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
                            .clip_text(true)
                            .desired_rows(3)
                            .desired_width(ui.available_width()),
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
                            ui.add(egui::TextEdit::singleline(lbl).clip_text(true).desired_width(85.0));
                            ui.label(RichText::new("cmd:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            let cmd_w = (ui.available_width() - 36.0).max(60.0);
                            ui.add_sized(
                                [cmd_w, 20.0],
                                egui::TextEdit::singleline(cmd).clip_text(true),
                            );
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
                            ui.add(egui::TextEdit::singleline(lbl).clip_text(true).desired_width(85.0));
                            ui.label(RichText::new("url:").size(10.5).color(Color32::from_rgb(123, 131, 148)));
                            let url_w = (ui.available_width() - 36.0).max(60.0);
                            ui.add_sized(
                                [url_w, 20.0],
                                egui::TextEdit::singleline(url).clip_text(true),
                            );
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

    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 100.0).max(300.0);

    let mut is_open = state.open;
    Window::new("⚡ Quick Launch (Ctrl+P)")
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .default_height(400.0)
        .max_height(max_h)
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

    let screen = ctx.screen_rect();
    let modal_w = 480.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 40.0).max(380.0);

    let mut is_open = state.open;
    Window::new(title)
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .max_height(max_h)
        .show(ctx, |ui| {
            ui.set_max_width(ui.available_width());
            ui.add_space(6.0);

            // Display Name
            ui.label(RichText::new("Display Name:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Dev Cloud Server, Staging VPS")
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Host & Port row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Host / IP Address:").size(12.0).strong().color(palette.text_primary));
                    let host_w = (ui.available_width() - 95.0).max(80.0);
                    ui.add_sized(
                        [host_w, 20.0],
                        egui::TextEdit::singleline(&mut state.host)
                            .hint_text("e.g. 192.168.1.100 or vps.example.com")
                            .clip_text(true),
                    );
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("Port:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.port_str)
                            .hint_text("22")
                            .clip_text(true)
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
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Private Key File
            ui.label(RichText::new("SSH Private Key Path (optional if using agent/default):").size(12.0).strong().color(palette.text_primary));
            ui.horizontal(|ui| {
                let browse_w = 75.0;
                let input_w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(80.0);
                ui.add_sized(
                    [input_w, 20.0],
                    egui::TextEdit::singleline(&mut state.key_path)
                        .hint_text("e.g. ~/.ssh/id_ed25519 or /home/user/.ssh/id_rsa")
                        .clip_text(true),
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
                    .clip_text(true)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Default Remote Directory
            ui.label(RichText::new("Default Remote Directory (optional):").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.remote_cwd)
                    .hint_text("e.g. /var/www or /home/ubuntu/app")
                    .clip_text(true)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseConfirmChoice {
    None,
    Cancel,
    StopAndExit,
    KeepRunningAndExit,
}

pub fn render_close_confirmation_modal(
    ctx: &Context,
    open: &mut bool,
    running_servers: &[(String, Option<u16>, Option<u32>)],
    palette: &ThemePalette,
) -> CloseConfirmChoice {
    if !*open {
        return CloseConfirmChoice::None;
    }

    let mut choice = CloseConfirmChoice::None;
    let mut is_open = *open;

    let screen = ctx.screen_rect();
    let modal_w = 460.0_f32.min(screen.width() - 40.0).max(320.0);

    Window::new("Exit Confirmation")
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(280.0)
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // Header with warning icon
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠️").size(24.0));
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Active Servers Still Running")
                                .size(16.0)
                                .strong()
                                .color(palette.text_primary),
                        );
                        ui.label(
                            RichText::new(format!(
                                "You have {} active server{} running.",
                                running_servers.len(),
                                if running_servers.len() == 1 { "" } else { "s" }
                            ))
                            .size(12.0)
                            .color(palette.text_muted),
                        );
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Would you like to stop all running servers before exiting, or leave them running in the background?")
                        .size(12.5)
                        .color(palette.text_secondary),
                );

                ui.add_space(8.0);

                // Running servers list
                egui::Frame::none()
                    .fill(palette.bg_subtle)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(100.0)
                            .show(ui, |ui| {
                                for (name, port, pid) in running_servers {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("●").size(10.0).color(palette.success));
                                        ui.label(RichText::new(name).size(12.0).strong().color(palette.text_primary));
                                        if let Some(p) = port {
                                            if *p > 0 {
                                                ui.label(RichText::new(format!(":{}", p)).size(11.0).color(palette.accent));
                                            }
                                        }
                                        if let Some(id) = pid {
                                            ui.label(RichText::new(format!("(PID {})", id)).size(10.5).color(palette.text_muted));
                                        }
                                    });
                                }
                            });
                    });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                // Action buttons: Cancel, Keep Running & Exit, Stop & Exit
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Cancel").size(12.0).color(palette.text_secondary)).clicked() {
                        choice = CloseConfirmChoice::Cancel;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_stop = egui::Button::new(
                            RichText::new("🛑 Stop & Exit").size(12.0).strong().color(Color32::WHITE),
                        )
                        .fill(palette.danger)
                        .rounding(Rounding::same(5.0))
                        .min_size(egui::vec2(100.0, 28.0));

                        if ui.add(btn_stop).clicked() {
                            choice = CloseConfirmChoice::StopAndExit;
                        }

                        let btn_keep = egui::Button::new(
                            RichText::new("Keep Running & Exit").size(12.0).color(palette.text_primary),
                        )
                        .fill(palette.bg_button)
                        .stroke(Stroke::new(1.0_f32, palette.border))
                        .rounding(Rounding::same(5.0))
                        .min_size(egui::vec2(130.0, 28.0));

                        if ui.add(btn_keep).on_hover_text("Leave processes active in the background. Server Launcher will reconnect to them when reopened.").clicked() {
                            choice = CloseConfirmChoice::KeepRunningAndExit;
                        }
                    });
                });
            });
        });

    if !is_open && choice == CloseConfirmChoice::None {
        choice = CloseConfirmChoice::Cancel;
    }

    if choice != CloseConfirmChoice::None {
        *open = false;
    }

    choice
}

// ═════════════════════════════════════════════════════════════════════════════
// SCRIPT MODAL (Create / Edit Custom Script)
// ═════════════════════════════════════════════════════════════════════════════
pub fn render_script_modal(
    ctx: &Context,
    state: &mut ScriptModalState,
    palette: &ThemePalette,
) -> Option<CustomScript> {
    if !state.open {
        return None;
    }

    let mut result = None;
    let title = if state.is_edit { "✏ Edit Custom Script" } else { "📜 New Custom Script" };

    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 40.0).max(400.0);

    let mut is_open = state.open;
    Window::new(RichText::new(title).size(15.0).strong().color(palette.text_primary))
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(480.0)
        .max_height(max_h)
        .show(ctx, |ui| {
            ui.add_space(4.0);

            // Error display if any
            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Script Name
            ui.label(RichText::new("Script Name:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Health Check, DB Backup, Build & Clean")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Shell & CWD row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Shell / Interpreter:").size(12.0).strong().color(palette.text_primary));
                    egui::ComboBox::from_id_salt("script_shell_combo")
                        .selected_text(&state.shell)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.shell, "bash".to_string(), "bash");
                            ui.selectable_value(&mut state.shell, "sh".to_string(), "sh");
                            ui.selectable_value(&mut state.shell, "python3".to_string(), "python3");
                            ui.selectable_value(&mut state.shell, "zsh".to_string(), "zsh");
                        });
                });

                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Working Directory (optional):").size(12.0).strong().color(palette.text_primary));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.cwd)
                                .hint_text("Default (current / home)")
                                .desired_width(ui.available_width() - 80.0),
                        );
                        if ui.button("Browse...").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                state.cwd = path.to_string_lossy().to_string();
                            }
                        }
                    });
                });
            });
            ui.add_space(8.0);

            // Startup Autostart Toggle
            ui.checkbox(
                &mut state.autostart,
                RichText::new("⚡ Run automatically at startup / connection")
                    .size(12.0)
                    .color(palette.accent)
                    .strong(),
            );
            ui.label(
                RichText::new("If enabled, this script will be triggered immediately when the launcher launches or connects to this host.")
                    .size(11.0)
                    .color(palette.text_muted),
            );
            ui.add_space(10.0);

            // Script Body
            ui.label(RichText::new("Script Content:").size(12.0).strong().color(palette.text_primary));
            let text_height = (ui.available_height() - 50.0).max(160.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut state.script_body)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(10)
                        .desired_width(ui.available_width())
                        .min_size(egui::vec2(0.0, text_height)),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Cancel").color(palette.text_secondary)).clicked() {
                    state.open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("💾 Save Script").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(100.0, 28.0));

                    if ui.add(btn_save).clicked() {
                        if state.name.trim().is_empty() {
                            state.error = Some("Script name cannot be empty".to_string());
                        } else if state.script_body.trim().is_empty() {
                            state.error = Some("Script body cannot be empty".to_string());
                        } else {
                            result = Some(CustomScript {
                                id: state.id.clone(),
                                name: state.name.trim().to_string(),
                                script: state.script_body.clone(),
                                shell: state.shell.trim().to_string(),
                                cwd: state.cwd.trim().to_string(),
                                autostart: state.autostart,
                                host_id: state.host_id.clone(),
                            });
                            state.open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

// ═════════════════════════════════════════════════════════════════════════════
// SAVED COMMAND MODAL (Create / Edit Custom Command)
// ═════════════════════════════════════════════════════════════════════════════
pub fn render_command_modal(
    ctx: &Context,
    state: &mut CommandModalState,
    palette: &ThemePalette,
) -> Option<SavedCommand> {
    if !state.open {
        return None;
    }

    let mut result = None;
    let title = if state.is_edit { "✏ Edit Saved Command" } else { "⚡ Save Custom Command" };

    let screen = ctx.screen_rect();
    let modal_w = 480.0_f32.min(screen.width() - 40.0).max(340.0);
    let max_h = (screen.height() - 40.0).max(340.0);

    let mut is_open = state.open;
    Window::new(RichText::new(title).size(15.0).strong().color(palette.text_primary))
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(320.0)
        .max_height(max_h)
        .show(ctx, |ui| {
            ui.add_space(4.0);

            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Command Name
            ui.label(RichText::new("Name / Label:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("e.g. Git Status & Log, Prune Docker, Disk Free")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Category & Host
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Category:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.category)
                            .hint_text("e.g. Git, Docker, System, Deploy")
                            .desired_width(200.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Scope:").size(12.0).strong().color(palette.text_primary));
                    let scope_str = if let Some(h) = &state.host_id {
                        format!("Host: {}", h)
                    } else {
                        "Local PC".to_string()
                    };
                    ui.label(RichText::new(scope_str).size(12.0).color(palette.text_muted));
                });
            });
            ui.add_space(8.0);

            // Command String
            ui.label(RichText::new("Command to Execute:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::multiline(&mut state.command)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Description
            ui.label(RichText::new("Description (optional):").size(12.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut state.description)
                    .hint_text("Short note on what this command does")
                    .desired_width(ui.available_width()),
            );

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Cancel").color(palette.text_secondary)).clicked() {
                    state.open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_save = egui::Button::new(
                        RichText::new("💾 Save Command").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(100.0, 28.0));

                    if ui.add(btn_save).clicked() {
                        if state.name.trim().is_empty() {
                            state.error = Some("Command name cannot be empty".to_string());
                        } else if state.command.trim().is_empty() {
                            state.error = Some("Command string cannot be empty".to_string());
                        } else {
                            result = Some(SavedCommand {
                                id: state.id.clone(),
                                name: state.name.trim().to_string(),
                                command: state.command.trim().to_string(),
                                description: state.description.trim().to_string(),
                                category: if state.category.trim().is_empty() {
                                    "General".to_string()
                                } else {
                                    state.category.trim().to_string()
                                },
                                host_id: state.host_id.clone(),
                            });
                            state.open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}

// ═════════════════════════════════════════════════════════════════════════════
// SYSTEMD SERVICE CREATION MODAL
// ═════════════════════════════════════════════════════════════════════════════
#[derive(Debug, Clone, Default)]
pub struct NewServiceModalState {
    pub open: bool,
    pub def: NewServiceDefinition,
    pub error: Option<String>,
}

impl NewServiceModalState {
    pub fn open(&mut self) {
        self.open = true;
        self.def = NewServiceDefinition::default();
        self.error = None;
    }
}

pub fn render_new_service_modal(
    ctx: &Context,
    state: &mut NewServiceModalState,
    palette: &ThemePalette,
) -> Option<NewServiceDefinition> {
    if !state.open {
        return None;
    }

    let mut result = None;
    let screen = ctx.screen_rect();
    let modal_w = 540.0_f32.min(screen.width() - 40.0).max(360.0);
    let max_h = (screen.height() - 40.0).max(400.0);
    let mut is_open = state.open;

    Window::new(RichText::new("⚙ Create New SystemMD Service").size(15.0).strong().color(palette.text_primary))
        .open(&mut is_open)
        .collapsible(false)
        .resizable(false)
        .min_width(modal_w)
        .max_width(modal_w)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_height(480.0)
        .max_height(max_h)
        .show(ctx, |ui| {
            ui.add_space(4.0);

            if let Some(err) = &state.error {
                ui.label(RichText::new(err).size(11.5).color(palette.danger));
                ui.add_space(4.0);
            }

            // Unit Name & Scope
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Service Unit Name:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.def.name)
                            .hint_text("e.g. my-app.service")
                            .desired_width(280.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Scope:").size(12.0).strong().color(palette.text_primary));
                    ui.checkbox(&mut state.def.is_user, "User Service (--user)");
                });
            });
            ui.add_space(8.0);

            // Description
            ui.label(RichText::new("Description:").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.description)
                    .hint_text("e.g. My Background Web Service")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // ExecStart
            ui.label(RichText::new("ExecStart (Executable & Arguments):").size(12.0).strong().color(palette.text_primary));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.exec_start)
                    .hint_text("e.g. /usr/bin/python3 /home/user/app/server.py")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(8.0);

            // Working Directory & Restart
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Working Directory:").size(12.0).strong().color(palette.text_primary));
                    ui.add(
                        egui::TextEdit::singleline(&mut state.def.working_dir)
                            .hint_text("e.g. /home/user/app")
                            .desired_width(ui.available_width() - 140.0),
                    );
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.label(RichText::new("Restart Policy:").size(12.0).strong().color(palette.text_primary));
                    egui::ComboBox::from_id_salt("service_restart_combo")
                        .selected_text(&state.def.restart)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut state.def.restart, "always".to_string(), "always");
                            ui.selectable_value(&mut state.def.restart, "on-failure".to_string(), "on-failure");
                            ui.selectable_value(&mut state.def.restart, "no".to_string(), "no");
                        });
                });
            });
            ui.add_space(8.0);

            // Environment
            ui.label(RichText::new("Environment Variables (e.g. PORT=8080 NODE_ENV=production):").size(12.0).color(palette.text_muted));
            ui.add(
                egui::TextEdit::singleline(&mut state.def.environment)
                    .hint_text("PORT=8080")
                    .desired_width(ui.available_width()),
            );

            ui.add_space(8.0);

            // Generated Preview
            ui.collapsing("🔍 Preview Generated Unit File", |ui| {
                let preview = state.def.generate_service_content();
                ui.add(
                    egui::TextEdit::multiline(&mut preview.as_str())
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(6)
                        .desired_width(ui.available_width()),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            // Action Buttons
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Cancel").color(palette.text_secondary)).clicked() {
                    state.open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn_create = egui::Button::new(
                        RichText::new("🚀 Create & Enable Service").size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(palette.accent)
                    .rounding(Rounding::same(5.0))
                    .min_size(egui::vec2(150.0, 28.0));

                    if ui.add(btn_create).clicked() {
                        if state.def.name.trim().is_empty() {
                            state.error = Some("Service name cannot be empty".to_string());
                        } else if state.def.exec_start.trim().is_empty() {
                            state.error = Some("ExecStart cannot be empty".to_string());
                        } else {
                            result = Some(state.def.clone());
                            state.open = false;
                        }
                    }
                });
            });
        });

    if !is_open {
        state.open = false;
    }

    result
}


