use ironrdp::cliprdr::{backend::CliprdrBackend, pdu::*};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

const MAX_SHARED_FILES: usize = 256;
const MAX_FILE_CHUNK: usize = 1024 * 1024;
const MAX_REMOTE_FILE_SIZE: u64 = u32::MAX as u64;

struct RemoteDownload {
    stream_id: u32,
    index: i32,
    data_id: Option<u32>,
    path: PathBuf,
    file: File,
    size: Option<u64>,
    position: u64,
}

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
    remote_files: Vec<FileDescriptor>,
    remote_file_data_id: Option<u32>,
    remote_download_queue: VecDeque<(i32, PathBuf, Option<u32>)>,
    active_remote_download: Option<RemoteDownload>,
    remote_file_requests: VecDeque<FileContentsRequest>,
    next_remote_stream_id: u32,
    completed_remote_files: usize,
    failed_remote_files: usize,
    remote_download_result: Option<String>,
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

    pub(super) fn remote_file_count(&self) -> usize {
        let mut names = HashSet::new();
        self.remote_files
            .iter()
            .take(MAX_SHARED_FILES)
            .filter(|file| {
                !file.attributes.is_some_and(|attributes| {
                    attributes.contains(ClipboardFileAttributes::DIRECTORY)
                })
            })
            .filter_map(|file| safe_remote_filename(&file.name))
            .filter(|name| names.insert(name.to_ascii_lowercase()))
            .count()
    }

    pub(super) fn take_file_response(&mut self) -> Option<FileContentsResponse<'static>> {
        self.file_responses.pop_front()
    }

    pub(super) fn take_remote_file_request(&mut self) -> Option<FileContentsRequest> {
        self.remote_file_requests.pop_front()
    }

    pub(super) fn take_remote_download_result(&mut self) -> Option<String> {
        self.remote_download_result.take()
    }

    pub(super) fn start_remote_download(&mut self, directory: PathBuf) {
        if self.active_remote_download.is_some() || !self.remote_download_queue.is_empty() {
            self.remote_download_result =
                Some("A remote file transfer is already in progress.".into());
            return;
        }
        self.remote_download_queue.clear();
        self.active_remote_download = None;
        self.remote_file_requests.clear();
        self.completed_remote_files = 0;
        self.failed_remote_files = 0;
        self.remote_download_result = None;
        if !directory.is_dir() {
            self.remote_download_result = Some("Choose an existing local folder.".into());
            return;
        }
        let mut names = HashSet::new();
        for (index, file) in self.remote_files.iter().take(MAX_SHARED_FILES).enumerate() {
            if file
                .attributes
                .is_some_and(|attributes| attributes.contains(ClipboardFileAttributes::DIRECTORY))
            {
                continue;
            }
            let Some(name) = safe_remote_filename(&file.name) else {
                self.failed_remote_files += 1;
                continue;
            };
            if !names.insert(name.to_ascii_lowercase()) {
                self.failed_remote_files += 1;
                continue;
            }
            let Ok(index) = i32::try_from(index) else {
                self.failed_remote_files += 1;
                continue;
            };
            self.remote_download_queue.push_back((
                index,
                directory.join(name),
                self.remote_file_data_id,
            ));
        }
        self.advance_remote_download();
    }

    fn advance_remote_download(&mut self) {
        while let Some((index, path, data_id)) = self.remote_download_queue.pop_front() {
            let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&path) else {
                self.failed_remote_files += 1;
                continue;
            };
            self.active_remote_download = Some(RemoteDownload {
                stream_id: 0,
                index,
                data_id,
                path,
                file,
                size: None,
                position: 0,
            });
            self.queue_remote_file_request(FileContentsFlags::SIZE, 0, 8);
            return;
        }
        let total = self.completed_remote_files + self.failed_remote_files;
        self.remote_download_result = Some(if total == 0 {
            "No safe remote files were available to download.".into()
        } else if self.failed_remote_files == 0 {
            format!("Downloaded {} remote file(s).", self.completed_remote_files)
        } else {
            format!(
                "Downloaded {} remote file(s); {} could not be saved.",
                self.completed_remote_files, self.failed_remote_files
            )
        });
    }

    fn queue_remote_file_request(&mut self, flags: FileContentsFlags, position: u64, size: u32) {
        let Some(active) = &mut self.active_remote_download else {
            return;
        };
        self.next_remote_stream_id = self.next_remote_stream_id.wrapping_add(1).max(1);
        active.stream_id = self.next_remote_stream_id;
        self.remote_file_requests.push_back(FileContentsRequest {
            stream_id: active.stream_id,
            index: active.index,
            flags,
            position,
            requested_size: size,
            data_id: active.data_id,
        });
    }

    fn handle_remote_file_response(&mut self, response: FileContentsResponse<'_>) {
        let Some(active) = self.active_remote_download.as_mut() else {
            return;
        };
        if active.stream_id != response.stream_id() {
            return;
        }
        if response.is_error() {
            self.fail_active_remote_download();
            return;
        }
        if active.size.is_none() {
            let Ok(size) = response.data_as_size() else {
                self.fail_active_remote_download();
                return;
            };
            if size > MAX_REMOTE_FILE_SIZE {
                self.fail_active_remote_download();
                return;
            }
            active.size = Some(size);
            if size == 0 {
                self.active_remote_download = None;
                self.completed_remote_files += 1;
                self.advance_remote_download();
            } else {
                let size = size.min(MAX_FILE_CHUNK as u64) as u32;
                self.queue_remote_file_request(FileContentsFlags::RANGE, 0, size);
            }
            return;
        }

        let size = active.size.unwrap_or_default();
        let remaining = size.saturating_sub(active.position);
        if response.data().is_empty()
            || response.data().len() > MAX_FILE_CHUNK
            || response.data().len() as u64 > remaining
            || active.file.write_all(response.data()).is_err()
        {
            self.fail_active_remote_download();
            return;
        }
        active.position += response.data().len() as u64;
        if active.position == size {
            self.active_remote_download = None;
            self.completed_remote_files += 1;
            self.advance_remote_download();
        } else {
            let next = (size - active.position).min(MAX_FILE_CHUNK as u64) as u32;
            let position = active.position;
            self.queue_remote_file_request(FileContentsFlags::RANGE, position, next);
        }
    }

    fn fail_active_remote_download(&mut self) {
        if let Some(active) = self.active_remote_download.take() {
            drop(active.file);
            let _ = fs::remove_file(active.path);
            self.failed_remote_files += 1;
        }
        self.advance_remote_download();
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
            | if self.receive_enabled || self.share_files_enabled {
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
        self.remote_files.clear();
        self.remote_file_data_id = None;
        self.offered.clear();
        if self.receive_enabled {
            if let Some(file_list) = formats.iter().find(|format| {
                format.name() == Some(&ClipboardFormatName::FILE_LIST)
            }) {
                self.offered.push_back(file_list.id());
            }
            self.offered.extend(
                [
                    ClipboardFormatId::CF_DIBV5,
                    ClipboardFormatId::CF_DIB,
                    ClipboardFormatId::CF_UNICODETEXT,
                ]
                .into_iter()
                .filter(|preferred| formats.iter().any(|format| format.id() == *preferred)),
            );
        }
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
    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        self.handle_remote_file_response(response);
    }
    fn on_lock(&mut self, data_id: LockDataId) {
        if self.locked_files.len() < 16 || self.locked_files.contains_key(&data_id.0) {
            self.locked_files.insert(data_id.0, self.local_files.clone());
        }
    }
    fn on_unlock(&mut self, data_id: LockDataId) {
        self.locked_files.remove(&data_id.0);
    }

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], clip_data_id: Option<u32>) {
        if self.superseded {
            self.pending = None;
            self.superseded = false;
            return;
        }
        self.remote_files = files.iter().take(MAX_SHARED_FILES).cloned().collect();
        self.remote_file_data_id = clip_data_id;
        self.pending = None;
        self.offered.clear();
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

fn safe_remote_filename(name: &str) -> Option<String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 240
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return None;
    }
    let device = name
        .split('.')
        .next()
        .unwrap_or(name)
        .to_ascii_uppercase();
    if matches!(
        device.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
            | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4"
            | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    ) {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_file_names_cannot_escape_the_selected_folder() {
        for name in ["../outside", r"..\outside", "C:outside", "NUL.txt"] {
            assert_eq!(safe_remote_filename(name), None);
        }
        assert_eq!(
            safe_remote_filename("report.txt").as_deref(),
            Some("report.txt")
        );
    }

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

    #[test]
    fn locked_remote_paste_keeps_the_original_local_file_list() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.txt");
        let replacement = directory.path().join("replacement.txt");
        std::fs::write(&original, b"original").unwrap();
        std::fs::write(&replacement, b"replacement").unwrap();
        let mut backend = ClipboardBackend::new(true, true, 1024);
        backend.local_files = vec![original];
        backend.on_lock(LockDataId(4));
        backend.local_files = vec![replacement];

        backend.on_file_contents_request(FileContentsRequest {
            stream_id: 12,
            index: 0,
            flags: FileContentsFlags::RANGE,
            position: 0,
            requested_size: 8,
            data_id: Some(4),
        });

        let response = backend.take_file_response().unwrap();
        assert!(!response.is_error());
        assert_eq!(response.data(), b"original");
    }
}
