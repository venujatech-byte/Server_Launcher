use super::plan::ActionPlan;
use crate::config::SshRemoteHost;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::{Zeroize, Zeroizing};

pub(crate) enum ActionEvent {
    StepStarted { index: usize, total: usize, title: &'static str },
    StepFinished { index: usize, total: usize, title: &'static str, output: String },
    Finished(Result<usize, String>),
}

/// Run only the fixed commands in a plan after the caller has obtained approval.
pub(crate) fn run_approved_plan(
    host: &SshRemoteHost,
    plan: &ActionPlan,
    password: Option<Zeroizing<String>>,
    cancel: &AtomicBool,
    mut report: impl FnMut(ActionEvent),
) -> Result<usize, String> {
    let password = if host.user != "root" {
        match password {
            Some(password) if !sudo_is_passwordless(host)? => {
                validate_sudo_password(host, &password)?;
                Some(password)
            }
            _ => None,
        }
    } else {
        None
    };
    let total = plan.steps.len();
    let mut completed = 0;
    for (index, step) in plan.steps.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(format!("Cancelled after {completed} completed step(s)."));
        }
        report(ActionEvent::StepStarted { index: index + 1, total, title: step.title });
        let remote_command = command_for_step(step, host.user == "root", password.is_some())?;
        let (program, _) = parse_fixed_command(step.command)?;
        let mut input = Zeroizing::new(Vec::new());
        if step.needs_root && host.user != "root" {
            if let Some(password) = password.as_ref() {
                input.extend_from_slice(password.as_bytes());
                input.push(b'\n');
            }
        }
        let command = crate::remote::build_ssh_command(host, &remote_command);
        let result = crate::remote::ssh_probe_output_with_stdin(
            command,
            Duration::from_secs(if program == "apt-get" { 300 } else { 45 }),
            &input,
        )
        .map_err(|error| format!("{} failed: {error}", step.title))?;
        input.zeroize();
        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            let stdout = String::from_utf8_lossy(&result.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(if detail.is_empty() {
                format!("{} failed (exit {:?}).", step.title, result.status.code())
            } else {
                format!("{} failed: {detail}", step.title)
            });
        }
        completed += 1;
        let output = String::from_utf8_lossy(&result.stdout);
        report(ActionEvent::StepFinished {
            index: index + 1,
            total,
            title: step.title,
            output: output_tail(&output, 8 * 1024),
        });
    }
    Ok(completed)
}

fn validate_sudo_password(
    host: &SshRemoteHost,
    password: &str,
) -> Result<(), String> {
    let mut input = Zeroizing::new(password.as_bytes().to_vec());
    input.push(b'\n');
    let command = crate::remote::build_ssh_command(host, "sudo -k -S -p '' -- true");
    let result = crate::remote::ssh_probe_output_with_stdin(command, Duration::from_secs(20), &input)
        .map_err(|error| format!("Could not validate sudo access: {error}"));
    input.zeroize();
    let result = result?;
    if result.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&result.stderr);
    if crate::systemd::needs_sudo_password(&detail) {
        return Err("Sudo password was rejected. Re-enter it and try again.".into());
    }
    Err(if detail.trim().is_empty() {
        "Sudo access could not be validated for this SSH user.".into()
    } else {
        format!("Sudo access could not be validated: {}", detail.trim())
    })
}

fn sudo_is_passwordless(host: &SshRemoteHost) -> Result<bool, String> {
    let command = crate::remote::build_ssh_command(host, "sudo -n -p '' -- true");
    crate::remote::ssh_probe_output(command, Duration::from_secs(20))
        .map(|result| result.status.success())
        .map_err(|error| format!("Could not check sudo access: {error}"))
}

fn output_tail(output: &str, max_chars: usize) -> String {
    let mut tail = output.chars().rev().take(max_chars).collect::<Vec<_>>();
    tail.reverse();
    tail.into_iter().collect()
}

pub(crate) fn command_for_step(
    step: &super::plan::ActionStep,
    ssh_as_root: bool,
    has_sudo_password: bool,
) -> Result<String, String> {
    let (program, args) = parse_fixed_command(step.command)?;
    let quoted = std::iter::once(program)
        .chain(args.iter().copied())
        .map(crate::remote::shell_quote)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(if step.needs_root && !ssh_as_root {
        if has_sudo_password {
            format!("sudo -k -S -p '' -- {quoted}")
        } else {
            format!("sudo -n -- {quoted}")
        }
    } else {
        quoted
    })
}

fn parse_fixed_command(command: &str) -> Result<(&str, Vec<&str>), String> {
    let command = command
        .strip_prefix("sudo ")
        .ok_or_else(|| "The approved plan contains an unexpected command.".to_string())?;
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "The approved plan contains an empty command.".to_string())?;
    let args = parts.collect::<Vec<_>>();
    let allowed = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-./_".contains(&byte))
    };
    if !allowed(program) || args.iter().any(|arg| !allowed(arg)) {
        return Err("The approved plan contains unsupported command syntax.".into());
    }
    Ok((program, args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_simple_fixed_plan_commands() {
        assert_eq!(
            parse_fixed_command("sudo systemctl enable --now xrdp.service").unwrap(),
            ("systemctl", vec!["enable", "--now", "xrdp.service"])
        );
        for hostile in [
            "sudo systemctl start xrdp; id",
            "sudo systemctl $(id)",
            "sudo sh -c `id`",
        ] {
            assert!(parse_fixed_command(hostile).is_err());
        }
    }

    #[test]
    fn reviewed_command_matches_the_sudo_mode_that_will_run() {
        let step = super::super::plan::ActionStep {
            id: "start",
            title: "Start xrdp",
            command: "sudo systemctl start xrdp.service",
            needs_root: true,
            undo_note: "Can be stopped again.",
        };
        assert_eq!(command_for_step(&step, true, false).unwrap(), "'systemctl' 'start' 'xrdp.service'");
        assert_eq!(command_for_step(&step, false, false).unwrap(), "sudo -n -- 'systemctl' 'start' 'xrdp.service'");
        assert_eq!(command_for_step(&step, false, true).unwrap(), "sudo -k -S -p '' -- 'systemctl' 'start' 'xrdp.service'");
    }

    #[test]
    fn action_output_keeps_only_a_bounded_unicode_safe_tail() {
        assert_eq!(output_tail("αβγδε", 3), "γδε");
        assert_eq!(output_tail("short", 20), "short");
    }

    #[test]
    fn recognizes_sudo_authentication_failures_for_retry() {
        assert!(crate::systemd::needs_sudo_password("sudo: 1 incorrect password attempt"));
        assert!(!crate::systemd::needs_sudo_password("user is not in the sudoers file"));
    }
}
