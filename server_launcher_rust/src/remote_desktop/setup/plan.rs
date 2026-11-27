use super::model::{InitSystem, ServiceInventoryReport};
use super::super::credentials::DesktopProtocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // The approval UI and executor are the next implementation stage.
pub(crate) enum Intent {
    Start,
    Stop,
    Restart,
    InstallAndStart,
    EnableAtBoot,
    RestrictToLocalhost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Plans are not shown until the approval dialog is wired.
pub(crate) struct ActionStep {
    pub id: &'static str,
    pub title: &'static str,
    pub command: &'static str,
    pub needs_root: bool,
    pub undo_note: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Plans are not shown until the approval dialog is wired.
pub(crate) struct ActionPlan {
    pub summary: &'static str,
    pub steps: Vec<ActionStep>,
    pub warnings: Vec<&'static str>,
}

#[allow(dead_code)] // No remote action may run before its approval flow exists.
pub(crate) fn build_plan(
    report: &ServiceInventoryReport,
    protocol: DesktopProtocol,
    intent: Intent,
) -> Result<ActionPlan, &'static str> {
    if !matches!(report.os_id.as_deref(), Some("ubuntu" | "debian")) {
        return Err("This distribution is not verified for automated service actions.");
    }
    if report.init_system != Some(InitSystem::Systemd) {
        return Err("Automated service actions require a detected systemd host.");
    }
    if protocol != DesktopProtocol::Rdp {
        return Err("TigerVNC actions remain manual until the per-user service mapping is verified.");
    }

    let (summary, commands): (
        &'static str,
        &[(&'static str, &'static str, &'static str)],
    ) = match intent {
        Intent::Start => (
            "Start xrdp",
            &[("start", "Start xrdp", "sudo systemctl start xrdp.service")],
        ),
        Intent::Stop => (
            "Stop xrdp",
            &[("stop", "Stop xrdp", "sudo systemctl stop xrdp.service")],
        ),
        Intent::Restart => (
            "Restart xrdp",
            &[("restart", "Restart xrdp", "sudo systemctl restart xrdp.service")],
        ),
        Intent::EnableAtBoot => (
            "Enable xrdp at boot",
            &[("enable", "Enable xrdp at boot", "sudo systemctl enable xrdp.service")],
        ),
        Intent::InstallAndStart => (
            "Install and start xrdp",
            &[
                ("package-index", "Update package index", "sudo apt-get update"),
                ("install", "Install xrdp", "sudo apt-get install -y xrdp"),
                (
                    "enable-start",
                    "Enable and start xrdp",
                    "sudo systemctl enable --now xrdp.service",
                ),
            ],
        ),
        Intent::RestrictToLocalhost => {
            return Err("xrdp listen-address configuration is not verified; use the manual instructions.")
        }
    };

    if intent != Intent::InstallAndStart && report.rdp.installed != Some(true) {
        return Err("xrdp is not confirmed installed; use the install plan or manual instructions.");
    }
    if intent != Intent::InstallAndStart && report.rdp.unit_present != Some(true) {
        return Err("The xrdp systemd units are not confirmed installed; use manual instructions.");
    }

    Ok(ActionPlan {
        summary,
        steps: commands
            .iter()
            .map(|(id, title, command)| ActionStep {
                id,
                title,
                command,
                needs_root: true,
                undo_note: match *id {
                    "start" | "restart" => "The service can be stopped again.",
                    "enable" | "enable-start" => "The service can be disabled again.",
                    "install" => "Package removal is not part of automatic undo.",
                    _ => "No automatic undo is needed.",
                },
            })
            .collect(),
        warnings: [
            (report.uid == Some(0))
                .then_some("SSH is connected as root; commands do not need sudo in that session."),
            report
                .desktop_sessions
                .is_empty()
                .then_some("No desktop session was detected; a successful RDP login may show a blank screen."),
        ]
        .into_iter()
        .flatten()
        .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_ubuntu() -> ServiceInventoryReport {
        ServiceInventoryReport {
            os_id: Some("ubuntu".into()),
            init_system: Some(InitSystem::Systemd),
            rdp: super::super::model::ServiceInventory {
                installed: Some(true),
                unit_present: Some(true),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn creates_fixed_reviewable_xrdp_steps() {
        let plan = build_plan(
            &verified_ubuntu(),
            DesktopProtocol::Rdp,
            Intent::InstallAndStart,
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].command, "sudo apt-get update");
        assert_eq!(plan.steps[1].command, "sudo apt-get install -y xrdp");
        assert_eq!(plan.steps[2].command, "sudo systemctl enable --now xrdp.service");
        assert!(plan.steps.iter().all(|step| step.needs_root));
    }

    #[test]
    fn refuses_unverified_hosts_and_unverified_mutations() {
        let mut report = verified_ubuntu();
        report.os_id = Some("fedora".into());
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::Start).is_err());

        let mut report = verified_ubuntu();
        report.rdp.installed = Some(false);
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::Start).is_err());
        let mut report = verified_ubuntu();
        report.rdp.unit_present = Some(false);
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::Start).is_err());
        assert!(build_plan(&report, DesktopProtocol::Vnc, Intent::Start).is_err());
        assert!(build_plan(
            &verified_ubuntu(),
            DesktopProtocol::Rdp,
            Intent::RestrictToLocalhost
        )
        .is_err());
    }
}
