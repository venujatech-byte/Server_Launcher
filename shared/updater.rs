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
const RELEASE_API: &str =
    "https://api.github.com/repos/venujatech-byte/Server_Launcher/releases/latest";
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
    Available(Manifest, Option<Package>),
    Downloading,
    Ready(Package, tempfile::TempDir, PathBuf),
    Closing,
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

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

fn fetch_legacy_release() -> Result<Manifest, String> {
    let output = curl()
        .args([
            "--max-time",
            "20",
            "--max-filesize",
            "262144",
            "--user-agent",
            "Server-Launcher-Updater",
            "--header",
            "Accept: application/vnd.github+json",
            RELEASE_API,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "GitHub release lookup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let release: GitHubRelease = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Invalid GitHub release response: {error}"))?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("Release has no version tag")?
        .to_owned();
    let mut platforms = HashMap::new();
    for asset in release.assets {
        let name = asset.name.to_ascii_lowercase();
        let key = if name.contains("x64") && name.ends_with("-setup.exe") {
            "windows-x64-exe"
        } else if name.contains("x64") && name.ends_with(".msi") {
            "windows-x64-msi"
        } else if name.contains("amd64") && name.ends_with(".deb") {
            "linux-x64-deb"
        } else if name.contains("x86_64") && name.ends_with(".rpm") {
            "linux-x64-rpm"
        } else if name.contains("amd64") && name.ends_with(".appimage") {
            "linux-x64-appimage"
        } else {
            continue;
        };
        if let Some(digest) = asset
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
        {
            let previous = platforms.insert(
                key.to_owned(),
                Asset {
                    url: asset.browser_download_url,
                    sha256: digest.to_owned(),
                    size: asset.size,
                },
            );
            if previous.is_some() {
                return Err(format!("GitHub lists multiple packages for {key}"));
            }
        }
    }
    Ok(Manifest { version, platforms })
}

