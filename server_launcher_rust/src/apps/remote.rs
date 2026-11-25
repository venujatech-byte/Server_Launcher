use super::{actions::format_bytes, AppEntry, AppSource, AppsEvent, AppsManagerState};
use crate::config::SshRemoteHost;
use crate::remote::{build_ssh_command, ssh_probe_output};
use std::time::Duration;

impl AppsManagerState {
    pub fn refresh_remote(&mut self, host: &SshRemoteHost, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.status_text = "Reading SSH inventory…".into();
        let host = host.clone();
        let ctx = ctx.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = inventory(&host);
            let (apps, status) = match result {
                Ok(apps) => (apps, "Ready".to_owned()),
                Err(error) => (Vec::new(), format!("SSH inventory failed: {error}")),
            };
            let _ = tx.send(AppsEvent::DiscoveryBatch {
                apps,
                repos: Vec::new(),
                done: true,
                status,
            });
            ctx.request_repaint();
        });
        self.apps.clear();
        self.filtered_indices.clear();
        self.selected_index = None;
        self.dirty_filter = true;
    }
}

fn inventory(host: &SshRemoteHost) -> Result<Vec<AppEntry>, String> {
    let script = r#"
export LC_ALL=C
if command -v rpm >/dev/null 2>&1; then
    rpm -qa --qf 'rpm\t%{NAME}\t%{VERSION}-%{RELEASE}.%{ARCH}\t%{SIZE}\t%{INSTALLTIME}\n' || exit
elif command -v dpkg-query >/dev/null 2>&1; then
    dpkg-query -W -f='deb\t${binary:Package}\t${Version}\t${Installed-Size}\t${db:Status-Status}\n' || exit
elif command -v pacman >/dev/null 2>&1; then
    pacman -Q | awk '{printf "pacman\t%s\t%s\t\t\n", $1, $2}'
else
    printf '%s\n' 'No supported package database (RPM, dpkg, or pacman).' >&2
    exit 1
fi
if command -v flatpak >/dev/null 2>&1; then
    flatpak list --columns=application,version | sed 's/^/flatpak\t/'
fi
"#;
    let output = ssh_probe_output(build_ssh_command(host, script), Duration::from_secs(20))
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_entry)
        .collect())
}

fn parse_entry(line: &str) -> Option<AppEntry> {
    let mut fields = line.split('\t');
    let source = match fields.next()? {
        "rpm" => AppSource::DnfRpm,
        "deb" => AppSource::AptDpkg,
        "pacman" => AppSource::Pacman,
        "flatpak" => AppSource::Flatpak,
        _ => return None,
    };
    let id = fields.next()?.trim();
    if id.is_empty() {
        return None;
    }
    let version = fields.next().unwrap_or("").trim();
    let size = fields.next().and_then(|size| size.parse::<u64>().ok());
    let metadata = fields.next();
    if source == AppSource::AptDpkg && metadata != Some("installed") {
        return None;
    }
    let size_bytes = if source == AppSource::AptDpkg {
        size.map(|size| size.saturating_mul(1024))
    } else {
        size
    };
    let installed_timestamp = if source == AppSource::DnfRpm {
        metadata.and_then(|value| value.parse::<i64>().ok())
    } else {
        None
    };
    Some(AppEntry {
        id: id.into(),
        name: id.into(),
        source,
        version: version.into(),
        size_bytes,
        size_display: size_bytes.map(format_bytes).unwrap_or_else(|| "—".into()),
        publisher_or_origin: format!("{} · SSH host", source.label()),
        description: String::new(),
        categories: String::new(),
        install_location: None,
        data_paths: Vec::new(),
        exec_cmd: None,
        uninstall_cmd: None,
        desktop_file: None,
        installed_date: installed_timestamp
            .and_then(|value| chrono::DateTime::from_timestamp(value, 0))
            .map(|date| date.format("%Y-%m-%d").to_string()),
        installed_timestamp,
        update_available: None,
        user_installed: false,
        can_launch: false,
        can_update: false,
        can_uninstall: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_debs_and_converts_kibibytes() {
        let entry = parse_entry("deb\thtop\t3.4\t42\tinstalled").unwrap();
        assert_eq!(entry.size_bytes, Some(42 * 1024));
        assert!(parse_entry("deb\thtop\t3.4\t42\tconfig-files").is_none());
        assert!(!entry.can_uninstall);
    }
}
