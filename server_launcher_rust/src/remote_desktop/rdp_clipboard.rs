use ironrdp::cliprdr::{backend::CliprdrBackend, pdu::*};

#[derive(Debug, Default)]
pub(super) struct ClipboardBackend {
    offered: bool,
    pending: bool,
    superseded: bool,
    pub(super) received: Option<String>,
}

ironrdp::core::impl_as_any!(ClipboardBackend);

impl ClipboardBackend {
    pub(super) fn request_text(&mut self) -> bool {
        if self.pending || !std::mem::take(&mut self.offered) {
            return false;
        }
        self.pending = true;
        self.superseded = false;
        true
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
        self.superseded = self.pending;
        self.received = None;
        self.offered = formats
            .iter()
            .any(|format| format.id() == ClipboardFormatId::CF_UNICODETEXT);
    }
    fn on_format_data_request(&mut self, _: FormatDataRequest) {}
    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if std::mem::take(&mut self.pending)
            && !self.superseded
            && !response.is_error()
            && response.data().len() <= super::vnc::CLIPBOARD_LIMIT
        {
            self.received = response.to_unicode_string().ok();
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
    fn newer_offer_invalidates_in_flight_clipboard_text() {
        let mut backend = ClipboardBackend::default();
        let formats = [ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)];
        backend.on_remote_copy(&formats);
        assert!(backend.request_text());
        backend.on_remote_copy(&[]);
        backend.on_format_data_response(FormatDataResponse::new_unicode_string("stale"));
        assert!(backend.received.is_none());

        backend.on_remote_copy(&formats);
        assert!(backend.request_text());
        backend.on_format_data_response(FormatDataResponse::new_unicode_string("hello ✓"));
        assert_eq!(backend.received.as_deref(), Some("hello ✓"));
    }
}
