use super::model::{parse_inventory, ServiceInventoryReport};
use std::time::Duration;

const DETECTION_SCRIPT: &str = include_str!("scripts/detect.sh");

pub(crate) fn scan(
    host: &crate::config::SshRemoteHost,
    timeout: Duration,
) -> Result<ServiceInventoryReport, String> {
    let remote_command = format!("sh -c {}", crate::remote::shell_quote(DETECTION_SCRIPT));
    let command = crate::remote::build_ssh_command(host, &remote_command);
    let output = crate::remote::ssh_probe_output(command, timeout)
        .map_err(|error| format!("SSH scan failed: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!("SSH inventory exited with status {:?}.", output.status.code())
        } else {
            format!("SSH inventory failed: {error}")
        });
    }
    let report = String::from_utf8_lossy(&output.stdout);
    parse_inventory(&report)
}

#[cfg(test)]
mod tests {
    use super::DETECTION_SCRIPT;

    #[test]
    fn listener_detection_keeps_read_only_fallbacks() {
        assert!(DETECTION_SCRIPT.contains("if command -v ss"));
        assert!(DETECTION_SCRIPT.contains("elif command -v netstat"));
        assert!(DETECTION_SCRIPT.contains("/proc/net/tcp /proc/net/tcp6"));
    }
}
