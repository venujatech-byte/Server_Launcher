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
            connecting.insert(host.id.clone());
        }
        let host_clone = host.clone();
        let status_arc = self.remote_connection_status.clone();
        let connecting_arc = self.remote_connecting.clone();

        std::thread::spawn(move || {
            let res = test_ssh_connection(&host_clone);
            if let Ok(mut status) = status_arc.lock() {
                status.insert(host_clone.id.clone(), res);
            }
            if let Ok(mut connecting) = connecting_arc.lock() {
                connecting.remove(&host_clone.id);
            }
        });
    }

    pub(in crate::app) fn trigger_remote_scan(&self, host: &SshRemoteHost) {
        let is_scanning = {
            let scanning = self.remote_scanning.lock().unwrap();
            scanning.contains(&host.id)
        };
        if is_scanning {
            return;
        }

        {
            let mut scanning = self.remote_scanning.lock().unwrap();
            scanning.insert(host.id.clone());
        }

        let host_clone = host.clone();
        let listeners_arc = self.remote_listeners.clone();
        let scanning_arc = self.remote_scanning.clone();

        std::thread::spawn(move || {
            let res = fetch_remote_listeners(&host_clone);
            if let Ok(mut map) = listeners_arc.lock() {
                // Ensure entry is inserted even on error to prevent runaway reconnect loop
                map.insert(host_clone.id.clone(), res.unwrap_or_default());
            }
            if let Ok(mut scanning) = scanning_arc.lock() {
                scanning.remove(&host_clone.id);
            }
        });
    }

    pub(in crate::app) fn schedule_remote_scan(&self, host: &SshRemoteHost, delay_ms: u64, ctx: egui::Context) {
        let host_clone = host.clone();
        let listeners_arc = self.remote_listeners.clone();
        let scanning_arc = self.remote_scanning.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if let Ok(listeners) = crate::remote::fetch_remote_listeners(&host_clone) {
                if let Ok(mut map) = listeners_arc.lock() {
                    map.insert(host_clone.id.clone(), listeners);
                }
            }
            if let Ok(mut scanning) = scanning_arc.lock() {
                scanning.remove(&host_clone.id);
            }
            ctx.request_repaint();
        });
    }
}
