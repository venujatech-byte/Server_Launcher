pub mod actions;
pub mod model;
pub mod ui;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

use eframe::egui;
use model::{AppAction, AppEntry, AppFilter, AppSortCol, AppSource, AppsEvent, ClearDataMode, RepoEntry};
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};

pub struct AppsManagerState {
    pub apps: Vec<AppEntry>,
    pub repos: Vec<RepoEntry>,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub filter: AppFilter,
    pub sort_col: AppSortCol,
    pub sort_desc: bool,
    pub selected_key: Option<(String, AppSource)>,
    pub inspector_scroll_to_top: bool,
    pub resolved_keys: HashSet<(String, AppSource)>,
    pub loading: bool,
    pub checking_updates: bool,
    pub status_text: String,
    pub task_running: bool,
    pub task_title: String,
    pub task_logs: Vec<String>,
    pub log_drawer_open: bool,
    pub confirm_uninstall: Option<AppEntry>,
    pub confirm_clear_data: Option<(AppEntry, ClearDataMode)>,
    pub tx: Sender<AppsEvent>,
    pub rx: Receiver<AppsEvent>,
    pub initialized: bool,
    dirty_filter: bool,
}

impl Default for AppsManagerState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            apps: Vec::new(),
            repos: Vec::new(),
            filtered_indices: Vec::new(),
            search_query: String::new(),
            filter: AppFilter::All,
            sort_col: AppSortCol::Name,
            sort_desc: false,
            selected_key: None,
            inspector_scroll_to_top: false,
            resolved_keys: HashSet::new(),
            loading: false,
            checking_updates: false,
            status_text: String::new(),
            task_running: false,
            task_title: String::new(),
            task_logs: Vec::new(),
            log_drawer_open: false,
            confirm_uninstall: None,
            confirm_clear_data: None,
            tx,
            rx,
            initialized: false,
            dirty_filter: true,
        }
    }
}

impl AppsManagerState {
    pub fn trigger_discovery(&mut self, ctx: &egui::Context) {
        self.loading = true;
        self.status_text = "Discovering installed apps & packages…".to_string();
        self.apps.clear();
        self.filtered_indices.clear();
        self.repos.clear();
        self.resolved_keys.clear();
        self.dirty_filter = true;

        #[cfg(target_os = "linux")]
        linux::discover_all(self.tx.clone(), ctx.clone());
        #[cfg(target_os = "windows")]
        windows::discover_all(self.tx.clone(), ctx.clone());
    }

    pub fn trigger_check_updates(&mut self, ctx: &egui::Context) {
        if self.checking_updates || self.loading || self.task_running {
            return;
        }
        self.checking_updates = true;
        self.status_text = "Checking for updates…".to_string();

        #[cfg(target_os = "linux")]
        linux::check_updates(self.tx.clone(), ctx.clone());
        #[cfg(target_os = "windows")]
        windows::check_updates(self.tx.clone(), ctx.clone());
    }

    pub fn dispatch_action(&mut self, action: AppAction, ctx: &egui::Context) {
        match action {
            AppAction::Launch(app) => {
                #[cfg(target_os = "linux")]
                let res = linux::launch_app(&app);
                #[cfg(target_os = "windows")]
                let res = windows::launch_app(&app);
                #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                let res: Result<(), String> = Err("Unsupported OS".to_string());

                match res {
                    Ok(()) => self.status_text = format!("Launched {}.", app.name),
                    Err(e) => self.status_text = format!("Launch error: {}", e),
                }
            }
            AppAction::OpenLocation(app) => {
                #[cfg(target_os = "linux")]
                let res = linux::open_app_location(&app);
                #[cfg(target_os = "windows")]
                let res = windows::open_app_location(&app);
                #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                let res: Result<(), String> = Err("Unsupported OS".to_string());

                if let Err(e) = res {
                    self.status_text = e;
                }
            }
            AppAction::ResolveDetails(app) => {
                let key = (app.id.clone(), app.source);
                if self.resolved_keys.insert(key) {
                    #[cfg(target_os = "linux")]
                    linux::resolve_details(app, self.tx.clone(), ctx.clone());
                    #[cfg(target_os = "windows")]
                    windows::resolve_details(app, self.tx.clone(), ctx.clone());
                }
            }
            AppAction::Update(app) => {
                if self.task_running || self.loading || self.checking_updates {
                    return;
                }
                self.task_running = true;
                #[cfg(target_os = "linux")]
                linux::run_update(app, self.tx.clone(), ctx.clone());
                #[cfg(target_os = "windows")]
                windows::run_update(app, self.tx.clone(), ctx.clone());
            }
            AppAction::UpdateAll(apps) => {
                if self.task_running || self.loading || self.checking_updates || apps.is_empty() {
                    return;
                }
                self.task_running = true;
                #[cfg(target_os = "linux")]
                linux::run_update_all(apps, self.tx.clone(), ctx.clone());
                #[cfg(target_os = "windows")]
                windows::run_update_all(apps, self.tx.clone(), ctx.clone());
            }
            AppAction::Uninstall(app) => {
                if self.task_running || self.loading || self.checking_updates {
                    return;
                }
                self.task_running = true;
                #[cfg(target_os = "linux")]
                linux::run_uninstall(app, self.tx.clone(), ctx.clone());
                #[cfg(target_os = "windows")]
                windows::run_uninstall(app, self.tx.clone(), ctx.clone());
            }
            AppAction::ClearData { app, mode } => {
                if self.task_running || self.loading || self.checking_updates {
                    return;
                }
                self.task_running = true;
                #[cfg(target_os = "linux")]
                linux::run_clear_data(app, mode, self.tx.clone(), ctx.clone());
                #[cfg(target_os = "windows")]
                windows::run_clear_data(app, mode, self.tx.clone(), ctx.clone());
            }
        }
    }

