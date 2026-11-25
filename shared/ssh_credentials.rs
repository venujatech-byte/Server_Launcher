use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::io::Write;

pub(crate) fn binding(host: &crate::config::SshRemoteHost) -> String {
    let mut digest = Sha256::new();
    for part in [&host.id, &host.host, &host.user] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    digest.update(host.port.to_le_bytes());
    format!("{:x}", digest.finalize())
}
use std::process::{Command, Stdio};

fn run(mut command: Command, input: Option<&str>) -> Result<String, String> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Credential store unavailable: {error}"))?;
    let written = if let Some(data) = input {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(data.as_bytes())
    } else {
        Ok(())
    };
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("The system credential store could not complete the request.".into());
    }
    written.map_err(|error| error.to_string())?;
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| "Invalid credential-store response.".into())
}

#[cfg(not(windows))]
const SERVICE: &str = "org.server_launcher.ssh";

#[cfg(not(windows))]
pub(crate) fn protect(password: &str, binding: &str) -> Result<String, String> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| "Could not generate a credential identifier.")?;
    let key = STANDARD.encode(nonce);
    let mut command = Command::new("secret-tool");
    command.args([
        "store",
        "--label=Server Launcher SSH",
        "application",
        SERVICE,
        "credential",
        &key,
        "target",
        binding,
    ]);
    run(command, Some(&STANDARD.encode(password)))?;
    Ok(key)
}

#[cfg(not(windows))]
pub(crate) fn load(reference: &str, binding: &str) -> Result<String, String> {
    let mut command = Command::new("secret-tool");
    command.args([
        "lookup",
        "application",
        SERVICE,
        "credential",
        reference,
        "target",
        binding,
    ]);
    decode(&run(command, None)?)
}

#[cfg(windows)]
fn powershell(body: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("powershell.exe");
    command
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-NonInteractive", "-Command", body]);
    command
}

#[cfg(windows)]
pub(crate) fn protect(password: &str, binding: &str) -> Result<String, String> {
    let mut command = powershell(
        r#"
        $ErrorActionPreference = 'Stop'
        Add-Type -AssemblyName System.Security
        $bytes = [Convert]::FromBase64String([Console]::In.ReadToEnd())
        $entropy = [Text.Encoding]::UTF8.GetBytes($env:SL_CREDENTIAL_TARGET)
        $blob = [Security.Cryptography.ProtectedData]::Protect($bytes, $entropy, 'CurrentUser')
        [Console]::Write([Convert]::ToBase64String($blob))
    "#,
    );
    command.env("SL_CREDENTIAL_TARGET", binding);
    let value = run(command, Some(&STANDARD.encode(password)))?;
    if value.is_empty() {
        return Err("Windows did not return a protected credential.".into());
    }
    Ok(value)
}

#[cfg(windows)]
pub(crate) fn load(reference: &str, binding: &str) -> Result<String, String> {
    let mut command = powershell(
        r#"
        $ErrorActionPreference = 'Stop'
        Add-Type -AssemblyName System.Security
        $blob = [Convert]::FromBase64String([Console]::In.ReadToEnd())
        $entropy = [Text.Encoding]::UTF8.GetBytes($env:SL_CREDENTIAL_TARGET)
        $bytes = [Security.Cryptography.ProtectedData]::Unprotect($blob, $entropy, 'CurrentUser')
        [Console]::Write([Convert]::ToBase64String($bytes))
    "#,
    );
    command.env("SL_CREDENTIAL_TARGET", binding);
    decode(&run(command, Some(reference))?)
}

fn decode(value: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| "Invalid protected credential.")?;
    String::from_utf8(bytes).map_err(|_| "Invalid protected credential.".into())
}

pub(crate) fn discard(references: Vec<String>) {
    #[cfg(not(windows))]
    if !references.is_empty() {
        std::thread::spawn(move || {
            for reference in references {
                let mut command = Command::new("secret-tool");
                command.args(["clear", "application", SERVICE, "credential", &reference]);
                if let Err(error) = run(command, None) {
                    eprintln!("Could not remove an unused SSH credential: {error}");
                }
            }
        });
    }
    #[cfg(windows)]
    drop(references); // DPAPI stores the protected bytes in the config itself.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_password_is_read_but_never_serialized() {
        let mut host: crate::config::SshRemoteHost = serde_json::from_str(
            r#"{"id":"example","name":"Test","host":"example.invalid","password":"dummy-password"}"#,
        ).unwrap();
        assert_eq!(host.password, "dummy-password");
        assert!(serde_json::to_value(&host)
            .unwrap()
            .get("password")
            .is_none());
        let original = binding(&host);
        host.port += 1;
        assert_ne!(original, binding(&host));
    }
}
