use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppSource {
    Desktop,
    Flatpak,
    DnfRpm,
    AptDpkg,
    Pacman,
    Snap,
    Win32,
    Winget,
    Appx,
    Scoop,
    Choco,
    ChromePwa,
}

impl AppSource {
    pub fn label(&self) -> &'static str {
        match self {
            AppSource::Desktop => "GUI App",
            AppSource::Flatpak => "Flatpak",
            AppSource::DnfRpm => "DNF / RPM",
            AppSource::AptDpkg => "APT / DEB",
            AppSource::Pacman => "Pacman",
            AppSource::Snap => "Snap",
            AppSource::Win32 => "Win32",
            AppSource::Winget => "Winget",
            AppSource::Appx => "MS Store",
            AppSource::Scoop => "Scoop",
            AppSource::Choco => "Chocolatey",
            AppSource::ChromePwa => "Web App",
        }
    }

    pub fn matches_filter(&self, filter: AppFilter) -> bool {
        match filter {
            AppFilter::All => true,
            AppFilter::GuiApps => matches!(
                self,
                AppSource::Desktop | AppSource::Flatpak | AppSource::Appx | AppSource::ChromePwa
            ),
            AppFilter::Flatpak => matches!(self, AppSource::Flatpak),
            AppFilter::SystemPackages => matches!(
                self,
                AppSource::DnfRpm
                    | AppSource::AptDpkg
                    | AppSource::Pacman
                    | AppSource::Snap
                    | AppSource::Win32
                    | AppSource::Winget
                    | AppSource::Scoop
                    | AppSource::Choco
            ),
            AppFilter::WebApps => matches!(self, AppSource::ChromePwa),
            AppFilter::Repos => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFilter {
    All,
    GuiApps,
    Flatpak,
    SystemPackages,
    WebApps,
    Repos,
}

impl AppFilter {
    pub fn all() -> &'static [AppFilter] {
        &[
            AppFilter::All,
            AppFilter::GuiApps,
            AppFilter::Flatpak,
            AppFilter::SystemPackages,
            AppFilter::WebApps,
            AppFilter::Repos,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AppFilter::All => "All",
            AppFilter::GuiApps => "GUI Apps",
            AppFilter::Flatpak => "Flatpak",
            AppFilter::SystemPackages => "System Packages",
            AppFilter::WebApps => "Web Apps (Chrome)",
            AppFilter::Repos => "Repos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSortCol {
    Name,
    Source,
    Version,
    Size,
    InstalledDate,
}

impl AppSortCol {
    pub fn all() -> &'static [AppSortCol] {
        &[
            AppSortCol::Name,
            AppSortCol::Source,
            AppSortCol::Version,
            AppSortCol::Size,
            AppSortCol::InstalledDate,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            AppSortCol::Name => "Name",
            AppSortCol::Source => "Source",
            AppSortCol::Version => "Version",
            AppSortCol::Size => "Size",
            AppSortCol::InstalledDate => "Installed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppDataPath {
    pub label: String,
    pub path: PathBuf,
    pub is_cache: bool,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub source: AppSource,
    pub version: String,
    pub size_bytes: Option<u64>,
    pub size_display: String,
    pub publisher_or_origin: String,
    pub description: String,
    pub categories: String,
    pub install_location: Option<String>,
    pub data_paths: Vec<AppDataPath>,
    pub exec_cmd: Option<String>,
    pub uninstall_cmd: Option<String>,
    pub desktop_file: Option<String>,
    pub icon: Option<eframe::egui::ColorImage>,
    pub installed_date: Option<String>,
    pub installed_timestamp: Option<i64>,
    pub update_available: Option<String>,
    pub user_installed: bool,
    pub can_launch: bool,
    pub can_update: bool,
    pub can_uninstall: bool,
}

#[derive(Debug, Clone)]
pub struct RepoEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub url_or_details: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearDataMode {
    CacheOnly,
    AllAppData,
}

#[derive(Debug, Clone)]
pub enum AppAction {
    Launch(AppEntry),
    Update(AppEntry),
    UpdateAll(Vec<AppEntry>),
    Uninstall(AppEntry),
    OpenLocation(AppEntry),
    ResolveDetails(AppEntry),
    ClearData {
        app: AppEntry,
        mode: ClearDataMode,
    },
}

#[derive(Debug, Clone)]
pub enum AppsEvent {
    DiscoveryBatch {
        apps: Vec<AppEntry>,
        repos: Vec<RepoEntry>,
        done: bool,
        status: String,
    },
    UpdatesChecked {
        updates: HashMap<String, String>,
        status: String,
    },
    DetailsResolved {
        id: String,
        source: AppSource,
        install_location: Option<String>,
        description: Option<String>,
        installed_date: Option<String>,
        installed_timestamp: Option<i64>,
        data_paths: Vec<AppDataPath>,
    },
    TaskStarted {
        title: String,
    },
    TaskOutput(String),
    TaskFinished {
        success: bool,
        summary: String,
        refresh_needed: bool,
    },
}