    pub fn poll_events(&mut self, ctx: &egui::Context) {
        let mut refresh_after = false;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppsEvent::DiscoveryBatch {
                    apps,
                    repos,
                    done,
                    status,
                } => {
                    self.apps.extend(apps);
                    if !repos.is_empty() {
                        self.repos.extend(repos);
                    }
                    if done {
                        self.loading = false;
                    }
                    self.status_text = status;
                    self.dirty_filter = true;
                }
                AppsEvent::UpdatesChecked { updates, status } => {
                    self.checking_updates = false;
                    for app in &mut self.apps {
                        app.update_available = updates.get(&app.id).cloned();
                    }
                    self.status_text = status;
                    self.dirty_filter = true;
                }
                AppsEvent::DetailsResolved {
                    id,
                    source,
                    install_location,
                    description,
                    installed_date,
                    installed_timestamp,
                    data_paths,
                } => {
                    if let Some(app) = self.apps.iter_mut().find(|a| a.id == id && a.source == source) {
                        if install_location.is_some() {
                            app.install_location = install_location;
                        }
                        if let Some(desc) = description {
                            if !desc.is_empty() {
                                app.description = desc;
                            }
                        }
                        if installed_date.is_some() {
                            app.installed_date = installed_date;
                        }
                        if installed_timestamp.is_some() {
                            app.installed_timestamp = installed_timestamp;
                        }
                        app.data_paths = data_paths;
                        self.dirty_filter = true;
                    }
                }
                AppsEvent::TaskStarted { title } => {
                    self.task_running = true;
                    self.task_title = title;
                    self.task_logs.clear();
                    self.log_drawer_open = true;
                    self.status_text = self.task_title.clone();
                }
                AppsEvent::TaskOutput(line) => {
                    self.task_logs.push(line);
                    if self.task_logs.len() > 2000 {
                        let excess = self.task_logs.len() - 2000;
                        self.task_logs.drain(0..excess);
                    }
                }
                AppsEvent::TaskFinished {
                    success: _,
                    summary,
                    refresh_needed,
                } => {
                    self.task_running = false;
                    self.status_text = summary.clone();
                    self.task_logs.push(format!("==> {}", summary));
                    if refresh_needed {
                        refresh_after = true;
                    }
                }
            }
        }
        if refresh_after {
            self.trigger_discovery(ctx);
        }
        if self.dirty_filter {
            self.rebuild_filter();
        }
    }

    pub fn mark_filter_dirty(&mut self) {
        self.dirty_filter = true;
    }

    pub fn rebuild_filter(&mut self) {
        self.dirty_filter = false;
        let q = self.search_query.trim().to_lowercase();
        let filter = self.filter;

        let mut indices: Vec<usize> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                if !a.source.matches_filter(filter) {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                a.name.to_lowercase().contains(&q)
                    || a.id.to_lowercase().contains(&q)
                    || a.description.to_lowercase().contains(&q)
                    || a.publisher_or_origin.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect();

        let apps = &self.apps;
        let col = self.sort_col;
        let desc = self.sort_desc;
        indices.sort_unstable_by(|&ia, &ib| {
            let a = &apps[ia];
            let b = &apps[ib];
            let ord = match col {
                AppSortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                AppSortCol::Source => a.source.label().cmp(b.source.label()),
                AppSortCol::Version => a.version.cmp(&b.version),
                AppSortCol::Size => a.size_bytes.unwrap_or(0).cmp(&b.size_bytes.unwrap_or(0)),
                AppSortCol::InstalledDate => a
                    .installed_timestamp
                    .unwrap_or(0)
                    .cmp(&b.installed_timestamp.unwrap_or(0)),
            };
            if desc { ord.reverse() } else { ord }
        });

        self.filtered_indices = indices;
    }
}
