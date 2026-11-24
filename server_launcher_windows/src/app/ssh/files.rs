use super::*;

#[derive(Clone)]
struct RemoteFiles {
    path: String,
    listing: Option<Arc<Mutex<Option<Result<String, String>>>>>,
}

impl LauncherApp {
    pub(super) fn render_remote_files(&self, ui: &mut Ui, host: &SshRemoteHost) {
        let id = egui::Id::new(("remote_files", &host.id));
        let mut files = ui.ctx().data(|data| data.get_temp::<RemoteFiles>(id))
            .unwrap_or_else(|| RemoteFiles { path: ".".into(), listing: None });
        let loading = files.listing.as_ref().is_some_and(|result| result.lock().unwrap().is_none());

        ui.heading("Remote files");
        ui.label("Enter a directory path to list its contents over SSH.");
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            let path = ui.add(egui::TextEdit::singleline(&mut files.path)
                .hint_text("Directory path")
                .desired_width(260.0));
            refresh = ui.add_enabled(!loading, egui::Button::new("List directory")).clicked()
                || (!loading && path.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        });

        if refresh || files.listing.is_none() {
            let path = if files.path.trim().is_empty() { "." } else { files.path.as_str() };
            // Pass the path as one shell argument and stop option parsing before it.
            let command = format!("ls -lah -- {}", crate::remote::shell_quote(path));
            let command = crate::remote::build_ssh_command(host, &command);
            let result = Arc::new(Mutex::new(None));
            files.listing = Some(Arc::clone(&result));
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let listing = crate::remote::ssh_probe_output(command, Duration::from_secs(15))
                    .map_err(|error| error.to_string())
                    .and_then(|output| {
                        if output.status.success() {
                            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
                        } else {
                            let error = String::from_utf8_lossy(&output.stderr);
                            Err(if error.trim().is_empty() { format!("SSH exited with {}", output.status) } else { error.into_owned() })
                        }
                    });
                *result.lock().unwrap() = Some(listing);
                ctx.request_repaint();
            });
        }

        ui.add_space(8.0);
        if let Some(result) = &files.listing {
            let result = result.lock().unwrap();
            match result.as_ref() {
                None => { ui.spinner(); }
                Some(Ok(listing)) => {
                    egui::ScrollArea::both().id_salt(id).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(listing).monospace()).extend());
                    });
                }
                Some(Err(error)) => { ui.label(format!("Could not list directory: {error}")); }
            }
        }
        ui.ctx().data_mut(|data| data.insert_temp(id, files));
    }
}
