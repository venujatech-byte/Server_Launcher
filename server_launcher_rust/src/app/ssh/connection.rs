use super::*;

impl LauncherApp {
    pub(in crate::app) fn remote_host_lifetime(&self, host: &SshRemoteHost) -> Option<Arc<()>> {
        if !self.ssh_hosts.iter().any(|configured| configured == host) {
            return None;
        }
        let mut lifetimes = self.remote_host_lifetimes.lock().unwrap();
        Some(Arc::clone(lifetimes.entry(host.id.clone()).or_insert_with(|| Arc::new(()))))
    }

    fn clear_remote_host_caches(&self, id: &str) {
        self.remote_scanning.lock().unwrap().remove(id);
        self.remote_connecting.lock().unwrap().remove(id);
        self.remote_listeners.lock().unwrap().remove(id);
        self.remote_connection_status.lock().unwrap().remove(id);
        self.remote_stopped_servers.lock().unwrap().remove(id);
        self.remote_terminal_logs.lock().unwrap().remove(id);
        self.remote_command_activity.lock().unwrap().remove(id);
        self.remote_sensors_snapshots.lock().unwrap().remove(id);
        self.remote_sensors_in_flight.lock().unwrap().remove(id);
        self.remote_sensors_last_fetch.lock().unwrap().remove(id);
        self.remote_sensors_errors.lock().unwrap().remove(id);
        self.remote_bluetooth_snapshots.lock().unwrap().remove(id);
        self.remote_bluetooth_in_flight.lock().unwrap().remove(id);
        self.remote_bluetooth_last_fetch.lock().unwrap().remove(id);
        self.remote_services.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.remote_services_in_flight.lock().unwrap().retain(|key| !remote_host_cache_key_matches(id, key));
        self.remote_services_errors.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.remote_journal_logs.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.systemd_actions_in_flight.lock().unwrap().retain(|key| !remote_host_cache_key_matches(id, key));
        self.systemd_files_cache.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.systemd_files_in_flight.lock().unwrap().retain(|key| !remote_host_cache_key_matches(id, key));
        self.systemd_file_content_cache.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.systemd_deps_cache.lock().unwrap().retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.systemd_deps_in_flight.lock().unwrap().retain(|key| !remote_host_cache_key_matches(id, key));
    }

    pub(in crate::app) fn invalidate_remote_host(&mut self, id: &str) {
        self.remote_host_lifetimes.lock().unwrap().remove(id);
        self.clear_remote_host_caches(id);
        self.remote_services_user.remove(id);
        self.service_details_cache.retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.service_query_errors.retain(|key, _| !remote_host_cache_key_matches(id, key));
        self.service_enabled_cache.retain(|key, _| !remote_host_cache_key_matches(id, key));
        if self.systemd_editor_context.as_ref().is_some_and(|(host_id, _)| host_id == id) {
            self.systemd_editor_context = None;
            self.systemd_selected_file = None;
            self.systemd_file_content.clear();
            self.systemd_file_original.clear();
            self.systemd_file_status_msg = None;
            self.selected_service = None;
        }
    }

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
        let Some(lifetime) = self.remote_host_lifetime(host) else { return };
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
        let lifetimes_arc = self.remote_host_lifetimes.clone();

        std::thread::spawn(move || {
            let res = test_ssh_connection(&host_clone, timeout);
            let lifetimes = lifetimes_arc.lock().unwrap();
            if !remote_host_lifetime_matches(&lifetimes, &host_clone.id, &lifetime) { return; }
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
        let Some(lifetime) = self.remote_host_lifetime(host) else { return };
        let mut request = (
            Instant::now() + std::time::Duration::from_millis(delay_ms),
            self.ssh_probe_timeouts.port_scan_seconds,
        );
        {
            let mut scans = self.remote_scanning.lock().unwrap();
            if let Some(pending) = scans.get_mut(&host.id) {
                // Keep the latest requested settling time without starting another worker.
                if pending.is_none_or(|(due, _)| due <= request.0) {
                    *pending = Some(request);
                }
                return;
            }
            scans.insert(host.id.clone(), None);
        }

        let host = host.clone();
        let listeners = self.remote_listeners.clone();
        let stopped = self.remote_stopped_servers.clone();
        let scanning = self.remote_scanning.clone();
        let lifetimes = self.remote_host_lifetimes.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(request.0.saturating_duration_since(Instant::now()));
            let current = lifetimes.lock().unwrap();
            if !remote_host_lifetime_matches(&current, &host.id, &lifetime) { break; }
            drop(current);
            if !scanning.lock().unwrap().contains_key(&host.id) {
                break;
            }
            let result = fetch_remote_listeners(&host, request.1);
            let current = lifetimes.lock().unwrap();
            if !remote_host_lifetime_matches(&current, &host.id, &lifetime) { break; }
            let mut scans = scanning.lock().unwrap();
            let Some(pending) = scans.get_mut(&host.id) else { break };
            let next = pending.take();
            {
                let mut snapshots = listeners.lock().unwrap();
                match result {
                    Ok(rows) => {
                        if let Some(entries) = stopped.lock().unwrap().get_mut(&host.id) {
                            entries.retain(|entry| rows.binary_search_by_key(&entry.port, |row| row.port).is_err());
                        }
                        snapshots.insert(host.id.clone(), RemoteListenerSnapshot { rows: Arc::new(rows), error: None });
                    }
                    Err(error) => { snapshots.entry(host.id.clone()).or_default().error = Some(error.into()); }
                }
            }
            if next.is_none() {
                scans.remove(&host.id);
            }
            drop(scans);
            drop(current);
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
            let Some(next) = next else { break };
            request = next;
        });
    }

    pub(in crate::app) fn delete_ssh_host(&mut self, id: &str) {
        let Some(position) = self.ssh_hosts.iter().position(|host| host.id == id) else { return };
        self.ssh_hosts.remove(position);
        self.machine_sidebar_tabs.remove(id);
        self.cancel_remote_commands(id);
        self.invalidate_remote_host(id);
        self.remote_terminal_inputs.remove(id);
        self.remote_search_query.remove(id);
        self.remote_sudo_passwords.remove(id);
        if self.remote_start_server_modal.host_id == id {
            self.remote_start_server_modal.open = false;
        }
        if self.remote_reboot_modal.host.as_ref().is_some_and(|host| host.id == id) {
            self.remote_reboot_modal.close();
        }
        if self.remote_shutdown_modal.host.as_ref().is_some_and(|host| host.id == id) {
            self.remote_shutdown_modal.close();
        }
        if self.sudo_popup.host_id.as_deref() == Some(id) {
            self.sudo_popup.open = false;
            self.sudo_popup.pending = None;
            self.sudo_popup.password_input.clear();
        }
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
