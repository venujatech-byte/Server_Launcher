use super::{model, transfers, SshRemoteHost};
use crate::remote::{build_ssh_command, shell_quote, ssh_probe_output};
use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

#[derive(Debug)]
pub(super) enum Job {
    List(String),
    Micro(String, bool),
    RunPython(String),
    Create(String, bool),
    Rename(String, String),
    Paste(Vec<String>, String, bool),
    Delete(Vec<String>),
    Compress(Vec<String>, String),
    Extract(String, String, bool),
    Transfer(transfers::Transfer),
}

#[derive(Debug)]
pub(super) enum Outcome {
    Listing(model::Listing),
    MicroMissing(String),
    TerminalOpened(&'static str),
    Changed { message: String, moved: bool },
}

pub(super) enum Event {
    Progress(transfers::Progress),
    Finished(Result<Outcome, String>),
}

#[derive(Clone)]
pub(super) struct Worker {
    pub events: Arc<Mutex<mpsc::Receiver<Event>>>,
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
}

pub(super) fn start(host: &SshRemoteHost, job: Job, ctx: egui::Context) -> Worker {
    let host = host.clone();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        let outcome = match job {
            Job::Transfer(request) => transfers::run(&host, request, &tx, &ctx, &worker_cancel)
                .map(|message| Outcome::Changed {
                    message,
                    moved: false,
                }),
            job => run(&host, job),
        };
        let _ = tx.send(Event::Finished(outcome));
        ctx.request_repaint();
    });
    Worker {
        events: Arc::new(Mutex::new(rx)),
        cancel,
    }
}

fn run(host: &SshRemoteHost, job: Job) -> Result<Outcome, String> {
    if let Job::List(path) = job {
        let path = if path.starts_with('/') {
            path
        } else {
            format!("./{path}")
        };
        let command = format!(
            r#"cd -P {} || exit; printf '%s\0' "$PWD"; if ! command -v find >/dev/null 2>&1; then echo 'Install findutils on this SSH host to browse files.' >&2; exit 127; fi; LC_ALL=C find . -mindepth 1 -maxdepth 1 -printf '%y\0%TY-%Tm-%Td %TH:%TM\0%m\0%s\0%f\0'"#,
            shell_quote(&path)
        );
        let output = execute(host, &command, 15)?;
        return model::parse_listing(&output).map(Outcome::Listing);
    }
    if let Job::Micro(path, install) = job {
        return open_micro(host, path, install);
    }
    if let Job::RunPython(path) = job {
        execute(host, "command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1 || { echo 'Python is not installed on this SSH host.' >&2; exit 127; }", 15)?;
        let (directory, _) = path.rsplit_once('/').ok_or("Invalid Python file path.")?;
        let directory = shell_quote(if directory.is_empty() { "/" } else { directory });
        let file = shell_quote(&path);
        let command = format!("cd {directory} && if command -v python3 >/dev/null 2>&1; then exec python3 {file}; else exec python {file}; fi");
        let mut host = host.clone();
        host.remote_cwd.clear();
        crate::remote::launch_external_ssh_command(&host, &command)?;
        return Ok(Outcome::TerminalOpened(
            "Started Python in an SSH terminal.",
        ));
    }
    let moved = matches!(&job, Job::Paste(_, _, true));
    let command = mutation_command(job)?;
    execute(host, &command, 300).map(|_| Outcome::Changed {
        message: if moved {
            "Moved selected items.".into()
        } else {
            "Operation complete.".into()
        },
        moved,
    })
}

fn open_micro(host: &SshRemoteHost, path: String, install: bool) -> Result<Outcome, String> {
    let mut command = String::new();
    if install {
        command.push_str(
            r#"
install_micro() {
    if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi
}
if ! command -v micro >/dev/null 2>&1; then
    if command -v apt-get >/dev/null 2>&1; then
        install_micro apt-get install micro || exit
    elif command -v dnf >/dev/null 2>&1; then
        install_micro dnf install micro || exit
    elif command -v pacman >/dev/null 2>&1; then
        install_micro pacman -S micro || exit
    elif command -v zypper >/dev/null 2>&1; then
        install_micro zypper install micro || exit
    elif command -v apk >/dev/null 2>&1; then
        install_micro apk add micro || exit
    else
        printf '%s\n' 'Install Micro manually on this SSH host, then try again.' >&2
        exit 127
    fi
fi
"#,
        );
    } else {
        let installed = execute(
            host,
            "if command -v micro >/dev/null 2>&1; then printf yes; else printf no; fi",
            15,
        )?;
        if installed != b"yes" {
            return Ok(Outcome::MicroMissing(path));
        }
    }
    command.push_str(&format!("exec micro {}", shell_quote(&path)));
    let mut host = host.clone();
    host.remote_cwd.clear();
    crate::remote::launch_external_ssh_command(&host, &command)?;
    Ok(Outcome::TerminalOpened("Opened Micro in an SSH terminal."))
}

pub(super) fn execute(
    host: &SshRemoteHost,
    command: &str,
    timeout: u64,
) -> Result<Vec<u8>, String> {
    let output = ssh_probe_output(
        build_ssh_command(host, command),
        Duration::from_secs(timeout),
    )
    .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(if error.trim().is_empty() {
            format!("SSH exited with {}", output.status)
        } else {
            error.into_owned()
        })
    }
}

