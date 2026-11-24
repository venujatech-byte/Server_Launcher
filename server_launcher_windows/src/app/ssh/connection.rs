use super::*;

impl LauncherApp {
    pub(in crate::app) fn ensure_remote_connected(&self, host: &SshRemoteHost) {
        let is_testing = {
            let connecting = self.remote_connecting.lock().unwrap();
            connecting.contains(&host.id)
        };
        let has_status = {
            let status = self.remote_connection_status.lock().unwrap();
            status.contains_key(&host.id)
        };

        if !is_testing && !has_status {
            self.test_remote_host(host);
        }
    }

    pub(in crate::app) fn test_remote_host(&self, host: &SshRemoteHost) {
        {
            let mut connecting = self.remote_connecting.lock().unwrap();
            if !connecting.insert(host.id.clone()) {
                return;
            }
        }
        let host_clone = host.clone();
        let status_arc = self.remote_connection_status.clone();
        let connecting_arc = self.remote_connecting.clone();
        let timeout = self.ssh_probe_timeouts.connection_test_seconds;

        std::thread::spawn(move || {
            let res = test_ssh_connection(&host_clone, timeout);
            let mut connecting = connecting_arc.lock().unwrap();
            if connecting.remove(&host_clone.id) {
                status_arc.lock().unwrap().insert(host_clone.id, res);
            }
        });
    }

    pub(in crate::app) fn trigger_remote_scan(&self, host: &SshRemoteHost) {
        self.spawn_remote_scan(host, 0, None);
    }

    pub(in crate::app) fn schedule_remote_scan(&self, host: &SshRemoteHost, delay_ms: u64, ctx: egui::Context) {
        self.spawn_remote_scan(host, delay_ms, Some(ctx));
    }

    fn spawn_remote_scan(&self, host: &SshRemoteHost, delay_ms: u64, ctx: Option<egui::Context>) {
        if !self.remote_scanning.lock().unwrap().insert(host.id.clone()) {
            return;
        }

        let host = host.clone();
        let listeners = self.remote_listeners.clone();
        let stopped = self.remote_stopped_servers.clone();
        let scanning = self.remote_scanning.clone();
        let timeout = self.ssh_probe_timeouts.port_scan_seconds;
        std::thread::spawn(move || {
            if delay_ms != 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            if !scanning.lock().unwrap().contains(&host.id) {
                return;
            }
            let result = fetch_remote_listeners(&host, timeout);
            let mut scanning = scanning.lock().unwrap();
            if scanning.remove(&host.id) {
                let mut snapshots = listeners.lock().unwrap();
                match result {
                    Ok(rows) => {
                        if let Some(entries) = stopped.lock().unwrap().get_mut(&host.id) {
                            entries.retain(|entry| rows.binary_search_by_key(&entry.port, |row| row.port).is_err());
                        }
                        snapshots.insert(host.id, RemoteListenerSnapshot { rows: Arc::new(rows), error: None });
                    }
                    Err(error) => { snapshots.entry(host.id).or_default().error = Some(error.into()); }
                }
            }
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    pub(in crate::app) fn delete_ssh_host(&mut self, id: &str) {
        let Some(position) = self.ssh_hosts.iter().position(|host| host.id == id) else { return };
        self.ssh_hosts.remove(position);
        self.machine_sidebar_tabs.remove(id);
        self.cancel_remote_commands(id);
        self.remote_scanning.lock().unwrap().remove(id);
        self.remote_connecting.lock().unwrap().remove(id);
        self.remote_listeners.lock().unwrap().remove(id);
        self.remote_connection_status.lock().unwrap().remove(id);
        self.remote_stopped_servers.lock().unwrap().remove(id);
        self.remote_terminal_logs.lock().unwrap().remove(id);
        self.remote_terminal_inputs.remove(id);
        self.remote_search_query.remove(id);
        self.remote_sudo_passwords.remove(id);
        if let Some(mut terminal) = self.remote_terminals.remove(id) {
            // Use the owned child handle, since the cached PID may have exited.
            terminal.running_pid = None;
            terminal.interrupt();
        }
        if self.selected_machine == id {
            self.selected_machine = "local".into();
        }
        self.persist_config();
    }
}
