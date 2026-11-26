use super::profile::{DriveShare, RdpProfile};
use ironrdp::{
    rdpdr::{backend::RdpdrBackend, Rdpdr},
};
use std::{io, path::Path};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use ironrdp::{
    core::PduResult,
    pdu::encode_err,
    rdpdr::pdu::{efs::*, RdpdrPdu},
    svc::SvcMessage,
};

pub(crate) fn attach(
    connector: &mut ironrdp::connector::ClientConnector,
    profile: &RdpProfile,
) -> io::Result<bool> {
    let configured: Vec<_> = profile
        .drives
        .iter()
        .filter(|share| share.path.is_dir())
        .cloned()
        .collect();
    if configured.len() != profile.drives.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A shared folder is missing or is not a directory. Update the RDP drive list.",
        ));
    }
    if configured.is_empty() {
        return Ok(false);
    }
    let (backend, drives) = backend(&configured)?;
    connector.attach_static_channel(
        Rdpdr::new(backend, "ServerLauncher".into()).with_drives(Some(drives)),
    );
    Ok(true)
}

fn share_name(path: &Path, index: usize) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Shared folder {}", index + 1))
}

#[cfg(windows)]
fn backend(shares: &[DriveShare]) -> io::Result<(Box<dyn RdpdrBackend>, Vec<(u32, String)>)> {
    use ironrdp_rdpdr_native::{RedirectedDrive, WindowsRdpdrBackendFactory};

    let drives = shares
        .iter()
        .enumerate()
        .map(|(index, share)| {
            RedirectedDrive::new(
                index as u32 + 1,
                share_name(&share.path, index),
                &share.path,
                !share.writable,
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let factory = WindowsRdpdrBackendFactory::from_drives(drives)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let names = factory.initial_drives();
    Ok((Box::new(factory.build()), names))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn backend(shares: &[DriveShare]) -> io::Result<(Box<dyn RdpdrBackend>, Vec<(u32, String)>)> {
    use ironrdp_rdpdr_native::backend::NixRdpdrBackend;

    let mut entries = Vec::with_capacity(shares.len());
    let mut names = Vec::with_capacity(shares.len());
    for (index, share) in shares.iter().enumerate() {
        let device_id = index as u32 + 1;
        let path = share.path.canonicalize()?;
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "A shared folder is not a directory.",
            ));
        }
        names.push((device_id, share_name(&path, index)));
        entries.push(ShareBackend {
            writable: share.writable,
            backend: NixRdpdrBackend::new(path.to_string_lossy().into_owned()),
        });
    }
    let routes = names
        .iter()
        .enumerate()
        .map(|(index, (device_id, _))| (*device_id, index))
        .collect();
    Ok((Box::new(MultiDriveBackend { entries, routes }), names))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn backend(_shares: &[DriveShare]) -> io::Result<(Box<dyn RdpdrBackend>, Vec<(u32, String)>)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "RDP folder sharing is not supported on this operating system.",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct ShareBackend {
    writable: bool,
    backend: ironrdp_rdpdr_native::backend::NixRdpdrBackend,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct MultiDriveBackend {
    entries: Vec<ShareBackend>,
    routes: std::collections::HashMap<u32, usize>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
ironrdp::core::impl_as_any!(MultiDriveBackend);

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl MultiDriveBackend {
    fn entry_mut(&mut self, device_id: u32) -> PduResult<&mut ShareBackend> {
        let index = self.routes.get(&device_id).copied().ok_or_else(|| {
            ironrdp::pdu::pdu_other_err!("RDP server addressed an unconfigured shared folder")
        })?;
        Ok(&mut self.entries[index])
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl RdpdrBackend for MultiDriveBackend {
    fn reset(&mut self) -> PduResult<()> {
        for entry in &mut self.entries {
            entry.backend.reset()?;
        }
        Ok(())
    }

    fn handle_server_device_announce_response(
        &mut self,
        response: ServerDeviceAnnounceResponse,
    ) -> PduResult<()> {
        self.entry_mut(response.device_id)?
            .backend
            .handle_server_device_announce_response(response)
    }

    fn handle_scard_call(
        &mut self,
        _req: DeviceControlRequest<ScardIoCtlCode>,
        _call: ScardCall,
    ) -> PduResult<Vec<SvcMessage>> {
        Ok(Vec::new())
    }

    fn handle_drive_io_request(
        &mut self,
        request: ServerDriveIoRequest,
    ) -> PduResult<Vec<SvcMessage>> {
        let device_id = request_device_id(&request);
        let entry = self.entry_mut(device_id)?;
        if !entry.writable && !read_only_request_allowed(&request) {
            return reject_write(request);
        }
        entry.backend.handle_drive_io_request(request)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn request_device_id(request: &ServerDriveIoRequest) -> u32 {
    match request {
        ServerDriveIoRequest::ServerCreateDriveRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::ServerDriveQueryInformationRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::DeviceCloseRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::ServerDriveQueryDirectoryRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::ServerDriveNotifyChangeDirectoryRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::ServerDriveQueryVolumeInformationRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::DeviceControlRequest(req) => req.header.device_id,
        ServerDriveIoRequest::DeviceReadRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::DeviceWriteRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::DeviceFlushBuffersRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::ServerDriveSetInformationRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::ServerDriveLockControlRequest(req) => req.device_io_request.device_id,
        ServerDriveIoRequest::ServerDriveQuerySecurityRequest(req) => {
            req.device_io_request.device_id
        }
        ServerDriveIoRequest::ServerDriveSetSecurityRequest(req) => req.device_io_request.device_id,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_only_request_allowed(request: &ServerDriveIoRequest) -> bool {
    match request {
        ServerDriveIoRequest::ServerCreateDriveRequest(req) => {
            const READ_ACCESS: u32 = DesiredAccess::FILE_READ_DATA_OR_FILE_LIST_DIRECTORY.bits()
                | DesiredAccess::FILE_READ_EA.bits()
                | DesiredAccess::FILE_TRAVERSE.bits()
                | DesiredAccess::FILE_READ_ATTRIBUTES.bits()
                | DesiredAccess::READ_CONTROL.bits()
                | DesiredAccess::SYNCHRONIZE.bits()
                | DesiredAccess::GENERIC_READ.bits()
                | DesiredAccess::GENERIC_EXECUTE.bits();
            req.desired_access.bits() & !READ_ACCESS == 0
                && req.create_disposition == CreateDisposition::FILE_OPEN
                && !req
                    .create_options
                    .contains(CreateOptions::FILE_DELETE_ON_CLOSE)
        }
        ServerDriveIoRequest::DeviceCloseRequest(_)
        | ServerDriveIoRequest::DeviceReadRequest(_)
        | ServerDriveIoRequest::DeviceFlushBuffersRequest(_)
        | ServerDriveIoRequest::ServerDriveQueryInformationRequest(_)
        | ServerDriveIoRequest::ServerDriveQueryDirectoryRequest(_)
        | ServerDriveIoRequest::ServerDriveQueryVolumeInformationRequest(_)
        | ServerDriveIoRequest::ServerDriveNotifyChangeDirectoryRequest(_)
        | ServerDriveIoRequest::ServerDriveQuerySecurityRequest(_)
        | ServerDriveIoRequest::ServerDriveLockControlRequest(_) => true,
        ServerDriveIoRequest::DeviceControlRequest(_)
        | ServerDriveIoRequest::DeviceWriteRequest(_)
        | ServerDriveIoRequest::ServerDriveSetInformationRequest(_)
        | ServerDriveIoRequest::ServerDriveSetSecurityRequest(_) => false,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn reject_write(request: ServerDriveIoRequest) -> PduResult<Vec<SvcMessage>> {
    let status = NtStatus::ACCESS_DENIED;
    let response = match request {
        ServerDriveIoRequest::ServerCreateDriveRequest(req) => {
            RdpdrPdu::DeviceCreateResponse(DeviceCreateResponse {
                device_io_reply: DeviceIoResponse::new(req.device_io_request, status),
                file_id: 0,
                information: Information::empty(),
            })
        }
        ServerDriveIoRequest::DeviceWriteRequest(req) => {
            RdpdrPdu::DeviceWriteResponse(DeviceWriteResponse {
                device_io_reply: DeviceIoResponse::new(req.device_io_request, status),
                length: 0,
            })
        }
        ServerDriveIoRequest::ServerDriveSetInformationRequest(req) => {
            RdpdrPdu::ClientDriveSetInformationResponse(
                ClientDriveSetInformationResponse::new(&req, status)
                    .map_err(|err| encode_err!(err))?,
            )
        }
        ServerDriveIoRequest::ServerDriveSetSecurityRequest(req) => {
            RdpdrPdu::ClientDriveSetSecurityResponse(
                ClientDriveSetSecurityResponse::new(&req, status)
                    .map_err(|err| encode_err!(err))?,
            )
        }
        ServerDriveIoRequest::DeviceControlRequest(req) => {
            RdpdrPdu::DeviceControlResponse(DeviceControlResponse::new(req, status, None))
        }
        _ => {
            return Err(ironrdp::pdu::pdu_other_err!(
                "unexpected request denied for read-only drive"
            ))
        }
    };
    Ok(vec![SvcMessage::from(response)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_names_fall_back_for_root_paths() {
        assert_eq!(share_name(Path::new("/"), 0), "Shared folder 1");
        assert_eq!(share_name(Path::new("/home/user/Docs"), 0), "Docs");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn read_only_shares_allow_open_for_read_but_reject_mutation_rights() {
        let request = |access, disposition, options| {
            ServerDriveIoRequest::ServerCreateDriveRequest(DeviceCreateRequest {
                device_io_request: DeviceIoRequest {
                    device_id: 1,
                    file_id: 0,
                    completion_id: 1,
                    major_function: MajorFunction::Create,
                    minor_function: MinorFunction::from(0),
                },
                desired_access: DesiredAccess::from_bits_retain(access),
                allocation_size: 0,
                file_attributes: FileAttributes::empty(),
                shared_access: SharedAccess::empty(),
                create_disposition: CreateDisposition::from(disposition),
                create_options: CreateOptions::from_bits_retain(options),
                path: String::new(),
            })
        };
        assert!(read_only_request_allowed(&request(
            DesiredAccess::GENERIC_READ.bits(),
            CreateDisposition::FILE_OPEN.into(),
            0
        )));
        assert!(!read_only_request_allowed(&request(
            DesiredAccess::GENERIC_WRITE.bits(),
            CreateDisposition::FILE_OPEN.into(),
            0
        )));
        assert!(!read_only_request_allowed(&request(
            DesiredAccess::GENERIC_READ.bits(),
            CreateDisposition::FILE_OPEN_IF.into(),
            0
        )));
    }
}
