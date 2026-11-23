use eframe::egui;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const MANIFEST: &str =
    "https://github.com/venujatech-byte/Server_Launcher/releases/latest/download/latest.json";
const RELEASES: &str = "https://github.com/venujatech-byte/Server_Launcher/releases/latest";
const VERSION: &str = match option_env!("SERVER_LAUNCHER_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: HashMap<String, Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Manifest),
    Downloading,
    Ready(Package, tempfile::TempDir, PathBuf),
    Applying,
    InstallerOpened,
    RestartRequired,
    Failed(String),
}

fn version_tuple(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn curl() -> Command {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut curl = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(target_os = "windows")]
    curl.creation_flags(0x08000000);
    curl.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
    ]);
    curl
}

fn fetch_update() -> Result<Option<Manifest>, String> {
    let mut curl = curl();
    let output = curl
        .args(["--max-time", "20", "--max-filesize", "65536", MANIFEST])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not start curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not reach the release feed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let manifest: Manifest = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid release manifest: {e}"))?;
    let remote = version_tuple(&manifest.version).ok_or("Invalid release version")?;
    let local = version_tuple(VERSION).ok_or("Invalid installed version")?;
    let asset_prefix = format!(
        "https://github.com/venujatech-byte/Server_Launcher/releases/download/v{}/",
        manifest.version
    );
    if manifest.platforms.is_empty()
        || manifest.platforms.values().any(|asset| {
            let filename = asset.url.strip_prefix(&asset_prefix);
            !filename.is_some_and(|name| !name.is_empty() && !name.contains(['/', '?', '#']))
                || asset.sha256.len() != 64
                || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !(1..=1_073_741_824).contains(&asset.size)
        })
    {
        return Err("Invalid release assets".into());
    }
    Ok((remote > local).then_some(manifest))
}

#[derive(Clone, Copy)]
struct Package {
    key: &'static str,
    name: &'static str,
    filename: &'static str,
}

#[cfg(target_os = "windows")]
fn store_install() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentPackageFullName(length: *mut u32, name: *mut u16) -> i32;
    }
    let mut length = 0;
    // An installed AppX/MSIX reports the required buffer size without writing a name.
    unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) == 122 }
}

#[cfg(not(target_os = "windows"))]
fn store_install() -> bool {
    false
}

fn packages() -> &'static [Package] {
    if !cfg!(target_arch = "x86_64") {
        return &[];
    }
    if cfg!(target_os = "windows") {
        return &[
            Package {
                key: "windows-x64-exe",
                name: "EXE",
                filename: "server-launcher-setup.exe",
            },
            Package {
                key: "windows-x64-msi",
                name: "MSI",
                filename: "server-launcher.msi",
            },
        ];
    }
    &[
        Package {
            key: "linux-x64-deb",
            name: "DEB",
            filename: "server-launcher.deb",
        },
        Package {
            key: "linux-x64-rpm",
            name: "RPM",
            filename: "server-launcher.rpm",
        },
        Package {
            key: "linux-x64-appimage",
            name: "AppImage",
            filename: "Server-Launcher.AppImage",
        },
    ]
}

fn download(package: Package, asset: &Asset) -> Result<State, String> {
    let folder = tempfile::Builder::new()
        .prefix("server-launcher-update-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let path = folder.path().join(package.filename);
    let output = curl()
        .args([
            "--max-time",
            "300",
            "--max-filesize",
            &asset.size.to_string(),
            "--output",
        ])
        .arg(&path)
        .arg(&asset.url)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let size = io::copy(&mut file, &mut digest).map_err(|error| error.to_string())?;
    if size != asset.size || !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&asset.sha256)
    {
        return Err("Package integrity check failed.".into());
    }
    Ok(State::Ready(package, folder, path))
}

#[cfg(target_os = "linux")]
fn replace_appimage(download: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("APPIMAGE") else {
        return Ok(false);
    };
    let destination = PathBuf::from(path).canonicalize()?;
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("Invalid AppImage path"))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    io::copy(&mut std::fs::File::open(download)?, staged.as_file_mut())?;
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(
            std::fs::metadata(&destination)?.permissions().mode() & 0o777,
        ))?;
    staged.as_file().sync_all()?;
    // A failed copy or rename leaves the current AppImage intact.
    staged.persist(&destination).map_err(|error| error.error)?;
    Ok(true)
}

#[cfg(not(target_os = "linux"))]
fn replace_appimage(_: &Path) -> io::Result<bool> {
    Ok(false)
}