fn absent(path: &str) -> String {
    let path = shell_quote(path);
    format!("if [ -e {path} ] || [ -L {path} ]; then printf 'Destination already exists: %s\\n' {path} >&2; exit 1; fi")
}

fn move_without_overwrite(from: &str, to: &str) -> String {
    let from = shell_quote(from);
    let to = shell_quote(to);
    format!("mv -nT -- {from} {to} && if [ -e {from} ] || [ -L {from} ]; then printf 'Destination already exists: %s\\n' {to} >&2; exit 1; fi")
}

fn require(tool: &str) -> String {
    format!("if ! command -v {tool} >/dev/null 2>&1; then echo 'Install {tool} on the SSH host to use this operation.' >&2; exit 127; fi")
}

fn operands(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| shell_quote(path))
        .collect::<Vec<_>>()
        .join(" ")
}

fn mutation_command(job: Job) -> Result<String, String> {
    Ok(match job {
        Job::Create(path, folder) => {
            let quoted = shell_quote(&path);
            if folder {
                format!("mkdir -- {quoted}")
            } else {
                format!("(set -C; : > {quoted})")
            }
        }
        Job::Rename(from, to) => format!("{}; {}", absent(&to), move_without_overwrite(&from, &to)),
        Job::Paste(paths, directory, cut) => {
            let mut checks = Vec::new();
            let mut actions = Vec::new();
            let mut names = std::collections::BTreeSet::new();
            for path in paths {
                let name = path
                    .rsplit('/')
                    .next()
                    .filter(|name| model::valid_name(name))
                    .ok_or("Invalid source name.")?;
                if !names.insert(name.to_owned()) {
                    return Err("Two selected items have the same name.".into());
                }
                let target = model::join(&directory, name);
                if target.starts_with(&format!("{path}/")) {
                    return Err("Cannot paste a folder into itself.".into());
                }
                checks.push(absent(&target));
                actions.push(if cut {
                    move_without_overwrite(&path, &target)
                } else {
                    format!("cp -anT -- {} {}", shell_quote(&path), shell_quote(&target))
                });
            }
            format!("{}; {}", checks.join("; "), actions.join(" && "))
        }
        Job::Delete(paths) => {
            if paths.is_empty()
                || paths
                    .iter()
                    .any(|path| !path.starts_with('/') || path == "/")
            {
                return Err("Select valid entries to delete.".into());
            }
            format!("rm -rf -- {}", operands(&paths))
        }
        Job::Compress(paths, target) => {
            let directory = target
                .rsplit_once('/')
                .ok_or("Invalid archive destination.")?
                .0;
            let names: Vec<_> = paths
                .iter()
                .map(|path| format!("./{}", path.rsplit('/').next().unwrap_or_default()))
                .collect();
            let names = operands(&names);
            let quoted = shell_quote(&target);
            let (tool, args) = match model::archive_extension(&target) {
                Some(".zip") => ("zip", format!("-r -y {quoted} {names}")),
                Some(".tar") => ("tar", format!("-cf {quoted} -- {names}")),
                Some(".tar.gz") => ("tar", format!("-czf {quoted} -- {names}")),
                Some(".rar") => ("rar", format!("a -ol {quoted} {names}")),
                _ => return Err("Choose a .zip, .tar, .tar.gz, or .rar name.".into()),
            };
            format!(
                "{}; {}; cd {} && {tool} {args}",
                require(tool),
                absent(&target),
                shell_quote(if directory.is_empty() { "/" } else { directory })
            )
        }
        Job::Extract(archive, directory, create) => {
            let source = shell_quote(&archive);
            let (tool, args) = match model::archive_extension(&archive) {
                Some(".zip") => ("unzip", format!("-n {source}")),
                Some(".rar") => ("unrar", format!("x -o- {source}")),
                Some(_) => ("tar", format!("--extract --keep-old-files --no-same-owner --no-same-permissions --file={source}")),
                None => return Err("Unsupported archive extension.".into()),
            };
            // Extract in an isolated staging folder, then publish only if no top-level names collide.
            let directory = shell_quote(&directory);
            let enter = if create {
                format!("mkdir -- {directory} && cd {directory}")
            } else {
                format!("cd {directory}")
            };
            format!(
                r#"{}; {enter} || exit; temp=$(mktemp -d .explorer-extract.XXXXXXXX) || exit
trap 'rm -rf -- "$temp"' EXIT
(cd "$temp" && {tool} {args}) || exit
for path in "$temp"/* "$temp"/.[!.]* "$temp"/..?*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    name=${{path##*/}}
    if [ -e "./$name" ] || [ -L "./$name" ]; then printf 'Destination already exists: %s\n' "$name" >&2; exit 1; fi
done
for path in "$temp"/* "$temp"/.[!.]* "$temp"/..?*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    name=${{path##*/}}
    mv -nT -- "$path" "./$name" || exit
    if [ -e "$path" ] || [ -L "$path" ]; then printf 'Destination already exists: %s\n' "$name" >&2; exit 1; fi
done"#,
                require(tool)
            )
        }
        Job::List(_) | Job::Micro(_, _) | Job::RunPython(_) | Job::Transfer(_) => unreachable!(),
    })
}
