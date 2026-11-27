use super::model::{InitSystem, ServiceInventoryReport};
use super::super::credentials::DesktopProtocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // The approval UI and executor are the next implementation stage.
pub(crate) enum Intent {
    Start,
    Stop,
    Restart,
    InstallAndStart,
    Uninstall,
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
    pub effect: &'static str,
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
    if protocol == DesktopProtocol::Vnc && intent == Intent::InstallAndStart {
        if report.package_manager.as_deref() != Some("apt") {
            return Err("Automated TigerVNC installation requires a detected APT package manager.");
        }
        return Ok(ActionPlan {
            summary: "Install TigerVNC",
            steps: vec![
                ActionStep {
                    id: "package-index",
                    title: "Update package index",
                    command: "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 update",
                    effect: "Refreshes package metadata; uses noninteractive mode and waits up to 60 seconds for APT locks.",
                    needs_root: true,
                    undo_note: "No automatic undo is needed.",
                },
                ActionStep {
                    id: "install",
                    title: "Install TigerVNC",
                    command: "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 install -y tigervnc-standalone-server",
                    effect: "Installs tigervnc-standalone-server without prompts; does not set a VNC password or start a session.",
                    needs_root: true,
                    undo_note: "Package removal is not part of automatic undo.",
                },
            ],
            warnings: vec![
                "After installation, set a VNC password with vncpasswd over SSH before starting TigerVNC.",
            ],
        });
    }
    if protocol == DesktopProtocol::Vnc && intent == Intent::Uninstall {
        if report.package_manager.as_deref() != Some("apt") {
            return Err("Automated TigerVNC uninstall requires a detected APT package manager.");
        }
        return Ok(ActionPlan {
            summary: "Uninstall TigerVNC",
            steps: vec![ActionStep {
                id: "uninstall",
                title: "Uninstall TigerVNC",
                command: "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 remove -y tigervnc-standalone-server",
                effect: "Removes the tigervnc-standalone-server package.",
                needs_root: true,
                undo_note: "Reinstall with apt-get install tigervnc-standalone-server.",
            }],
            warnings: vec!["Removing the package does not delete user ~/.vnc configuration files."],
        });
    }
    if protocol != DesktopProtocol::Rdp {
        return Err("TigerVNC start/stop/restart/config actions remain manual until per-user service mapping and password setup are verified.");
    }
    if matches!(intent, Intent::InstallAndStart | Intent::Uninstall)
        && report.package_manager.as_deref() != Some("apt")
    {
        return Err("Automated xrdp package actions require a detected APT package manager.");
    }

    let (summary, commands): (
        &'static str,
        &[(&'static str, &'static str, &'static str, &'static str)],
    ) = match intent {
        Intent::Start => (
            "Start xrdp",
            &[(
                "start",
                "Start xrdp",
                "sudo systemctl start xrdp.service",
                "Starts xrdp now; boot behavior is unchanged.",
            )],
        ),
        Intent::Stop => (
            "Stop xrdp",
            &[(
                "stop",
                "Stop xrdp",
                "sudo systemctl stop xrdp.service",
                "Stops xrdp now; it remains enabled or disabled as before.",
            )],
        ),
        Intent::Restart => (
            "Restart xrdp",
            &[(
                "restart",
                "Restart xrdp",
                "sudo systemctl restart xrdp.service",
                "Restarts xrdp now; active remote desktop sessions may disconnect.",
            )],
        ),
        Intent::EnableAtBoot => (
            "Enable xrdp at boot",
            &[(
                "enable",
                "Enable xrdp at boot",
                "sudo systemctl enable xrdp.service",
                "Enables xrdp for future boots; does not start it now.",
            )],
        ),
        Intent::Uninstall => (
            "Uninstall xrdp",
            &[
                (
                    "stop",
                    "Stop xrdp",
                    "sudo systemctl stop xrdp.service",
                    "Stops the running xrdp service before removal.",
                ),
                (
                    "uninstall",
                    "Uninstall xrdp",
                    "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 remove -y xrdp",
                    "Removes the xrdp package.",
                ),
            ],
        ),
        Intent::InstallAndStart => (
            "Install and start xrdp",
            &[
                (
                    "package-index",
                    "Update package index",
                    "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 update",
                    "Refreshes package metadata; uses noninteractive mode and waits up to 60 seconds for APT locks.",
                ),
                (
                    "install",
                    "Install xrdp",
                    "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 install -y xrdp",
                    "Installs xrdp and dependencies without prompts; waits up to 60 seconds for APT locks. Automatic undo does not remove packages.",
                ),
                (
                    "start",
                    "Start xrdp",
                    "sudo systemctl start xrdp.service",
                    "Starts xrdp now; boot behavior is unchanged.",
                ),
                (
                    "enable",
                    "Enable xrdp at boot",
                    "sudo systemctl enable xrdp.service",
                    "Enables xrdp for future boots; this step does not start it.",
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
            .map(|(id, title, command, effect)| ActionStep {
                id,
                title,
                command,
                effect,
                needs_root: true,
                undo_note: match *id {
                    "start" | "restart" => "The service can be stopped again.",
                    "enable" => "The service can be disabled again.",
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
        effect: "Restores the prior active/enabled service state; package changes are not reversed.",
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
        assert_eq!(plan.steps.len(), 4);
        assert_eq!(
            plan.steps[0].command,
            "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 update"
        );
        assert_eq!(
            plan.steps[1].command,
            "sudo env DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=60 install -y xrdp"
        );
        assert_eq!(plan.steps[2].command, "sudo systemctl start xrdp.service");
        assert_eq!(plan.steps[3].command, "sudo systemctl enable xrdp.service");
        assert!(plan.steps[0].effect.contains("package metadata"));
        assert!(plan.steps[1].effect.contains("does not remove packages"));
        assert!(plan.steps[2].effect.contains("Starts xrdp now"));
        assert!(plan.steps[3].effect.contains("future boots"));
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
            assert!(!plan.steps[0].effect.is_empty());
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
