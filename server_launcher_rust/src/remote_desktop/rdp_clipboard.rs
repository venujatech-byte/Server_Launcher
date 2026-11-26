use ironrdp::cliprdr::{backend::CliprdrBackend, pdu::*};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct ClipboardBackend {
    offered: VecDeque<ClipboardFormatId>,
    pub(super) receive_enabled: bool,
    max_image_bytes: usize,
    pub(super) local_text: Option<String>,
    pub(super) local_image: Option<Vec<u8>>,
    pub(super) reply: Option<OwnedFormatDataResponse>,
    pending: Option<ClipboardFormatId>,
    superseded: bool,
    pub(super) received: Option<String>,
    pub(super) received_image: Option<super::clipboard_image::ClipboardImage>,
}

impl std::fmt::Debug for ClipboardBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardBackend").finish_non_exhaustive()
    }
}

ironrdp::core::impl_as_any!(ClipboardBackend);

impl ClipboardBackend {
    pub(super) fn new(receive_enabled: bool, max_image_bytes: usize) -> Self {
        Self {
            receive_enabled,
            max_image_bytes,
            ..Default::default()
        }
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
    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_request_can_be_followed_by_a_new_offer() {
        let mut backend = ClipboardBackend::new(true, super::clipboard_image::MAX_BYTES);
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
        let mut backend = ClipboardBackend::new(true, super::clipboard_image::MAX_BYTES);
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
}