fn launch(package: Package, path: &Path) -> io::Result<()> {
    let mut command = match package.key {
        "windows-x64-msi" => {
            let mut cmd = Command::new("msiexec.exe");
            cmd.arg("/i").arg(path);
            cmd
        }
        "windows-x64-exe" => Command::new(path),
        "linux-x64-appimage" => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            }
            Command::new(path)
        }
        _ => {
            let mut cmd = Command::new("xdg-open");
            let status = cmd.arg(path).status()?;
            if !status.success() {
                return Err(io::Error::other(format!(
                    "The system package opener exited with {status}"
                )));
            }
            return Ok(());
        }
    };
    command.spawn()?;
    Ok(())
}

pub(crate) fn render(ui: &mut egui::Ui) {
    let storage_id = egui::Id::new("server_launcher_update_state");
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<State>>>(storage_id)
            .clone()
    });
    ui.heading("App updates");
    if store_install() {
        ui.label(format!("Installed version {VERSION}"));
        ui.hyperlink_to(
            "Check for updates in Microsoft Store",
            "ms-windows-store://downloadsandupdates",
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Installed version {VERSION}"));
        let busy = matches!(
            *state.lock().unwrap(),
            State::Checking | State::Downloading | State::Applying
        );
        if ui
            .add_enabled(!busy, egui::Button::new("Check for updates"))
            .clicked()
        {
            *state.lock().unwrap() = State::Checking;
            let state = Arc::clone(&state);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = match fetch_update() {
                    Ok(Some(manifest)) => State::Available(manifest),
                    Ok(None) => State::Current,
                    Err(error) => State::Failed(error),
                };
                *state.lock().unwrap() = result;
                ctx.request_repaint();
            });
        }
    });
    let mut requested_download = None;
    let mut install = false;
    let mut current = state.lock().unwrap();
    match &*current {
        State::Idle => {}
        State::Checking => {
            ui.label("Checking GitHub…");
        }
        State::Current => {
            ui.label("You’re up to date.");
        }
        State::Available(manifest) => {
            ui.label(format!("Version {} is available.", manifest.version));
            let running_appimage =
                cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some();
            if !running_appimage {
                ui.label("Choose the same package type you originally installed.");
            }
            ui.horizontal_wrapped(|ui| {
                for &package in packages() {
                    if running_appimage && package.key != "linux-x64-appimage" {
                        continue;
                    }
                    if let Some(asset) = manifest.platforms.get(package.key) {
                        if ui.button(format!("Download {}", package.name)).clicked() {
                            requested_download = Some((package, asset.clone()));
                        }
                    }
                }
                ui.hyperlink_to("Release downloads", RELEASES);
            });
        }
        State::Downloading => {
            ui.label("Downloading and verifying the package…");
        }
        State::Ready(package, _, _) => {
            ui.label(format!("{} package downloaded and verified.", package.name));
            if package.key == "linux-x64-appimage" {
                ui.label(if std::env::var_os("APPIMAGE").is_some() {
                    "This replaces the current AppImage. Restart Server Launcher to use the update."
                } else {
                    "This opens the new AppImage. Keep the downloaded file to use it again."
                });
            }
            install = ui.button("Open verified update").clicked();
        }
        State::Applying => {
            ui.label("Opening the installer or applying the update…");
        }
        State::RestartRequired => {
            ui.label("AppImage updated. Restart Server Launcher to finish.");
        }
        State::InstallerOpened => {
            ui.label("Update opened. Follow the installer prompts.");
        }
        State::Failed(error) => {
            ui.label(error);
            ui.hyperlink_to("Release downloads", RELEASES);
        }
    };
    if let Some((package, asset)) = requested_download {
        *current = State::Downloading;
        let state = Arc::clone(&state);
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            *state.lock().unwrap() = download(package, &asset).unwrap_or_else(State::Failed);
            ctx.request_repaint();
        });
    }
    if install {
        if let State::Ready(package, folder, path) = std::mem::take(&mut *current) {
            *current = State::Applying;
            let state = Arc::clone(&state);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = if package.key == "linux-x64-appimage" {
                    replace_appimage(&path)
                } else {
                    Ok(false)
                };
                let next = match result.and_then(|replaced| {
                    if replaced {
                        Ok(true)
                    } else {
                        launch(package, &path).map(|()| false)
                    }
                }) {
                    Ok(true) => State::RestartRequired,
                    Ok(false) => {
                        // The installer may still need the package after Server Launcher closes.
                        let _ = folder.keep();
                        State::InstallerOpened
                    }
                    Err(error) => State::Failed(format!("Could not open the update: {error}")),
                };
                *state.lock().unwrap() = next;
                ctx.request_repaint();
            });
        }
    }
}
