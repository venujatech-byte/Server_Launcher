use ironrdp::cliprdr::{backend::CliprdrBackend, pdu::*};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

const MAX_SHARED_FILES: usize = 256;
const MAX_FILE_CHUNK: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct ClipboardBackend {
    offered: VecDeque<ClipboardFormatId>,
    pub(super) receive_enabled: bool,
    share_files_enabled: bool,
    max_image_bytes: usize,
    pub(super) local_text: Option<String>,
    pub(super) local_image: Option<Vec<u8>>,
    pub(super) reply: Option<OwnedFormatDataResponse>,
    pending: Option<ClipboardFormatId>,
    superseded: bool,
    pub(super) received: Option<String>,
    pub(super) received_image: Option<super::clipboard_image::ClipboardImage>,
    pub(super) local_files: Vec<PathBuf>,
    locked_files: HashMap<u32, Vec<PathBuf>>,
    file_responses: VecDeque<FileContentsResponse<'static>>,
}

impl std::fmt::Debug for ClipboardBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardBackend").finish_non_exhaustive()
    }
}

ironrdp::core::impl_as_any!(ClipboardBackend);

impl ClipboardBackend {
    pub(super) fn new(
        receive_enabled: bool,
        share_files_enabled: bool,
        max_image_bytes: usize,
    ) -> Self {
        Self {
            receive_enabled,
            share_files_enabled,
            max_image_bytes,
            ..Default::default()
        }
    }

    pub(super) fn take_file_response(&mut self) -> Option<FileContentsResponse<'static>> {
        self.file_responses.pop_front()
    }

    pub(super) fn cancel_request(&mut self) {
        self.pending = None;
    }

    pub(super) fn request_format(&mut self) -> Option<ClipboardFormatId> {
        if self.pending.is_some() {
            return None;
        }
        let format = self.offered.pop_front()?;
        self.pending = Some(format);
        self.superseded = false;
        Some(format)
    }
}

impl CliprdrBackend for ClipboardBackend {
    fn temporary_directory(&self) -> &str {
        ""
    }
    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
            | if self.share_files_enabled {
                ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
                    | ClipboardGeneralCapabilityFlags::CAN_LOCK_CLIPDATA
            } else {
                ClipboardGeneralCapabilityFlags::empty()
            }
    }
    fn on_ready(&mut self) {}
    fn on_request_format_list(&mut self) {}
    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}
    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        self.superseded = self.pending.is_some();
        self.received = None;
        self.received_image = None;
        self.offered = if self.receive_enabled {
            [
                ClipboardFormatId::CF_DIBV5,
                ClipboardFormatId::CF_DIB,
                ClipboardFormatId::CF_UNICODETEXT,
            ]
            .into_iter()
            .filter(|preferred| formats.iter().any(|format| format.id() == *preferred))
            .collect()
        } else {
            VecDeque::new()
        };
    }
    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.reply = Some(match (&self.local_text, &self.local_image) {
            (Some(text), _) if request.format == ClipboardFormatId::CF_UNICODETEXT => {
                FormatDataResponse::new_unicode_string(text)
            }
            (_, Some(image))
                if matches!(
                    request.format,
                    ClipboardFormatId::CF_DIB | ClipboardFormatId::CF_DIBV5
                ) =>
            {
                FormatDataResponse::new_data(image.clone())
            }
            _ => FormatDataResponse::new_error(),
        });
    }
    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        let Some(format) = self.pending.take() else {
            return;
        };
        if self.superseded || response.is_error() {
            return;
        }
        match format {
            ClipboardFormatId::CF_UNICODETEXT
                if response.data().len() <= super::vnc::CLIPBOARD_LIMIT =>
            {
                self.received = response.to_unicode_string().ok();
                if self.received.is_some() {
                    self.offered.clear();
                }
            }
            ClipboardFormatId::CF_DIB | ClipboardFormatId::CF_DIBV5
                if response.data().len() <= self.max_image_bytes =>
            {
                self.received_image = super::clipboard_image::decode_dib_with_limit(
                    response.data(),
                    self.max_image_bytes,
                )
                .ok();
                if self.received_image.is_some() {
                    self.offered.clear();
                }
            }
            _ => {}
        }
    }
    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        if self.file_responses.len() < 16 {
            let files = request
                .data_id
                .and_then(|id| self.locked_files.get(&id).map(Vec::as_slice))
                .unwrap_or_else(|| {
                    if request.data_id.is_none() {
                        self.local_files.as_slice()
                    } else {
                        &[]
                    }
                });
            self.file_responses.push_back(file_contents_response(files, request));
        }
    }
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, data_id: LockDataId) {
        if self.locked_files.len() < 16 || self.locked_files.contains_key(&data_id.0) {
            self.locked_files.insert(data_id.0, self.local_files.clone());
        }
    }
    fn on_unlock(&mut self, data_id: LockDataId) {
        self.locked_files.remove(&data_id.0);
    }
}

