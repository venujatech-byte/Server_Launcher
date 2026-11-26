use super::{
    backend::{self, Event},
    model, SshRemoteHost,
};
use crate::remote::{build_scp_command, shell_quote, ssh_probe_output_cancellable};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct Progress {
    pub direction: &'static str,
    pub name: String,
    pub next: Option<String>,
    pub completed: usize,
    pub count: usize,
    pub bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed: Option<u64>,
}

#[derive(Debug)]
pub(super) enum Transfer {
    UploadFiles(String),
    UploadFolder(String),
    Dropped(String, Vec<PathBuf>),
    Download(Vec<String>, bool),
}

pub(super) fn run(
    host: &SshRemoteHost,
    request: Transfer,
    tx: &Sender<Event>,
    ctx: &egui::Context,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let picker = || rfd::FileDialog::new().set_title("Choose files or a folder");
    match request {
        Transfer::UploadFiles(directory) => upload(
            host,
            &directory,
            picker().pick_files().unwrap_or_default(),
            tx,
            ctx,
            cancel,
        ),
        Transfer::UploadFolder(directory) => upload(
            host,
            &directory,
            picker().pick_folder().into_iter().collect(),
            tx,
            ctx,
            cancel,
        ),
        Transfer::Dropped(directory, paths) => upload(host, &directory, paths, tx, ctx, cancel),
        Transfer::Download(paths, single_file) => {
            let chosen = if single_file {
                let Some(path) = picker()
                    .set_title("Save remote file")
                    .set_file_name(paths[0].rsplit('/').next().unwrap_or("Download"))
                    .save_file()
                else {
                    return Ok("Download cancelled.".into());
                };
                let directory = path
                    .parent()
                    .ok_or("Invalid download destination.")?
                    .to_owned();
                (directory, vec![path])
            } else {
                let Some(directory) = picker()
                    .set_title("Choose the download destination")
                    .pick_folder()
                else {
                    return Ok("Download cancelled.".into());
                };
                let destinations: Result<Vec<_>, _> = paths
                    .iter()
                    .map(|source| {
                        let name = source
                            .rsplit('/')
                            .next()
                            .filter(|name| model::valid_name(name))
                            .ok_or("Invalid remote file name.")?;
                        Ok::<_, &str>(directory.join(name))
                    })
                    .collect();
                (directory, destinations?)
            };
            let (directory, destinations) = chosen;
            for destination in &destinations {
                if destination.parent() != Some(directory.as_path()) {
                    return Err(
                        "A remote item name is incompatible with the local filesystem.".into(),
                    );
                }
                check_local_destination(destination)?;
            }
            for (index, (source, destination)) in paths.iter().zip(destinations).enumerate() {
                let name = source.rsplit('/').next().unwrap_or(source);

                check_local_destination(&destination)?;
                // OpenSSH's SFTP transport glob-matches remote download paths.
                let mut literal = String::new();
                for ch in source.chars() {
                    if matches!(ch, '\\' | '*' | '?' | '[' | ']') {
                        literal.push('\\');
                    }
                    literal.push(ch);
                }
                let target = destination
                    .file_name()
                    .ok_or("Invalid download destination.")?;
                let mut command = build_scp_command(host);
                command
                    .current_dir(&directory)
                    .arg("-r")
                    .arg("--")
                    .arg(remote_operand(host, &literal))
                    .arg(Path::new(".").join(target));
                let progress = Progress {
                    direction: "Downloading",
                    name: name.escape_debug().to_string(),
                    next: paths
                        .get(index + 1)
                        .and_then(|path| path.rsplit('/').next())
                        .map(|name| name.escape_debug().to_string()),
                    completed: index,
                    count: paths.len(),
                    bytes: 0,
                    total_bytes: remote_bytes(host, source, cancel),
                    speed: None,
                };
                transfer_with_progress(command, cancel, tx, ctx, progress, |_| {
                    local_bytes(&destination)
                })?;
            }
            Ok(format!("Downloaded {} item(s).", paths.len()))
        }
    }
}

