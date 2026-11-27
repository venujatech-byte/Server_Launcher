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
    RestoreState,
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
    if intent == Intent::InstallAndStart && report.package_manager.as_deref() != Some("apt") {
        return Err("Automated xrdp installation requires a detected APT package manager.");
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
        Intent::RestoreState => return Err("Use the reviewed restore-state planner."),
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

pub(crate) fn build_restore_plan(
    prior_active: Option<bool>,
    prior_enabled: Option<bool>,
    current_active: Option<bool>,
    current_enabled: Option<bool>,
) -> Result<ActionPlan, &'static str> {
    let (Some(prior_active), Some(prior_enabled), Some(current_active), Some(current_enabled)) =
        (prior_active, prior_enabled, current_active, current_enabled)
    else {
        return Err("The previous or current xrdp service state is unknown; Undo is unavailable.");
    };

    let mut steps = Vec::new();
    if current_active != prior_active {
        let (id, title, command) = if prior_active {
            ("restore-start", "Restore running state", "sudo systemctl start xrdp.service")
        } else {
            ("restore-stop", "Restore stopped state", "sudo systemctl stop xrdp.service")
        };
        steps.push(restore_step(id, title, command));
    }
    if current_enabled != prior_enabled {
        let (id, title, command) = if prior_enabled {
            ("restore-enable", "Restore enabled-at-boot state", "sudo systemctl enable xrdp.service")
        } else {
            ("restore-disable", "Restore disabled-at-boot state", "sudo systemctl disable xrdp.service")
        };
        steps.push(restore_step(id, title, command));
    }
    if steps.is_empty() {
        return Err("xrdp already matches the state recorded before the last action.");
    }
    Ok(ActionPlan {
        summary: "Restore the xrdp state observed before the last action",
        steps,
        warnings: Vec::new(),
    })
}

fn restore_step(id: &'static str, title: &'static str, command: &'static str) -> ActionStep {
    ActionStep {
        id,
        title,
        command,
        needs_root: true,
        undo_note: "Restores the observed state from before the app action.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_ubuntu() -> ServiceInventoryReport {
        ServiceInventoryReport {
            os_id: Some("ubuntu".into()),
            init_system: Some(InitSystem::Systemd),
            package_manager: Some("apt".into()),
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
    fn install_plan_requires_detected_apt_capability() {
        let mut report = verified_ubuntu();
        report.package_manager = Some("unknown".into());
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::InstallAndStart).is_err());

        report.package_manager = None;
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::InstallAndStart).is_err());

        report.package_manager = Some("apt".into());
        assert!(build_plan(&report, DesktopProtocol::Rdp, Intent::InstallAndStart).is_ok());
    }

    #[test]
    fn builds_only_the_fixed_command_for_each_supported_service_intent() {
        for (intent, summary, command) in [
            (Intent::Start, "Start xrdp", "sudo systemctl start xrdp.service"),
            (Intent::Stop, "Stop xrdp", "sudo systemctl stop xrdp.service"),
            (
                Intent::Restart,
                "Restart xrdp",
                "sudo systemctl restart xrdp.service",
            ),
            (
                Intent::EnableAtBoot,
                "Enable xrdp at boot",
                "sudo systemctl enable xrdp.service",
            ),
        ] {
            let plan = build_plan(&verified_ubuntu(), DesktopProtocol::Rdp, intent).unwrap();
            assert_eq!(plan.summary, summary);
            assert_eq!(plan.steps.len(), 1);
            assert_eq!(plan.steps[0].command, command);
            assert!(plan.steps[0].needs_root);
        }
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

    #[test]
    fn restore_plan_only_contains_fixed_changes_needed_for_known_prior_state() {
        let plan = build_restore_plan(Some(false), Some(true), Some(true), Some(false)).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].command, "sudo systemctl stop xrdp.service");
        assert_eq!(plan.steps[1].command, "sudo systemctl enable xrdp.service");
        assert!(build_restore_plan(None, Some(true), Some(true), Some(false)).is_err());
        assert!(build_restore_plan(Some(true), Some(true), Some(true), Some(true)).is_err());
    }
}
