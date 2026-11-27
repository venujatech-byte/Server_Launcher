use super::super::credentials::DesktopProtocol;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ManualInstructions {
    pub install: Vec<String>,
    pub start: Vec<String>,
    pub verify: Vec<String>,
    pub note: &'static str,
}

pub(crate) fn instructions(
    os_id: Option<&str>,
    protocol: DesktopProtocol,
    port: u16,
) -> Option<ManualInstructions> {
    if !matches!(os_id, Some("ubuntu" | "debian")) || port == 0 {
        return None;
    }

    Some(match protocol {
        DesktopProtocol::Rdp => ManualInstructions {
            install: vec![
                "sudo apt-get update".into(),
                "sudo apt-get install xrdp".into(),
            ],
            start: vec!["sudo systemctl start xrdp.service".into()],
            verify: vec![
                "systemctl status xrdp.service --no-pager".into(),
                "ss -ltn | grep ':3389'".into(),
            ],
            note: "A graphical desktop session is required. xrdp uses its configured port (normally 3389); review your installed xrdp.ini before changing a custom port. The app will not change firewall or cloud security rules.",
        },
        DesktopProtocol::Vnc => ManualInstructions {
            install: vec![
                "sudo apt-get update".into(),
                "sudo apt-get install tigervnc-standalone-server".into(),
            ],
            start: vec![
                "vncpasswd".into(),
                format!("vncserver :1 -rfbport {port} -localhost yes -geometry 1920x1080 -depth 24"),
            ],
            verify: vec![
                "vncserver -list".into(),
                format!("ss -ltn | grep ':{port}'"),
            ],
            note: "Run as the intended desktop user, not root. Classic VNC passwords use at most eight bytes. The SSH tunnel is recommended; this server is bound to localhost. A desktop startup script may be needed for a usable session.",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gives_verified_debian_family_commands_and_preserves_selected_vnc_port() {
        let vnc = instructions(Some("ubuntu"), DesktopProtocol::Vnc, 5901).unwrap();
        assert!(vnc.install.iter().any(|command| command.contains("tigervnc-standalone-server")));
        assert!(vnc.start.iter().any(|command| command.contains("-rfbport 5901")));
        assert!(vnc.start.iter().any(|command| command.contains("-localhost yes")));

        let rdp = instructions(Some("debian"), DesktopProtocol::Rdp, 3389).unwrap();
        assert!(rdp.start.iter().any(|command| command.contains("xrdp.service")));
    }

    #[test]
    fn leaves_unknown_distributions_manual_only() {
        assert!(instructions(Some("fedora"), DesktopProtocol::Rdp, 3389).is_none());
        assert!(instructions(None, DesktopProtocol::Vnc, 5901).is_none());
        assert!(instructions(Some("ubuntu"), DesktopProtocol::Vnc, 0).is_none());
    }
}
