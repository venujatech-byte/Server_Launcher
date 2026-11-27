use super::super::credentials::DesktopProtocol;
use std::net::IpAddr;

const MAX_REPORT_BYTES: usize = 1024 * 1024;
const MAX_FIELD_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InitSystem {
    Systemd,
    Other,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceState {
    NotInstalled,
    InstalledStopped,
    RunningSystemd,
    RunningManual,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    NotInstalled,
    InstalledStopped,
    Failed,
    RunningSystemd,
    RunningManual,
    RunningNotReachable,
    NotReachable,
    HandshakeSucceededUnconfirmed,
    Ready,
    LimitedDetection,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Exposure {
    LocalhostOnly,
    AllInterfaces,
    SpecificAddress(String),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listener {
    pub address: String,
    pub port: u16,
    pub process: Option<String>,
    pub pid: Option<u32>,
    pub owner: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VncDisplay {
    pub display: String,
    pub port: u16,
    pub user: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ServiceInventory {
    pub installed: Option<bool>,
    pub version: Option<String>,
    pub unit_present: Option<bool>,
    pub unit_active: Option<bool>,
    pub unit_enabled: Option<bool>,
    pub unit_failed: Option<bool>,
    pub configured_port: Option<u16>,
    pub manual_process: Option<bool>,
    pub listeners: Vec<Listener>,
    pub displays: Vec<VncDisplay>,
}

impl ServiceInventory {
    pub(crate) fn state(&self) -> ServiceState {
        if self.unit_failed == Some(true) {
            ServiceState::Failed
        } else if self.unit_active == Some(true) {
            ServiceState::RunningSystemd
        } else if self.manual_process == Some(true) {
            ServiceState::RunningManual
        } else if !self.listeners.is_empty() {
            ServiceState::Unknown
        } else {
            match self.installed {
                Some(false) => ServiceState::NotInstalled,
                Some(true) => ServiceState::InstalledStopped,
                None => ServiceState::Unknown,
            }
        }
    }

    pub(crate) fn exposure(&self) -> Exposure {
        let mut specific = None;
        let mut unknown = false;
        for listener in &self.listeners {
            let address = listener.address.trim_matches(['[', ']']);
            if address == "unknown" {
                unknown = true;
                continue;
            }
            if address == "*"
                || address
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_unspecified())
            {
                return Exposure::AllInterfaces;
            }
            if address == "localhost"
                || address
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
            {
                continue;
            }
            specific = Some(listener.address.clone());
        }
        if let Some(address) = specific {
            if unknown {
                Exposure::Unknown
            } else {
                Exposure::SpecificAddress(address)
            }
        } else if unknown {
            Exposure::Unknown
        } else if !self.listeners.is_empty() {
            Exposure::LocalhostOnly
        } else {
            Exposure::Unknown
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ServiceInventoryReport {
    pub os_id: Option<String>,
    pub os_like: Option<String>,
    pub os_version: Option<String>,
    pub os_pretty_name: Option<String>,
    pub init_system: Option<InitSystem>,
    pub user: Option<String>,
    pub uid: Option<u32>,
    pub sudo_available: Option<bool>,
    pub passwordless_sudo: Option<bool>,
    pub package_manager: Option<String>,
    pub desktop_sessions: Vec<String>,
    pub firewall_hints: Vec<String>,
    pub available_memory_mb: Option<u64>,
    pub free_disk_mb: Option<u64>,
    pub rdp: ServiceInventory,
    pub vnc: ServiceInventory,
    pub rdp_handshake: Option<Result<String, String>>,
    pub vnc_handshake: Option<Result<String, String>>,
    pub raw_output: String,
}

impl ServiceInventoryReport {
    pub(crate) fn service(&self, protocol: DesktopProtocol) -> &ServiceInventory {
        match protocol {
            DesktopProtocol::Rdp => &self.rdp,
            DesktopProtocol::Vnc => &self.vnc,
        }
    }

    pub(crate) fn handshake(&self, protocol: DesktopProtocol) -> Option<&Result<String, String>> {
        match protocol {
            DesktopProtocol::Rdp => self.rdp_handshake.as_ref(),
            DesktopProtocol::Vnc => self.vnc_handshake.as_ref(),
        }
    }

    pub(crate) fn readiness(&self, protocol: DesktopProtocol) -> Readiness {
        let service = self.service(protocol);
        let state = service.state();
        let running = matches!(state, ServiceState::RunningSystemd | ServiceState::RunningManual);
        match self.handshake(protocol) {
            Some(Ok(_)) if running => Readiness::Ready,
            Some(Ok(_)) => Readiness::HandshakeSucceededUnconfirmed,
            Some(Err(_)) if running => Readiness::RunningNotReachable,
            Some(Err(_)) => Readiness::NotReachable,
            None => match state {
                ServiceState::NotInstalled => Readiness::NotInstalled,
                ServiceState::InstalledStopped => Readiness::InstalledStopped,
                ServiceState::Failed => Readiness::Failed,
                ServiceState::RunningSystemd => Readiness::RunningSystemd,
                ServiceState::RunningManual => Readiness::RunningManual,
                ServiceState::Unknown
                    if self.os_id.is_none()
                        || !matches!(self.init_system, Some(InitSystem::Systemd)) =>
                {
                    Readiness::LimitedDetection
                }
                ServiceState::Unknown => Readiness::Unknown,
            },
        }
    }
}

/// Parse the bounded SLD/1 TSV protocol emitted by the read-only SSH inventory script.
/// Records are `SLD/1<TAB>field<TAB>scope<TAB>key<TAB>percent-encoded-value`; listener
/// records are `SLD/1<TAB>listener<TAB>scope<TAB>address<TAB>port<TAB>process<TAB>pid<TAB>owner`.
pub(crate) fn parse_inventory(input: &str) -> Result<ServiceInventoryReport, String> {
    if input.len() > MAX_REPORT_BYTES {
        return Err("The remote service inventory exceeded the 1 MiB limit.".into());
    }

    let mut report = ServiceInventoryReport {
        raw_output: input.to_owned(),
        ..Default::default()
    };
    let mut seen = std::collections::HashSet::new();
    let mut in_records = false;
    let mut ended = false;

    for line in input.lines() {
        if line.len() > MAX_FIELD_BYTES * 2 {
            return Err("The remote service inventory contains an oversized record.".into());
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() == 2 && fields[0] == "SLD/1" && fields[1] == "begin" {
            if in_records || ended {
                return Err("The remote service inventory has invalid record boundaries.".into());
            }
            in_records = true;
            continue;
        }
        if fields.len() == 2 && fields[0] == "SLD/1" && fields[1] == "end" {
            if !in_records || ended {
                return Err("The remote service inventory has invalid record boundaries.".into());
            }
            in_records = false;
            ended = true;
            continue;
        }
        if !in_records {
            continue;
        }
        if fields.first() != Some(&"SLD/1") {
            if fields.first().is_some_and(|field| field.starts_with("SLD/")) {
                return Err("The remote service inventory uses an unsupported format version.".into());
            }
            continue;
        }
        match fields.get(1).copied() {
            Some("field") if fields.len() == 5 => {
                let scope = fields[2];
                let key = fields[3];
                let value = decode_field(fields[4])?;
                if value.len() > MAX_FIELD_BYTES {
                    return Err("The remote service inventory contains an oversized value.".into());
                }
                let repeated = (scope == "host"
                    && matches!(key, "desktop_session" | "firewall_hint"))
                    || (scope == "vnc" && key == "display");
                let unique_key = format!("{scope}.{key}");
                if !repeated && !seen.insert(unique_key.clone()) {
                    return Err(format!("The remote service inventory repeats {scope}.{key}."));
                }
                set_field(&mut report, scope, key, &value)?;
            }
            Some("listener") if fields.len() == 8 => {
                let listener = Listener {
                    address: decode_field(fields[3])?,
                    port: parse_number(fields[4], "listener port")?,
                    process: optional_field(fields[5])?,
                    pid: optional_number(fields[6], "listener PID")?,
                    owner: optional_field(fields[7])?,
                };
                if listener.port == 0 || listener.address.is_empty() {
                    return Err("The remote service inventory contains an invalid listener.".into());
                }
                service_mut(&mut report, fields[2])?
                    .listeners
                    .push(listener);
            }
            Some("display") if fields.len() == 6 && fields[2] == "vnc" => {
                let display = decode_field(fields[3])?;
                if !display
                    .strip_prefix(':')
                    .is_some_and(|number| number.parse::<u16>().is_ok())
                {
                    return Err("The remote service inventory contains an invalid VNC display.".into());
                }
                let port = parse_number(fields[4], "VNC display port")?;
                if port == 0 {
                    return Err("The remote service inventory contains an invalid VNC display port.".into());
                }
                report.vnc.displays.push(VncDisplay {
                    display,
                    port,
                    user: optional_field(fields[5])?,
                });
            }
            Some("field") | Some("listener") | Some("display") => {
                return Err("The remote service inventory contains a malformed SLD/1 record.".into())
            }
            _ => {}
        }
    }

    if !ended || in_records {
        return Err("The remote service inventory is incomplete.".into());
    }

    Ok(report)
}

fn set_field(
    report: &mut ServiceInventoryReport,
    scope: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if scope == "host" {
        match key {
            "os_id" => report.os_id = nonempty(value),
            "os_like" => report.os_like = nonempty(value),
            "os_version" => report.os_version = nonempty(value),
            "os_pretty_name" => report.os_pretty_name = nonempty(value),
            "init" => {
                report.init_system = Some(match value {
                    "systemd" => InitSystem::Systemd,
                    "other" => InitSystem::Other,
                    "unknown" => InitSystem::Unknown,
                    _ => return Err("The remote service inventory has an invalid init value.".into()),
                })
            }
            "user" => report.user = nonempty(value),
            "uid" => report.uid = Some(parse_number(value, "uid")?),
            "sudo_available" => report.sudo_available = Some(parse_bool(value)?),
            "passwordless_sudo" => report.passwordless_sudo = Some(parse_bool(value)?),
            "package_manager" => report.package_manager = nonempty(value),
            "desktop_session" => {
                if !value.is_empty() {
                    report.desktop_sessions.push(value.to_owned());
                }
            }
            "firewall_hint" => {
                if !value.is_empty() {
                    report.firewall_hints.push(value.to_owned());
                }
            }
            "available_memory_mb" => {
                report.available_memory_mb = Some(parse_number(value, "available memory")?)
            }
            "free_disk_mb" => report.free_disk_mb = Some(parse_number(value, "free disk")?),
            _ => {}
        }
        return Ok(());
    }

    let service = service_mut(report, scope)?;
    match key {
        "installed" => service.installed = Some(parse_bool(value)?),
        "version" => service.version = nonempty(value),
        "unit_present" => service.unit_present = Some(parse_bool(value)?),
        "unit_active" => service.unit_active = Some(parse_bool(value)?),
        "unit_enabled" => service.unit_enabled = Some(parse_bool(value)?),
        "unit_failed" => service.unit_failed = Some(parse_bool(value)?),
        "configured_port" => {
            let port = parse_number(value, "configured port")?;
            if port == 0 {
                return Err("The remote service inventory has an invalid configured port.".into());
            }
            service.configured_port = Some(port);
        }
        "manual_process" => service.manual_process = Some(parse_bool(value)?),
        _ => {}
    }
    Ok(())
}

fn service_mut<'a>(
    report: &'a mut ServiceInventoryReport,
    scope: &str,
) -> Result<&'a mut ServiceInventory, String> {
    match scope {
        "rdp" => Ok(&mut report.rdp),
        "vnc" => Ok(&mut report.vnc),
        _ => Err("The remote service inventory uses an invalid protocol scope.".into()),
    }
}

fn decode_field(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| "The remote service inventory has invalid escaping.".to_owned())?;
            let hex = std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "The remote service inventory has invalid escaping.".to_owned())?;
            decoded.push(hex);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| "The remote service inventory contains invalid UTF-8.".into())
}

fn optional_field(value: &str) -> Result<Option<String>, String> {
    let value = decode_field(value)?;
    Ok(nonempty(&value))
}

fn optional_number(value: &str, name: &str) -> Result<Option<u32>, String> {
    if value.is_empty() || value == "-" {
        Ok(None)
    } else {
        parse_number(value, name).map(Some)
    }
}

fn parse_number<T>(value: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("The remote service inventory has an invalid {name}."))
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err("The remote service inventory has an invalid boolean value.".into()),
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty() && value != "-").then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "banner from ssh\nSLD/1\tbegin\nSLD/1\tfield\thost\tos_id\tubuntu\nSLD/1\tfield\thost\tinit\tsystemd\nSLD/1\tfield\thost\tuser\troot\nSLD/1\tfield\trdp\tinstalled\t1\nSLD/1\tfield\trdp\tunit_active\t1\nSLD/1\tlistener\trdp\t0.0.0.0\t3389\t-\t-\t-\nSLD/1\tfield\tvnc\tinstalled\t1\nSLD/1\tfield\tvnc\tmanual_process\t1\nSLD/1\tlistener\tvnc\t0.0.0.0\t5901\tXtigervnc\t26146\tubuntu\nSLD/1\tdisplay\tvnc\t:1\t5901\tubuntu\nSLD/1\tend\n";

    #[test]
    fn parses_rdp_without_process_and_manual_vnc_on_display_one_port() {
        let report = parse_inventory(SAMPLE).unwrap();
        assert_eq!(report.os_id.as_deref(), Some("ubuntu"));
        assert_eq!(report.init_system, Some(InitSystem::Systemd));
        assert_eq!(report.rdp.listeners[0].port, 3389);
        assert_eq!(report.rdp.listeners[0].process, None);
        assert_eq!(report.vnc.listeners[0].port, 5901);
        assert_eq!(report.vnc.listeners[0].process.as_deref(), Some("Xtigervnc"));
        assert_eq!(report.vnc.manual_process, Some(true));
        assert_eq!(report.vnc.displays[0].display, ":1");
        assert_eq!(report.vnc.displays[0].port, 5901);
    }

    #[test]
    fn ignores_noise_and_unknown_fields_but_rejects_invalid_known_data() {
        let report = parse_inventory("motd\nSLD/1\tbegin\nSLD/1\tfield\thost\tnew_key\tvalue\nSLD/1\tend\n").unwrap();
        assert_eq!(report.os_id, None);
        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tfield\trdp\tinstalled\tmaybe\nSLD/1\tend\n").is_err());
        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tlistener\trdp\t*\t0\t-\t-\t-\nSLD/1\tend\n").is_err());
        assert!(parse_inventory("SLD/1\tbegin\nSLD/2\tfield\thost\tos_id\tubuntu\nSLD/1\tend\n").is_err());
    }

    #[test]
    fn percent_decodes_listener_owner_and_rejects_bad_escapes() {
        let report = parse_inventory("SLD/1\tbegin\nSLD/1\tlistener\tvnc\t127.0.0.1\t5901\tXtigervnc\t1\tuser%20name\nSLD/1\tend\n").unwrap();
        assert_eq!(report.vnc.listeners[0].owner.as_deref(), Some("user name"));
        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tfield\thost\tuser\tbad%XZ\nSLD/1\tend\n").is_err());
    }

    #[test]
    fn rejects_oversized_report_and_duplicate_singleton() {
        assert!(parse_inventory(&"x".repeat(MAX_REPORT_BYTES + 1)).is_err());
        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tfield\thost\tos_id\tubuntu\nSLD/1\tfield\thost\tos_id\tfedora\nSLD/1\tend\n").is_err());
    }

    #[test]
    fn derives_readiness_and_exposure_without_confusing_process_with_handshake() {
        let report = parse_inventory(SAMPLE).unwrap();
        assert_eq!(report.rdp.state(), ServiceState::RunningSystemd);
        assert_eq!(report.rdp.exposure(), Exposure::AllInterfaces);
        assert_eq!(report.vnc.state(), ServiceState::RunningManual);
        assert_eq!(report.vnc.exposure(), Exposure::AllInterfaces);
        assert_eq!(ServiceInventory::default().state(), ServiceState::Unknown);
        assert_eq!(ServiceInventory::default().exposure(), Exposure::Unknown);
        let ambiguous = ServiceInventory {
            installed: Some(true),
            unit_active: Some(false),
            listeners: report.rdp.listeners.clone(),
            ..Default::default()
        };
        assert_eq!(ambiguous.state(), ServiceState::Unknown);
    }

    #[test]
    fn derives_each_service_readiness_from_scan_and_handshake_evidence() {
        use super::Readiness;

        let mut report = ServiceInventoryReport {
            os_id: Some("ubuntu".into()),
            init_system: Some(InitSystem::Systemd),
            ..Default::default()
        };
        report.rdp.installed = Some(false);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::NotInstalled);

        report.rdp.installed = Some(true);
        report.rdp.unit_present = Some(true);
        report.rdp.unit_active = Some(false);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::InstalledStopped);

        report.rdp.unit_failed = Some(true);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::Failed);

        report.rdp.unit_failed = Some(false);
        report.rdp.unit_active = Some(true);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::RunningSystemd);

        report.rdp.unit_active = Some(false);
        report.rdp.manual_process = Some(true);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::RunningManual);

        report.rdp_handshake = Some(Err("Connection timed out".into()));
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::RunningNotReachable);
        report.rdp_handshake = Some(Ok("X.224 negotiation succeeded".into()));
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::Ready);

        report.rdp.manual_process = None;
        report.rdp.installed = None;
        report.rdp.unit_present = None;
        report.rdp_handshake = Some(Ok("X.224 negotiation succeeded".into()));
        assert_eq!(
            report.readiness(DesktopProtocol::Rdp),
            Readiness::HandshakeSucceededUnconfirmed
        );

        report.rdp_handshake = Some(Err("Connection refused".into()));
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::NotReachable);

        report.rdp_handshake = None;
        report.init_system = Some(InitSystem::Unknown);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::LimitedDetection);

        report.init_system = Some(InitSystem::Systemd);
        assert_eq!(report.readiness(DesktopProtocol::Rdp), Readiness::Unknown);
        assert_eq!(
            ServiceInventoryReport::default().readiness(DesktopProtocol::Rdp),
            Readiness::LimitedDetection
        );
    }

    #[test]
    fn permits_multiple_desktop_sessions_and_classifies_loopback() {
        let report = parse_inventory("SLD/1\tbegin\nSLD/1\tfield\thost\tdesktop_session\txfce\nSLD/1\tfield\thost\tdesktop_session\tgnome\nSLD/1\tlistener\trdp\t127.0.0.1\t3389\t-\t-\t-\nSLD/1\tend\n").unwrap();
        assert_eq!(report.desktop_sessions, ["xfce", "gnome"]);
        assert_eq!(report.rdp.exposure(), Exposure::LocalhostOnly);
        let unknown_listener = ServiceInventory {
            listeners: vec![Listener {
                address: "unknown".into(),
                port: 5901,
                process: None,
                pid: None,
                owner: None,
            }],
            ..Default::default()
        };
        assert_eq!(unknown_listener.exposure(), Exposure::Unknown);
    }

    #[test]
    fn parses_bind_variants_multiple_displays_and_rejects_truncated_records() {
        let report = parse_inventory("SLD/1\tbegin\nSLD/1\tfield\thost\tinit\tother\nSLD/1\tlistener\trdp\t[::]\t3389\t-\t-\t-\nSLD/1\tlistener\tvnc\t*\t5901\t-\t-\t-\nSLD/1\tlistener\tvnc\t192.0.2.10\t5902\tXtigervnc\t7\tuser\nSLD/1\tdisplay\tvnc\t:1\t5901\tuser\nSLD/1\tdisplay\tvnc\t:2\t5902\tuser\nSLD/1\tend\n").unwrap();
        assert_eq!(report.init_system, Some(InitSystem::Other));
        assert_eq!(report.rdp.exposure(), Exposure::AllInterfaces);
        assert_eq!(report.vnc.exposure(), Exposure::AllInterfaces);
        assert_eq!(report.vnc.listeners[1].address, "192.0.2.10");
        assert_eq!(report.vnc.displays.iter().map(|display| display.port).collect::<Vec<_>>(), [5901, 5902]);

        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tlistener\tvnc\t127.0.0.1\t5901\t-").is_err());
        assert!(parse_inventory("SLD/1\tbegin\nSLD/1\tlistener\tvnc\t127.0.0.1").is_err());
    }
}
