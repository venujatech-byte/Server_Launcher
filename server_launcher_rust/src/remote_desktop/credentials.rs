//! Separate credential bindings for the two desktop protocols.

use crate::{config::SshRemoteHost, ssh_credentials};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DesktopProtocol {
    Rdp,
    Vnc,
}

impl DesktopProtocol {
    pub(crate) const ALL: [Self; 2] = [Self::Rdp, Self::Vnc];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rdp => "RDP",
            Self::Vnc => "VNC",
        }
    }
}

fn binding(host: &SshRemoteHost, protocol: DesktopProtocol) -> Option<String> {
    let (name, port, user, domain, tunnel) = match protocol {
        DesktopProtocol::Rdp => {
            let p = host.rdp.as_ref()?;
            let user = if p.same_username_as_ssh {
                &host.user
            } else {
                &p.username
            };
            (
                "rdp",
                p.port,
                user.as_str(),
                p.domain.as_str(),
                p.tunnel_via_ssh,
            )
        }
        DesktopProtocol::Vnc => {
            let p = host.vnc.as_ref()?;
            ("vnc", p.port, p.username.as_str(), "", p.tunnel_via_ssh)
        }
    };
    let ssh_target = tunnel.then(|| ssh_credentials::binding(host));
    let mut digest = Sha256::new();
    for part in [
        "server-launcher/desktop/v1",
        name,
        &host.id,
        &host.host,
        user,
        domain,
        ssh_target.as_deref().unwrap_or_default(),
    ] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    digest.update(port.to_le_bytes());
    Some(format!("{:x}", digest.finalize()))
}

fn reference(host: &SshRemoteHost, protocol: DesktopProtocol) -> Option<&String> {
    match protocol {
        DesktopProtocol::Rdp => host.rdp.as_ref()?.credential_ref.as_ref(),
        DesktopProtocol::Vnc => host.vnc.as_ref()?.credential_ref.as_ref(),
    }
}

/// Stage a credential on a draft host. Discard the returned reference if saving the draft fails.
pub fn protect(
    host: &mut SshRemoteHost,
    protocol: DesktopProtocol,
    password: Zeroizing<String>,
) -> Result<String, String> {
    let binding = binding(host, protocol).ok_or("Desktop protocol is not configured.")?;
    let new_reference = ssh_credentials::protect(&password, &binding)?;
    match protocol {
        DesktopProtocol::Rdp => {
            if let Some(profile) = host.rdp.as_mut() {
                profile.credential_ref = Some(new_reference.clone());
            }
        }
        DesktopProtocol::Vnc => {
            if let Some(profile) = host.vnc.as_mut() {
                profile.credential_ref = Some(new_reference.clone());
            }
        }
    }
    Ok(new_reference)
}

/// Load the password in the session process when it is needed.
pub fn load(
    host: &SshRemoteHost,
    protocol: DesktopProtocol,
) -> Result<Option<Zeroizing<String>>, String> {
    let Some(reference) = reference(host, protocol) else {
        return Ok(None);
    };
    let binding = binding(host, protocol).ok_or("Desktop protocol is not configured.")?;
    ssh_credentials::load(reference, &binding).map(|password| Some(Zeroizing::new(password)))
}

pub(crate) fn references(host: &SshRemoteHost) -> impl Iterator<Item = &String> {
    [
        host.credential_ref.as_ref(),
        reference(host, DesktopProtocol::Rdp),
        reference(host, DesktopProtocol::Vnc),
    ]
    .into_iter()
    .flatten()
}

pub(crate) fn retain_matching(previous: &SshRemoteHost, updated: &mut SshRemoteHost) {
    let rdp = DesktopProtocol::Rdp;
    if binding(previous, rdp) != binding(updated, rdp)
        && reference(previous, rdp) == reference(updated, rdp)
    {
        if let Some(profile) = &mut updated.rdp {
            profile.credential_ref = None;
        }
    }

    let vnc = DesktopProtocol::Vnc;
    if binding(previous, vnc) != binding(updated, vnc)
        && reference(previous, vnc) == reference(updated, vnc)
    {
        if let Some(profile) = &mut updated.vnc {
            profile.credential_ref = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_protocols_and_clears_only_the_changed_target() {
        let original: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test","host":"example.invalid","rdp":{"credential_ref":"one"},"vnc":{"credential_ref":"two"}}"#,
        )
        .unwrap();
        let rdp = DesktopProtocol::Rdp;
        let vnc = DesktopProtocol::Vnc;
        assert_ne!(binding(&original, rdp), binding(&original, vnc));
        let mut edited = original.clone();
        edited.rdp.as_mut().unwrap().port += 1;
        retain_matching(&original, &mut edited);
        assert!(reference(&edited, rdp).is_none());
        assert_eq!(reference(&edited, vnc), reference(&original, vnc));
        assert_eq!(references(&original).count(), 2);
    }
}

/// References created for a pending save. Cancelled saves remove their staged secrets.
#[derive(Default)]
pub(crate) struct StagedCredentials(Vec<String>);

impl StagedCredentials {
    pub(crate) fn keep(&mut self) {
        self.0.clear();
    }

    pub(crate) fn stage(
        &mut self,
        host: &mut SshRemoteHost,
        protocol: DesktopProtocol,
        password: Zeroizing<String>,
    ) -> Result<(), String> {
        if !password.is_empty() {
            self.0.push(protect(host, protocol, password)?);
        }
        Ok(())
    }
}

impl Drop for StagedCredentials {
    fn drop(&mut self) {
        ssh_credentials::discard(std::mem::take(&mut self.0));
    }
}
