use super::*;

mod backend;
mod model;
mod transfers;
mod ui;

use backend::{Event, Job, Outcome, Worker};
use model::{Entry, Kind, Listing};
use std::collections::BTreeSet;
use transfers::Transfer;

#[derive(Clone)]
enum Prompt {
    Create(bool),
    Rename(String),
    Delete(Vec<String>),
    Compress(Vec<String>),
    Extract(String),
    InstallMicro(String),
}

#[derive(Clone, Default)]
struct RemoteFiles {
    listing: Option<Arc<Listing>>,
    worker: Option<Worker>,
    transfer: bool,
    changing: bool,
    refreshing_after_change: bool,
    progress: Option<transfers::Progress>,
    notice: Option<(String, bool)>,
    history: Vec<String>,
    history_index: usize,
    history_target: Option<usize>,
    selected: BTreeSet<String>,
    anchor: Option<String>,
    clipboard: Option<(bool, Vec<String>)>,
    filter: String,
    hidden: bool,
    sort: usize,
    descending: bool,
    address: Option<(String, bool)>,
    prompt: Option<Prompt>,
    name: String,
    format: usize,
}

impl RemoteFiles {
    fn directory(&self) -> Option<&str> {
        self.listing.as_ref().map(|listing| listing.path.as_str())
    }

    fn start(&mut self, host: &SshRemoteHost, ctx: &egui::Context, job: Job) {
        if self.worker.is_none() {
            self.notice = None;
            self.progress = None;
            self.transfer = matches!(job, Job::Transfer(_));
            self.changing = !matches!(job, Job::List(_) | Job::Micro(_, _));
            self.worker = Some(backend::start(host, job, ctx.clone()));
        }
    }

    fn poll(&mut self, host: &SshRemoteHost, ctx: &egui::Context) {
        let Some(worker) = &self.worker else { return };
        let result = {
            let receiver = worker.events.lock().unwrap();
            loop {
                match receiver.try_recv() {
                    Ok(Event::Progress(progress)) => self.progress = Some(progress),
                    Ok(Event::Finished(result)) => break result,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        break Err("File worker disconnected.".into())
                    }
                }
            }
        };
        self.worker = None;
        self.progress = None;
        self.transfer = false;
        let changing = std::mem::take(&mut self.changing);

        match result {
            Ok(Outcome::MicroMissing(path)) => self.prompt = Some(Prompt::InstallMicro(path)),
            Ok(Outcome::MicroOpened) => {
                self.notice = Some(("Opened Micro in an SSH terminal.".into(), false))
            }
            Ok(Outcome::Listing(listing)) => {
                if let Some(index) = self.history_target.take() {
                    self.history_index = index;
                } else if self.history.get(self.history_index) != Some(&listing.path) {
                    self.history.truncate(self.history_index + 1);
                    self.history.push(listing.path.clone());
                    self.history_index = self.history.len() - 1;
                }
                if self.directory() != Some(&listing.path) {
                    self.selected.clear();
                    self.anchor = None;
                    self.filter.clear();
                } else {
                    let names: BTreeSet<_> = listing
                        .entries
                        .iter()
                        .map(|entry| entry.name.as_str())
                        .collect();
                    self.selected.retain(|name| names.contains(name.as_str()));
                }
                self.listing = Some(Arc::new(listing));
            }
            Ok(Outcome::Changed { message, moved }) => {
                if let Some(path) = self.directory().map(str::to_owned) {
                    self.start(host, ctx, Job::List(path));
                    self.refreshing_after_change = true;
                }
                self.notice = Some((message, false));
                if moved {
                    self.clipboard = None;
                }
            }
            Err(error) => {
                self.history_target = None;
                if self.refreshing_after_change {
                    let previous = self.notice.take().map(|(text, _)| text).unwrap_or_default();
                    self.notice = Some((format!("{previous}\nCould not refresh: {error}"), true));
                } else {
                    if changing {
                        if let Some(path) = self.directory().map(str::to_owned) {
                            self.start(host, ctx, Job::List(path));
                            self.refreshing_after_change = true;
                        }
                    }
                    self.notice = Some((error, true));
                }
            }
        }
        if self.worker.is_none() {
            self.refreshing_after_change = false;
        }
    }
}

impl LauncherApp {
    pub(super) fn render_remote_files(&self, ui: &mut Ui, host: &SshRemoteHost) {
        let id = egui::Id::new(("remote_files_explorer", &host.id));
        let mut files = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<RemoteFiles>(id))
            .unwrap_or_default();
        files.poll(host, ui.ctx());
        if files.worker.is_none() && files.listing.is_none() && files.notice.is_none() {
            files.start(host, ui.ctx(), Job::List(".".into()));
        }
        ui::explorer(ui, id, host, &self.palette(), &mut files);
        ui.ctx().data_mut(|data| data.insert_temp(id, files));
    }
}
