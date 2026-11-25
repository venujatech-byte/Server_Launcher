use super::model::{AppDataPath, AppEntry, AppsEvent, ClearDataMode};
use eframe::egui;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

/// Compute total size of a file or directory safely (skipping symlinks to avoid cycles).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        total = total.saturating_add(dir_size(&entry.path()));
    }
    total
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        "—".to_string()
    }
}

pub fn populate_data_path_sizes(paths: &mut [AppDataPath]) {
    for dp in paths.iter_mut() {
        if dp.path.exists() {
            dp.size_bytes = Some(dir_size(&dp.path));
        }
    }
}

pub fn open_folder_in_host(path: &Path) -> Result<(), String> {
    let target = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    if !target.exists() {
        return Err(format!("Path does not exist: {}", target.display()));
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to open folder with xdg-open: {}", e))?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer.exe")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to open folder with explorer.exe: {}", e))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("Unsupported OS".to_string())
}

/// Spawn a command, stream stdout and stderr line-by-line to the UI log drawer,
/// and return whether it exited with status 0.
pub fn stream_command_output(
    cmd: &mut Command,
    tx: &Sender<AppsEvent>,
    ctx: &egui::Context,
) -> Result<bool, String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {}", e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let tx_out = tx.clone();
    let ctx_out = ctx.clone();
    let out_handle = std::thread::spawn(move || {
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(AppsEvent::TaskOutput(line));
                ctx_out.request_repaint();
            }
        }
    });

    let tx_err = tx.clone();
    let ctx_err = ctx.clone();
    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(AppsEvent::TaskOutput(format!("[stderr] {}", line)));
                ctx_err.request_repaint();
            }
        }
    });

    let status = child.wait().map_err(|e| format!("Failed waiting for process: {}", e))?;
    let _ = out_handle.join();
    let _ = err_handle.join();

    Ok(status.success())
}

/// Delete verified app data paths according to `mode` and stream progress.
pub fn clear_app_data(
    app: &AppEntry,
    mode: ClearDataMode,
    tx: &Sender<AppsEvent>,
    ctx: &egui::Context,
) {
    let targets: Vec<&AppDataPath> = app
        .data_paths
        .iter()
        .filter(|dp| match mode {
            ClearDataMode::CacheOnly => dp.is_cache,
            ClearDataMode::AllAppData => true,
        })
        .collect();

    if targets.is_empty() {
        let _ = tx.send(AppsEvent::TaskFinished {
            success: false,
            summary: format!("No matching data folders found for {}.", app.name),
            refresh_needed: false,
        });
        ctx.request_repaint();
        return;
    }

    let mut freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for dp in targets {
        if !dp.path.exists() {
            let _ = tx.send(AppsEvent::TaskOutput(format!(
                "Skipped (not found): {}",
                dp.path.display()
            )));
            continue;
        }
        let sz = dir_size(&dp.path);
        let _ = tx.send(AppsEvent::TaskOutput(format!(
            "Removing {} ({}): {}",
            dp.label,
            format_bytes(sz),
            dp.path.display()
        )));
        ctx.request_repaint();

        let res = if dp.path.is_dir() {
            std::fs::remove_dir_all(&dp.path)
        } else {
            std::fs::remove_file(&dp.path)
        };
        match res {
            Ok(()) => {
                freed = freed.saturating_add(sz);
                let _ = tx.send(AppsEvent::TaskOutput(format!(
                    "Removed {}",
                    dp.path.display()
                )));
            }
            Err(e) => {
                let msg = format!("Failed to remove {}: {}", dp.path.display(), e);
                let _ = tx.send(AppsEvent::TaskOutput(format!("[error] {}", msg)));
                errors.push(msg);
            }
        }
        ctx.request_repaint();
    }

    if errors.is_empty() {
        let _ = tx.send(AppsEvent::TaskFinished {
            success: true,
            summary: format!("Cleared {} for {} (freed {}).", match mode {
                ClearDataMode::CacheOnly => "cache",
                ClearDataMode::AllAppData => "app data",
            }, app.name, format_bytes(freed)),
            refresh_needed: false,
        });
    } else {
        let _ = tx.send(AppsEvent::TaskFinished {
            success: false,
            summary: format!("Completed with {} error(s); freed {}.", errors.len(), format_bytes(freed)),
            refresh_needed: false,
        });
    }
    ctx.request_repaint();
}