fn check_local_destination(path: &Path) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid download destination.")?;
    if cfg!(windows) {
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.chars().count() == 4
                && stem.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9', '¹', '²', '³']);
        if reserved
            || name.ends_with([' ', '.'])
            || name.contains([':', '\\', '<', '>', '"', '|', '?', '*'])
            || name.chars().any(char::is_control)
        {
            return Err(format!(
                "Unsupported Windows file name: {}",
                name.escape_debug()
            ));
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(format!(
            "Destination already exists: {}",
            name.escape_debug()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn upload(
    host: &SshRemoteHost,
    directory: &str,
    paths: Vec<PathBuf>,
    tx: &Sender<Event>,
    ctx: &egui::Context,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if paths.is_empty() {
        return Ok("Upload cancelled.".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for path in &paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| model::valid_name(name))
            .ok_or("Unsupported local file name.")?;
        if !names.insert(name) {
            return Err(format!("Duplicate upload name: {name}"));
        }
        let target = shell_quote(&model::join(directory, name));
        backend::execute_cancellable(host, &format!("if [ -e {target} ] || [ -L {target} ]; then printf 'Destination already exists: %s\\n' {target} >&2; exit 1; fi"), 15, Some(cancel))?;
    }
    for (index, path) in paths.iter().enumerate() {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid upload name.")?;

        let mut command = build_scp_command(host);
        command
            .current_dir(path.parent().ok_or("Invalid upload path.")?)
            .arg("-r")
            .arg("--")
            .arg(Path::new(".").join(name))
            .arg(remote_operand(host, directory));
        let destination = model::join(directory, name);
        let progress = Progress {
            direction: "Uploading",
            name: name.escape_debug().to_string(),
            next: paths
                .get(index + 1)
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().escape_debug().to_string()),
            completed: index,
            count: paths.len(),
            bytes: 0,
            total_bytes: local_bytes(path),
            speed: None,
        };
        transfer_with_progress(command, cancel, tx, ctx, progress, |stop| {
            remote_bytes(host, &destination, stop)
        })?;
    }
    Ok(format!("Uploaded {} item(s).", paths.len()))
}

fn remote_operand(host: &SshRemoteHost, path: &str) -> String {
    let hostname = host
        .host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    if hostname.contains(':') {
        format!("{}@[{hostname}]:{path}", host.user.trim())
    } else {
        format!("{}@{hostname}:{path}", host.user.trim())
    }
}

fn local_bytes(path: &Path) -> Option<u64> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.is_file() {
        return Some(metadata.len());
    }
    if !metadata.is_dir() {
        return Some(0);
    }
    let mut pending = vec![path.to_owned()];
    let mut bytes = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).ok()?.flatten() {
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Some(bytes)
}

fn remote_bytes(host: &SshRemoteHost, path: &str, stop: &AtomicBool) -> Option<u64> {
    let path = shell_quote(path);
    let command = format!(
        r#"if [ -f {path} ]; then stat -c %s -- {path}; elif [ -d {path} ]; then find {path} -type f -printf '%s\n' | awk '{{sum += $1}} END {{printf "%.0f", sum}}'; fi"#
    );
    let output = ssh_probe_output_cancellable(
        crate::remote::build_ssh_command(host, &command),
        Duration::from_secs(5),
        Some(stop),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn transfer_with_progress(
    command: std::process::Command,
    cancel: &AtomicBool,
    tx: &Sender<Event>,
    ctx: &egui::Context,
    mut progress: Progress,
    measure: impl Fn(&AtomicBool) -> Option<u64> + Send,
) -> Result<(), String> {
    let report = |progress: &Progress| {
        let _ = tx.send(Event::Progress(progress.clone()));
        ctx.request_repaint();
    };
    report(&progress);
    let stop = AtomicBool::new(false);
    let (done, wait) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let stop = &stop;
        scope.spawn(move || {
            let mut last_sample = Instant::now();
            while matches!(
                wait.recv_timeout(Duration::from_secs(1)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                if cancel.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(bytes) = measure(stop) {
                    let elapsed = last_sample.elapsed().as_secs_f64().max(0.001);
                    progress.speed =
                        Some((bytes.saturating_sub(progress.bytes) as f64 / elapsed) as u64);
                    progress.bytes = bytes;
                    last_sample = Instant::now();
                    report(&progress);
                }
            }
        });
        let result = transfer(command, cancel);
        stop.store(true, Ordering::Relaxed);
        let _ = done.send(());
        result
    })
}

fn transfer(command: std::process::Command, cancel: &AtomicBool) -> Result<(), String> {
    let output = ssh_probe_output_cancellable(command, Duration::from_secs(3600), Some(cancel))
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&output.stderr);
    Err(if error.trim().is_empty() {
        format!("SCP exited with {}", output.status)
    } else {
        error.into_owned()
    })
}
