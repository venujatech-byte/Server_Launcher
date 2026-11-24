use super::*;

mod backend;
mod model;
mod transfers;

#[derive(Clone)]
struct RemoteFiles {
    path: String,
    worker: Option<backend::Worker>,
    listing: Option<Result<model::Listing, String>>,
}

impl LauncherApp {
    pub(super) fn render_remote_files(&self, ui: &mut Ui, host: &SshRemoteHost) {
        let id = egui::Id::new(("remote_files", &host.id));
        let mut files = ui.ctx().data(|data| data.get_temp::<RemoteFiles>(id))
            .unwrap_or_else(|| RemoteFiles { path: ".".into(), worker: None, listing: None });

        let result = files.worker.as_ref().and_then(|worker| match worker.events.lock().unwrap().try_recv() {
            Ok(backend::Event::Finished(result)) => Some(result),
            Ok(backend::Event::Progress(_, _)) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("File operation stopped unexpectedly.".into())),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            files.worker = None;
            files.listing = Some(result.and_then(|result| match result {
                backend::Outcome::Listing(listing) => Ok(listing),
                backend::Outcome::Changed(message) => Err(message),
            }));
        }

        ui.heading("Remote files");
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            let path = ui.add(egui::TextEdit::singleline(&mut files.path).hint_text("Directory path").desired_width(260.0));
            refresh = ui.add_enabled(files.worker.is_none(), egui::Button::new("List directory")).clicked()
                || (files.worker.is_none() && path.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        });
        if refresh || (files.worker.is_none() && files.listing.is_none()) {
            files.worker = Some(backend::start(host, backend::Job::List(files.path.clone()), ui.ctx().clone()));
        }
        if let Some(worker) = &files.worker {
            ui.spinner();
            if ui.button("Cancel").clicked() {
                worker.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        match &files.listing {
            Some(Ok(listing)) => {
                ui.label(&listing.path);
                egui::ScrollArea::both().id_salt(id).show(ui, |ui| {
                    for entry in &listing.entries {
                        ui.monospace(format!("{}  {}  {}  {}  {}", entry.modified, entry.mode, entry.size, entry.kind.label(), model::join(&listing.path, &entry.name).escape_debug()));
                    }
                });
            }
            Some(Err(error)) => { ui.label(format!("Could not list directory: {error}")); }
            None => {}
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, files));
    }
}
