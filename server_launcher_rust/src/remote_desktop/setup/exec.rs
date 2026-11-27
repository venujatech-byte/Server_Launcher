use super::plan::ActionPlan;
use crate::config::SshRemoteHost;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

/// Run only the fixed commands in a plan after the caller has obtained approval.
pub(crate) fn run_approved_plan(
    host: &SshRemoteHost,
    plan: &ActionPlan,
    password: Option<Zeroizing<String>>,
) -> Result<Vec<String>, String> {
    let mut output = Vec::with_capacity(plan.steps.len());
    for step in &plan.steps {
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
        output.push(String::from_utf8_lossy(&result.stdout).into_owned());
    }
    Ok(output)
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
            format!("sudo -S -p '' -- {quoted}")
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
        assert_eq!(command_for_step(&step, false, true).unwrap(), "sudo -S -p '' -- 'systemctl' 'start' 'xrdp.service'");
    }
}