fn fetch_update() -> Result<Option<Manifest>, String> {
    let mut curl = curl();
    let output = curl
        .args([
            "--max-time",
            "20",
            "--max-filesize",
            "65536",
            "--write-out",
            "\n%{http_code}",
            MANIFEST,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not start curl: {e}"))?;
    let status_suffix = output.stdout.rsplitn(2, |byte| *byte == b'\n').next();
    let manifest: Manifest = if status_suffix == Some(b"404") {
        fetch_legacy_release()?
    } else {
        if !output.status.success() {
            return Err(format!(
                "Could not reach the release feed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let manifest_body = output
            .stdout
            .rsplitn(2, |byte| *byte == b'\n')
            .nth(1)
            .ok_or("The release feed returned no HTTP status")?;
        serde_json::from_slice(manifest_body)
            .map_err(|e| format!("Invalid release manifest: {e}"))?
    };
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

fn package_for_install() -> Option<Package> {
    if !cfg!(target_arch = "x86_64") {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().ok()?;
        let folder = exe.parent()?;
        if folder.join("uninstall.exe").is_file() {
            return Some(Package {
                key: "windows-x64-exe",
                name: "Windows EXE",
                filename: "server-launcher-setup.exe",
            });
        }
        let result = Command::new("powershell.exe")
            .creation_flags(0x08000000)
            .env("SL_INSTALL_DIR", folder)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
                $keys = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
                Get-ItemProperty $keys -ErrorAction SilentlyContinue |
                    Where-Object { $_.WindowsInstaller -eq 1 -and
                        $_.DisplayName -like 'Server Launcher*' -and $_.InstallLocation -and
                        $_.InstallLocation.TrimEnd('\') -eq $env:SL_INSTALL_DIR.TrimEnd('\') } |
                    Select-Object -First 1 -ExpandProperty WindowsInstaller
            "#,
            ])
            .output()
            .ok()?;
        return (String::from_utf8_lossy(&result.stdout).trim() == "1").then_some(Package {
            key: "windows-x64-msi",
            name: "Windows MSI",
            filename: "server-launcher.msi",
        });
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some() {
            return Some(Package {
                key: "linux-x64-appimage",
                name: "AppImage",
                filename: "Server-Launcher.AppImage",
            });
        }
        let release = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .ok()?;
        return linux_package(&release).filter(|package| {
            Path::new("/usr/bin/pkexec").is_file() && linux_installer(package.key).is_some()
        });
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

#[cfg(target_os = "linux")]
fn linux_package(release: &str) -> Option<Package> {
    for field in ["ID=", "ID_LIKE="] {
        let Some(value) = release.lines().find_map(|line| line.strip_prefix(field)) else {
            continue;
        };
        for id in value
            .trim_matches(|c| c == '"' || c == char::from(39))
            .split_whitespace()
        {
            match id {
                "fedora"
                | "rhel"
                | "centos"
                | "rocky"
                | "almalinux"
                | "suse"
                | "opensuse"
                | "opensuse-leap"
                | "opensuse-tumbleweed" => {
                    return Some(Package {
                        key: "linux-x64-rpm",
                        name: "RPM",
                        filename: "server-launcher.rpm",
                    })
                }
                "debian" | "ubuntu" | "linuxmint" | "pop" => {
                    return Some(Package {
                        key: "linux-x64-deb",
                        name: "DEB",
                        filename: "server-launcher.deb",
                    })
                }
                _ => {}
            }
        }
    }
    None
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

#[cfg(not(any(target_os = "linux", windows)))]
fn replace_appimage(_: &Path) -> io::Result<bool> {
    Ok(false)
}

type Staged = (Package, tempfile::TempDir, PathBuf);
static PENDING: Mutex<Option<Staged>> = Mutex::new(None);
const STATE_ID: &str = "server_launcher_update_state";

pub(crate) fn close_for_update(ctx: &egui::Context) {
    let Some(state) = ctx.data(|data| data.get_temp::<Arc<Mutex<State>>>(egui::Id::new(STATE_ID)))
    else {
        return;
    };
    let mut state = state.lock().unwrap();
    if matches!(*state, State::Ready(..)) {
        if let State::Ready(package, folder, path) = std::mem::replace(&mut *state, State::Closing)
        {
            *PENDING.lock().unwrap() = Some((package, folder, path));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

pub(crate) fn cancel_install(ctx: &egui::Context) {
    if PENDING.lock().unwrap().take().is_some() {
        if let Some(state) =
            ctx.data(|data| data.get_temp::<Arc<Mutex<State>>>(egui::Id::new(STATE_ID)))
        {
            *state.lock().unwrap() = State::Idle;
        }
    }
}

pub(crate) fn install_pending() {
    let Some((package, folder, path)) = PENDING.lock().unwrap().take() else {
        return;
    };
    if let Err(error) = install(package, folder, &path) {
        eprintln!("Server Launcher update failed: {error}");
    }
}

#[cfg(windows)]
fn install(package: Package, folder: tempfile::TempDir, path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    // Wait for the old process to release its executable before starting setup.
    Command::new("powershell.exe")
        .creation_flags(0x08000000)
        .env("SL_UPDATE_PID", std::process::id().to_string())
        .env("SL_UPDATE_PATH", path)
        .env("SL_UPDATE_DIR", folder.path())
        .env("SL_UPDATE_FORMAT", package.key)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            r#"
            Wait-Process -Id $env:SL_UPDATE_PID -ErrorAction SilentlyContinue
            if ($env:SL_UPDATE_FORMAT -eq 'windows-x64-msi') {
                $arguments = '/i "' + $env:SL_UPDATE_PATH + '"'
                Start-Process msiexec.exe -ArgumentList $arguments -Wait
            } else {
                Start-Process -FilePath $env:SL_UPDATE_PATH -Wait
            }
            Remove-Item -LiteralPath $env:SL_UPDATE_DIR -Recurse -Force
        "#,
        ])
        .spawn()?;
    let _ = folder.keep();
    Ok(())
}

#[cfg(not(windows))]
fn linux_installer(key: &str) -> Option<&'static str> {
    let candidates: &[&str] = match key {
        "linux-x64-deb" => &["/usr/bin/apt-get"],
        "linux-x64-rpm" => &["/usr/bin/dnf", "/usr/bin/zypper", "/usr/bin/yum"],
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .find(|path| Path::new(path).is_file())
}

#[cfg(not(windows))]
fn install(package: Package, folder: tempfile::TempDir, path: &Path) -> io::Result<()> {
    if package.key == "linux-x64-appimage" {
        if !replace_appimage(path)? {
            return Err(io::Error::other("Running AppImage path is unavailable"));
        }
        let image = std::env::var_os("APPIMAGE").unwrap();
        Command::new(image)
            .env_remove("APPIMAGE")
            .env_remove("APPDIR")
            .spawn()?;
        return Ok(());
    }
    let manager = linux_installer(package.key)
        .ok_or_else(|| io::Error::other("No supported package manager is installed"))?;
    let installed_launcher = Path::new("/usr/bin/server_launcher");
    let mut cmd = Command::new("/usr/bin/pkexec");
    cmd.arg(manager);
    if manager.ends_with("/zypper") {
        cmd.args(["--non-interactive", "install"]);
    } else {
        cmd.args(["-y", "install"]);
    }
    let status = cmd.arg(path).status()?;
    drop(folder);
    if !status.success() {
        return Err(io::Error::other(format!("Installer exited with {status}")));
    }
    Command::new(installed_launcher).spawn()?;
    Ok(())
}

pub(crate) fn render(ui: &mut egui::Ui, palette: &crate::theme::ThemePalette) {
    let state = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<State>>>(egui::Id::new(STATE_ID))
            .clone()
    });
    egui::Frame::none()
        .fill(palette.bg_subtle)
        .rounding(12.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new("App updates")
                    .size(14.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.label(
                egui::RichText::new(format!("Installed version {VERSION}"))
                    .size(11.5)
                    .color(palette.text_muted),
            );
            if store_install() {
                ui.hyperlink_to(
                    "Check for updates in Microsoft Store",
                    "ms-windows-store://downloadsandupdates",
                );
                return;
            }
            let mut current = state.lock().unwrap();
            let busy = matches!(
                *current,
                State::Checking | State::Downloading | State::Ready(..) | State::Closing
            );
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(egui::RichText::new("Check for updates").size(11.5))
                        .fill(palette.bg_button)
                        .stroke(egui::Stroke::NONE)
                        .rounding(8.0),
                )
                .clicked()
            {
                *current = State::Checking;
                let state = Arc::clone(&state);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = match fetch_update() {
                        Ok(Some(manifest)) => State::Available(manifest, package_for_install()),
                        Ok(None) => State::Current,
                        Err(error) => State::Failed(error),
                    };
                    *state.lock().unwrap() = result;
                    ctx.request_repaint();
                });
            }
            let mut request = None;
            match &*current {
                State::Idle => {}
                State::Checking => {
                    ui.spinner();
                    ui.label("Checking GitHub…");
                }
                State::Current => {
                    ui.label("You’re up to date.");
                }
                State::Available(manifest, package) => {
                    ui.label(
                        egui::RichText::new(format!("Version {} is available", manifest.version))
                            .strong(),
                    );
                    if let Some((package, asset)) =
                        package.and_then(|p| manifest.platforms.get(p.key).map(|a| (p, a)))
                    {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} update · Server Launcher closes before installation",
                                package.name
                            ))
                            .size(11.5)
                            .color(palette.text_muted),
                        );
                        let install = egui::Button::new(
                            egui::RichText::new("Install update")
                                .size(11.5)
                                .color(palette.accent_text),
                        )
                        .fill(palette.accent)
                        .stroke(egui::Stroke::NONE)
                        .rounding(8.0);
                        if ui.add(install).clicked() {
                            request = Some((package, asset.clone()));
                        }
                    } else {
                        ui.label("No matching installer was found for this installation.");
                        ui.hyperlink_to("Release downloads", RELEASES);
                    }
                }
                State::Downloading => {
                    ui.spinner();
                    ui.label("Downloading and verifying…");
                }
                State::Ready(..) | State::Closing => {
                    ui.label("Update verified. Closing Server Launcher…");
                }
                State::Failed(error) => {
                    ui.label(egui::RichText::new(error).color(palette.danger));
                    ui.hyperlink_to("Release downloads", RELEASES);
                }
            }
            if let Some((package, asset)) = request {
                *current = State::Downloading;
                let state = Arc::clone(&state);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    *state.lock().unwrap() =
                        download(package, &asset).unwrap_or_else(State::Failed);
                    ctx.request_repaint();
                });
            }
        });
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux_package;

    #[test]
    fn selects_distro_package_without_guessing_for_unknown_distros() {
        assert_eq!(linux_package("ID=fedora\n").unwrap().key, "linux-x64-rpm");
        assert_eq!(
            linux_package("ID=custom\nID_LIKE=\"ubuntu debian\"\n")
                .unwrap()
                .key,
            "linux-x64-deb"
        );
        assert!(linux_package("ID=arch\n").is_none());
    }
}
