use std::{
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use super::clipboard_image::ClipboardImage;

pub(crate) struct ClipboardSync {
    focused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    remote: Arc<Mutex<Option<PendingClipboard>>>,
    pending_local: Arc<Mutex<Option<PendingClipboard>>>,
    notice: Arc<Mutex<Option<&'static str>>>,
}

pub(crate) enum PendingClipboard {
    Text(String),
    Image(ClipboardImage),
}

fn queue_latest(slot: &Mutex<Option<PendingClipboard>>, value: PendingClipboard) {
    *slot.lock().unwrap() = Some(value);
}

fn take_notice(slot: &Mutex<Option<&'static str>>) -> Option<&'static str> {
    slot.lock().unwrap().take()
}

fn report_notice(slot: &Mutex<Option<&'static str>>, notice: &'static str) {
    *slot.lock().unwrap() = Some(notice);
}

impl ClipboardSync {
    pub(crate) fn start(ctx: egui::Context, read_local: bool, max_image_bytes: usize) -> Self {
        let focused = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let remote = Arc::new(Mutex::new(None::<PendingClipboard>));
        let pending_local = Arc::new(Mutex::new(None::<PendingClipboard>));
        let notice = Arc::new(Mutex::new(None));
        let (worker_focus, worker_stopped) = (focused.clone(), stopped.clone());
        let (worker_remote, worker_pending_local, worker_notice) =
            (remote.clone(), pending_local.clone(), notice.clone());

        thread::spawn(move || {
            let mut clipboard = None;
            let mut last_text = None;
            let mut last_image = None;
            let mut init_notice_sent = false;
            let mut remote_write_failed = false;
            while !worker_stopped.load(Ordering::Acquire) {
                if clipboard.is_none() {
                    clipboard = arboard::Clipboard::new().ok();
                    if clipboard.is_none() {
                        if !init_notice_sent {
                            report_notice(
                                &worker_notice,
                                "This device's clipboard is unavailable; retrying.",
                            );
                            ctx.request_repaint();
                            init_notice_sent = true;
                        }
                        thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    init_notice_sent = false;
                }
                if worker_focus.load(Ordering::Acquire) {
                    let Some(clipboard) = clipboard.as_mut() else {
                        continue;
                    };
                    let remote = worker_remote.lock().unwrap().take();
                    if let Some(remote) = remote {
                        match remote {
                            PendingClipboard::Text(text) => {
                                if clipboard.set_text(text.clone()).is_ok() {
                                    last_text = Some(text);
                                    remote_write_failed = false;
                                    report_notice(
                                        &worker_notice,
                                        "Remote clipboard copied to this device.",
                                    );
                                    ctx.request_repaint();
                                } else {
                                    if !remote_write_failed {
                                        report_notice(
                                            &worker_notice,
                                            "Could not write the remote clipboard here; retrying.",
                                        );
                                        ctx.request_repaint();
                                    }
                                    remote_write_failed = true;
                                    let mut pending = worker_remote.lock().unwrap();
                                    if pending.is_none() {
                                        *pending = Some(PendingClipboard::Text(text));
                                    }
                                }
                            }
                            PendingClipboard::Image(image) => {
                                let fingerprint = image_fingerprint(&image.rgba);
                                if clipboard
                                    .set_image(arboard::ImageData {
                                        width: image.width,
                                        height: image.height,
                                        bytes: image.rgba.clone().into(),
                                    })
                                    .is_ok()
                                {
                                    last_image = Some(fingerprint);
                                    remote_write_failed = false;
                                    report_notice(
                                        &worker_notice,
                                        "Remote clipboard image copied to this device.",
                                    );
                                    ctx.request_repaint();
                                } else {
                                    if !remote_write_failed {
                                        report_notice(
                                            &worker_notice,
                                            "Could not write the remote image clipboard here; retrying.",
                                        );
                                        ctx.request_repaint();
                                    }
                                    remote_write_failed = true;
                                    let mut pending = worker_remote.lock().unwrap();
                                    if pending.is_none() {
                                        *pending = Some(PendingClipboard::Image(image));
                                    }
                                }
                            }
                        }
                    }
                    if read_local {
                        let local_text = clipboard.get_text();
                        let local_image = if should_read_image(local_text.as_deref().ok()) {
                            clipboard.get_image().ok()
                        } else {
                            None
                        };
                        if let Some(image) = local_image {
                            let valid_size = image
                                .width
                                .checked_mul(image.height)
                                .and_then(|pixels| pixels.checked_mul(4))
                                == Some(image.bytes.len())
                                && image.bytes.len() <= max_image_bytes.saturating_sub(124);
                            if valid_size {
                                let image = ClipboardImage {
                                    width: image.width,
                                    height: image.height,
                                    rgba: image.bytes.to_vec(),
                                };
                                let fingerprint = image_fingerprint(&image.rgba);
                                if last_image != Some(fingerprint) {
                                    last_image = Some(fingerprint);
                                    queue_latest(
                                        &worker_pending_local,
                                        PendingClipboard::Image(image),
                                    );
                                    ctx.request_repaint();
                                }
                            }
                        } else if let Ok(text) = local_text {
                            if should_forward(&text, &mut last_text) {
                                queue_latest(&worker_pending_local, PendingClipboard::Text(text));
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(200));
            }
        });

        Self {
            focused,
            stopped,
            remote,
            pending_local,
            notice,
        }
    }

    pub(crate) fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Release);
    }

    pub(crate) fn set_remote_text(&self, text: String) {
        queue_latest(&self.remote, PendingClipboard::Text(text));
    }

    pub(crate) fn set_remote_image(&self, image: ClipboardImage) {
        queue_latest(&self.remote, PendingClipboard::Image(image));
    }

    pub(crate) fn take_pending(&self) -> Option<PendingClipboard> {
        self.pending_local.lock().unwrap().take()
    }

    pub(crate) fn take_notice(&self) -> Option<&'static str> {
        take_notice(&self.notice)
    }
}

fn image_fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl Drop for ClipboardSync {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

fn should_forward(text: &str, last_text: &mut Option<String>) -> bool {
    if text.len() > super::vnc::CLIPBOARD_LIMIT || last_text.as_deref() == Some(text) {
        return false;
    }
    *last_text = Some(text.to_owned());
    true
}

fn should_read_image(text: Option<&str>) -> bool {
    text.is_none_or(str::is_empty)
}

#[cfg(test)]
mod tests {
    use super::{queue_latest, report_notice, should_forward, take_notice, PendingClipboard};
    use std::sync::Mutex;

    #[test]
    fn forwards_changes_once_and_respects_transfer_limit() {
        let mut last = None;
        assert!(should_forward("local", &mut last));
        assert!(!should_forward("local", &mut last));
        assert!(!should_forward(
            &"x".repeat(super::super::vnc::CLIPBOARD_LIMIT + 1),
            &mut last
        ));
    }

    #[test]
    fn checks_image_fallback_for_empty_or_unavailable_text() {
        assert!(super::should_read_image(None));
        assert!(super::should_read_image(Some("")));
        assert!(!super::should_read_image(Some("text")));
    }

    #[test]
    fn latest_clipboard_kind_replaces_an_older_pending_value() {
        let slot = Mutex::new(None);
        queue_latest(
            &slot,
            PendingClipboard::Image(super::ClipboardImage {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            }),
        );
        queue_latest(&slot, PendingClipboard::Text("new text".into()));

        assert!(matches!(
            slot.into_inner().unwrap(),
            Some(PendingClipboard::Text(text)) if text == "new text"
        ));
    }

    #[test]
    fn clipboard_notice_is_consumed_once() {
        let slot = Mutex::new(None);
        report_notice(&slot, "clipboard unavailable");
        assert_eq!(take_notice(&slot), Some("clipboard unavailable"));
        assert_eq!(take_notice(&slot), None);
    }
}
