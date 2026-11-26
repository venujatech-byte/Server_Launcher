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
    let (name, port, user, domain, transport) = match protocol {
        DesktopProtocol::Rdp => {
            let p = host.rdp.as_ref()?;
            let user = if p.same_username_as_ssh {
                &host.user
            } else {
                &p.username
            };
            let gateway = if p.gateway.trim().is_empty() {
                String::new()
            } else {
                super::rdp::gateway_endpoint(&p.gateway).ok()?
            };
            ("rdp", p.port, user.as_str(), p.domain.as_str(), gateway)
        }
        DesktopProtocol::Vnc => {
            let p = host.vnc.as_ref()?;
            ("vnc", p.port, p.username.as_str(), "", String::new())
        }
    };
    Some(binding_digest(host, name, port, user, domain, &transport))
}

fn legacy_binding(host: &SshRemoteHost, protocol: DesktopProtocol, tunnel: bool) -> Option<String> {
    let (name, port, user, domain) = match protocol {
        DesktopProtocol::Rdp => {
            let profile = host.rdp.as_ref()?;
            if !profile.gateway.trim().is_empty() {
                return None;
            }
            let user = if profile.same_username_as_ssh {
                &host.user
            } else {
                &profile.username
            };
            ("rdp", profile.port, user.as_str(), profile.domain.as_str())
        }
        DesktopProtocol::Vnc => {
            let profile = host.vnc.as_ref()?;
            ("vnc", profile.port, profile.username.as_str(), "")
        }
    };
    let ssh_target = tunnel.then(|| ssh_credentials::binding(host));
    Some(binding_digest(
        host,
        name,
        port,
        user,
        domain,
        ssh_target.as_deref().unwrap_or_default(),
    ))
}

fn binding_digest(
    host: &SshRemoteHost,
    name: &str,
    port: u16,
    user: &str,
    domain: &str,
    transport: &str,
) -> String {
    let mut digest = Sha256::new();
    for part in [
        "server-launcher/desktop/v1",
        name,
        &host.id,
        &host.host,
        user,
        domain,
        transport,
    ] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    digest.update(port.to_le_bytes());
    format!("{:x}", digest.finalize())
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
    match ssh_credentials::load(reference, &binding) {
        Ok(password) => Ok(Some(Zeroizing::new(password))),
        Err(error) => {
            if protocol == DesktopProtocol::Rdp
                && host
                    .rdp
                    .as_ref()
                    .is_some_and(|profile| !profile.gateway.trim().is_empty())
            {
                return Err(error);
            }
            let tunnel = match protocol {
                DesktopProtocol::Rdp => host
                    .rdp
                    .as_ref()
                    .is_some_and(|profile| profile.tunnel_via_ssh),
                DesktopProtocol::Vnc => host
                    .vnc
                    .as_ref()
                    .is_some_and(|profile| profile.tunnel_via_ssh),
            };
            for legacy_tunnel in [tunnel, !tunnel] {
                let Some(legacy) = legacy_binding(host, protocol, legacy_tunnel) else {
                    continue;
                };
                if legacy == binding {
                    continue;
                }
                if let Ok(password) = ssh_credentials::load(reference, &legacy) {
                    return Ok(Some(Zeroizing::new(password)));
                }
            }
            Err(error)
        }
    }
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
    fn preserves_both_desktop_references_in_saved_host() {
        let host: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test","host":"example.invalid","rdp":{"credential_ref":"one"},"vnc":{"credential_ref":"two"}}"#,
        )
        .unwrap();
        let saved = serde_json::to_value(&host).unwrap();
        let restored: SshRemoteHost = serde_json::from_value(saved).unwrap();
        assert_eq!(references(&restored).collect::<Vec<_>>(), ["one", "two"]);
    }

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

    #[test]
    fn desktop_password_binding_is_independent_of_ssh_tunnel_mode() {
        let mut host: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test","host":"example.invalid","rdp":{"enabled":true,"port":3389,"username":"user"},"vnc":{"enabled":true,"port":5900}}"#,
        )
        .unwrap();
        for protocol in DesktopProtocol::ALL {
            let original = binding(&host, protocol);
            match protocol {
                DesktopProtocol::Rdp => host.rdp.as_mut().unwrap().tunnel_via_ssh = true,
                DesktopProtocol::Vnc => host.vnc.as_mut().unwrap().tunnel_via_ssh = true,
            }
            assert_eq!(binding(&host, protocol), original);
        }
    }

    #[test]
    fn rdp_password_binding_tracks_normalized_gateway_endpoint() {
        let mut host: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test","host":"example.invalid","rdp":{"enabled":true,"port":3389,"username":"user","gateway":"rdg.example.test"}}"#,
        )
        .unwrap();
        let original = binding(&host, DesktopProtocol::Rdp);
        host.rdp.as_mut().unwrap().gateway = "rdg.example.test:443".into();
        assert_eq!(binding(&host, DesktopProtocol::Rdp), original);
        host.rdp.as_mut().unwrap().gateway = "other.example.test".into();
        assert_ne!(binding(&host, DesktopProtocol::Rdp), original);
        assert!(legacy_binding(&host, DesktopProtocol::Rdp, false).is_none());
    }

    #[test]
    fn invalid_rdp_gateway_cannot_bind_a_credential() {
        let host: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test","host":"example.invalid","rdp":{"enabled":true,"port":3389,"username":"user","gateway":"https://rdg.example.test"}}"#,
        )
        .unwrap();
        assert!(binding(&host, DesktopProtocol::Rdp).is_none());
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