pub(super) fn shared_file_descriptors(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<FileDescriptor>) {
    let mut shared_paths = Vec::new();
    let mut descriptors = Vec::new();
    for path in paths.iter().take(MAX_SHARED_FILES) {
        let Ok(metadata) = fs::metadata(path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > u64::from(u32::MAX) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.is_empty() || name.contains('\0') || name.encode_utf16().count() > 259 {
            continue;
        }
        shared_paths.push(path.clone());
        descriptors.push(
            FileDescriptor::new(name)
                .with_file_size(metadata.len())
                .with_attributes(ClipboardFileAttributes::NORMAL),
        );
    }
    (shared_paths, descriptors)
}

pub(super) fn file_contents_response(
    paths: &[PathBuf],
    request: FileContentsRequest,
) -> FileContentsResponse<'static> {
    let error = || FileContentsResponse::new_error(request.stream_id);
    if request.flags.validate().is_err() {
        return error();
    }
    let Some(path) = usize::try_from(request.index)
        .ok()
        .and_then(|index| paths.get(index))
    else {
        return error();
    };
    if request.flags == FileContentsFlags::SIZE {
        return fs::metadata(path)
            .map(|metadata| {
                FileContentsResponse::new_size_response(request.stream_id, metadata.len())
            })
            .unwrap_or_else(|_| error());
    }
    if request.flags != FileContentsFlags::RANGE
        || request.requested_size as usize > MAX_FILE_CHUNK
    {
        return error();
    }
    let Ok(mut file) = File::open(path) else {
        return error();
    };
    let Ok(metadata) = file.metadata() else {
        return error();
    };
    if !metadata.is_file() || file.seek(SeekFrom::Start(request.position)).is_err() {
        return error();
    }
    let available = metadata.len().saturating_sub(request.position);
    let length = (request.requested_size as u64).min(available) as usize;
    let mut data = vec![0; length];
    if file.read_exact(&mut data).is_err() {
        return error();
    }
    FileContentsResponse::new_data_response(request.stream_id, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_request_can_be_followed_by_a_new_offer() {
        let mut backend = ClipboardBackend::new(
            true,
            false,
            crate::remote_desktop::clipboard_image::MAX_BYTES,
        );
        let formats = [ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)];
        backend.on_remote_copy(&formats);
        assert_eq!(
            backend.request_format(),
            Some(ClipboardFormatId::CF_UNICODETEXT)
        );
        backend.cancel_request();
        backend.on_remote_copy(&formats);
        assert_eq!(
            backend.request_format(),
            Some(ClipboardFormatId::CF_UNICODETEXT)
        );
    }

    #[test]
    fn newer_offer_invalidates_in_flight_clipboard_text() {
        let mut backend = ClipboardBackend {
            receive_enabled: true,
            ..Default::default()
        };
        let formats = [ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)];
        backend.on_remote_copy(&formats);
        assert_eq!(
            backend.request_format(),
            Some(ClipboardFormatId::CF_UNICODETEXT)
        );
        backend.on_remote_copy(&[]);
        backend.on_format_data_response(FormatDataResponse::new_unicode_string("stale"));
        assert!(backend.received.is_none());

        backend.on_remote_copy(&formats);
        assert_eq!(
            backend.request_format(),
            Some(ClipboardFormatId::CF_UNICODETEXT)
        );
        backend.on_format_data_response(FormatDataResponse::new_unicode_string("hello ✓"));
        assert_eq!(backend.received.as_deref(), Some("hello ✓"));
    }

    #[test]
    fn rejected_image_format_falls_back_to_text_from_same_offer() {
        let mut backend = ClipboardBackend::new(
            true,
            false,
            crate::remote_desktop::clipboard_image::MAX_BYTES,
        );
        let formats = [
            ClipboardFormat::new(ClipboardFormatId::CF_DIBV5),
            ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT),
        ];
        backend.on_remote_copy(&formats);

        assert_eq!(backend.request_format(), Some(ClipboardFormatId::CF_DIBV5));
        backend.on_format_data_response(FormatDataResponse::new_error());

        assert_eq!(
            backend.request_format(),
            Some(ClipboardFormatId::CF_UNICODETEXT)
        );
        backend.on_format_data_response(FormatDataResponse::new_unicode_string("fallback ✓"));
        assert_eq!(backend.received.as_deref(), Some("fallback ✓"));
        assert_eq!(backend.request_format(), None);
    }

    #[test]
    fn local_image_can_be_requested_in_both_dib_formats() {
        let payload = vec![1, 2, 3];
        let mut backend = ClipboardBackend {
            local_image: Some(payload.clone()),
            ..Default::default()
        };

        for format in [ClipboardFormatId::CF_DIBV5, ClipboardFormatId::CF_DIB] {
            backend.on_format_data_request(FormatDataRequest { format });
            let response = backend.reply.take().unwrap();
            assert!(!response.is_error());
            assert_eq!(response.data(), payload);
        }
    }

    #[test]
    fn shared_file_requests_serve_bounded_ranges_and_reject_bad_indexes() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("source.txt");
        std::fs::write(&file, b"remote clipboard file").unwrap();
        let (paths, descriptors) = shared_file_descriptors(std::slice::from_ref(&file));
        assert_eq!(paths, [file]);
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].name, "source.txt");

        let response = file_contents_response(
            &paths,
            FileContentsRequest {
                stream_id: 7,
                index: 0,
                flags: FileContentsFlags::RANGE,
                position: 7,
                requested_size: 9,
                data_id: None,
            },
        );
        assert!(!response.is_error());
        assert_eq!(response.data(), b"clipboard");

        let invalid = file_contents_response(
            &paths,
            FileContentsRequest {
                stream_id: 8,
                index: 1,
                flags: FileContentsFlags::RANGE,
                position: 0,
                requested_size: 4,
                data_id: None,
            },
        );
        assert!(invalid.is_error());
    }

    #[test]
    fn shared_file_list_skips_directories_and_oversized_chunks_fail() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("source.txt");
        std::fs::write(&file, b"data").unwrap();
        let (paths, descriptors) = shared_file_descriptors(&[
            directory.path().to_path_buf(),
            file,
        ]);
        assert_eq!(paths.len(), 1);
        assert_eq!(descriptors.len(), 1);

        let response = file_contents_response(
            &paths,
            FileContentsRequest {
                stream_id: 9,
                index: 0,
                flags: FileContentsFlags::RANGE,
                position: 0,
                requested_size: (MAX_FILE_CHUNK + 1) as u32,
                data_id: None,
            },
        );
        assert!(response.is_error());
    }
}
