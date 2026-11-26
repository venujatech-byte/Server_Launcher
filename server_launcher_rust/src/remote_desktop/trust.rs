use super::vnc::Connection;
use crate::config::SshRemoteHost;
use rustls::{
    client::{
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        WebPkiServerVerifier,
    },
    crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider},
    pki_types::{CertificateDer, ServerName, UnixTime},
    CertificateError, DigitallySignedStruct, Error, SignatureScheme,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use x509_cert::der::Decode;

#[derive(Clone, Copy)]
pub(crate) enum Choice {
    Once,
    Always,
    Cancel,
}

pub(crate) struct Prompt {
    pub subject: String,
    pub fingerprint: String,
    pub answer: mpsc::Sender<Choice>,
}

pub(crate) struct SavedPin {
    pub name: String,
    pub fingerprint: String,
    pub path: PathBuf,
}

pub(crate) fn saved_pins(config_path: &Path, hosts: &[SshRemoteHost]) -> io::Result<Vec<SavedPin>> {
    let directory = config_path.with_file_name("rdp-trust");
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut pins = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "sha256")
        {
            continue;
        }
        let path = entry.path();
        let name = hosts
            .iter()
            .find(|host| pin_path(config_path, host) == path)
            .map_or_else(
                || "Unmatched or changed host".to_owned(),
                |host| host.name.clone(),
            );
        let fingerprint = read_pin(&path)
            .map(|value| value.unwrap_or_else(|| "Fingerprint missing".to_owned()))
            .unwrap_or_else(|_| "Invalid fingerprint file".to_owned());
        pins.push(SavedPin {
            name,
            fingerprint,
            path,
        });
    }
    pins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(pins)
}

pub(crate) fn remove_saved_pin(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

pub(crate) struct Verifier {
    system: Option<Arc<WebPkiServerVerifier>>,
    provider: Arc<CryptoProvider>,
    pin: PathBuf,
    accepted: Mutex<Option<String>>,
    permanent: Mutex<Option<String>>,
    failure: Mutex<Option<&'static str>>,
    ask: Box<dyn Fn(String, String) -> Choice + Send + Sync>,
}

impl std::fmt::Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RdpCertificateVerifier")
            .finish_non_exhaustive()
    }
}

impl Verifier {
    pub(crate) fn new(
        config_path: &Path,
        host: &SshRemoteHost,
        provider: Arc<CryptoProvider>,
        state: Arc<Mutex<Connection>>,
        ctx: egui::Context,
    ) -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
        let system = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
            .build()
            .ok();
        Self {
            system,
            provider,
            pin: pin_path(config_path, host),
            accepted: Mutex::new(None),
            permanent: Mutex::new(None),
            failure: Mutex::new(None),
            ask: Box::new(move |subject, fingerprint| {
                let (answer, reply) = mpsc::channel();
                state.lock().unwrap().trust_prompt = Some(Prompt {
                    subject,
                    fingerprint,
                    answer,
                });
                ctx.request_repaint();
                let choice = reply
                    .recv_timeout(Duration::from_secs(90))
                    .unwrap_or(Choice::Cancel);
                state.lock().unwrap().trust_prompt = None;
                ctx.request_repaint();
                choice
            }),
        }
    }

    pub(crate) fn failure(&self) -> &'static str {
        self.failure
            .lock()
            .unwrap()
            .unwrap_or("RDP TLS verification failed. Check the host name and certificate.")
    }

    pub(crate) fn finish(&self) -> io::Result<()> {
        let Some(fingerprint) = self.permanent.lock().unwrap().take() else {
            return Ok(());
        };
        let directory = self
            .pin
            .parent()
            .ok_or_else(|| io::Error::other("Invalid certificate store path."))?;
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        writeln!(file, "{fingerprint}")?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(&self.pin) {
            Ok(_) => Ok(()),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists
                && read_pin(&self.pin)?.as_deref() == Some(&fingerprint) => Ok(()),
            Err(_) => Err(io::Error::other("Could not save this RDP certificate. A different certificate may already be trusted.")),
        }
    }

    fn reject(&self, message: &'static str) -> Error {
        *self.failure.lock().unwrap() = Some(message);
        Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
    }
}

fn pin_path(path: &Path, host: &SshRemoteHost) -> PathBuf {
    let mut digest = Sha256::new();
    for value in [&host.id, &host.host] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    if let Some(profile) = &host.rdp {
        digest.update(profile.port.to_le_bytes());
        digest.update([u8::from(profile.tunnel_via_ssh)]);
        if profile.tunnel_via_ssh {
            digest.update(crate::ssh_credentials::binding(host));
        }
    }
    path.with_file_name("rdp-trust")
        .join(format!("{:x}.sha256", digest.finalize()))
}

fn read_pin(path: &Path) -> io::Result<Option<String>> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut value = String::new();
    file.take(67).read_to_string(&mut value)?;
    let value = value.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("Invalid RDP certificate fingerprint."));
    }
    Ok(Some(value.to_ascii_lowercase()))
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let cert = x509_cert::Certificate::from_der(certificate.as_ref())
            .map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        let validity = cert.tbs_certificate.validity;
        if !(validity.not_before.to_unix_duration().as_secs()
            ..=validity.not_after.to_unix_duration().as_secs())
            .contains(&now.as_secs())
        {
            return Err(self.reject("The RDP certificate is expired or not yet valid."));
        }
        let fingerprint = format!("{:x}", Sha256::digest(certificate.as_ref()));
        if let Some(saved) = read_pin(&self.pin)
            .map_err(|_| self.reject("Could not read the saved RDP certificate fingerprint."))?
        {
            return if saved == fingerprint {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(self.reject("The RDP certificate changed. Verify the new fingerprint with the server administrator before changing trust."))
            };
        }
        if let Some(system) = &self.system {
            match system.verify_server_cert(certificate, intermediates, name, ocsp, now) {
                Ok(valid) => return Ok(valid),
                Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {}
                Err(error) => return Err(error),
            }
        }
        if self.accepted.lock().unwrap().as_deref() == Some(&fingerprint) {
            return Ok(ServerCertVerified::assertion());
        }
        match (self.ask)(
            cert.tbs_certificate.subject.to_string(),
            fingerprint.clone(),
        ) {
            Choice::Cancel => return Err(self.reject("The RDP certificate was not trusted.")),
            Choice::Always => *self.permanent.lock().unwrap() = Some(fingerprint.clone()),
            Choice::Once => {}
        }
        *self.accepted.lock().unwrap() = Some(fingerprint);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_removes_saved_certificate_pins() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("servers.json");
        let host: SshRemoteHost = serde_json::from_str(
            r#"{"id":"test","name":"Test host","host":"example.test","rdp":{"enabled":true}}"#,
        )
        .unwrap();
        let path = pin_path(&config_path, &host);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{}\n", "a".repeat(64))).unwrap();

        let pins = saved_pins(&config_path, std::slice::from_ref(&host)).unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].name, "Test host");
        assert_eq!(pins[0].fingerprint, "a".repeat(64));
        remove_saved_pin(&pins[0].path).unwrap();
        assert!(saved_pins(&config_path, &[host]).unwrap().is_empty());
    }

    #[test]
    fn treats_a_corrupt_pin_as_an_error_and_normalizes_the_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pin");
        assert!(read_pin(&path).unwrap().is_none());
        fs::write(&path, "junk").unwrap();
        assert!(read_pin(&path).is_err());
        fs::write(&path, format!("{}\n", "A".repeat(64))).unwrap();
        assert_eq!(read_pin(&path).unwrap().unwrap(), "a".repeat(64));
    }
}
