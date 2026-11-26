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
}

pub(crate) enum PendingClipboard {
    Text(String),
    Image(ClipboardImage),
}

impl ClipboardSync {
    pub(crate) fn start(ctx: egui::Context, read_local: bool) -> Self {
        let focused = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let remote = Arc::new(Mutex::new(None::<PendingClipboard>));
        let pending_local = Arc::new(Mutex::new(None::<PendingClipboard>));
        let (worker_focus, worker_stopped) = (focused.clone(), stopped.clone());
        let (worker_remote, worker_pending_local) = (remote.clone(), pending_local.clone());

        thread::spawn(move || {
            let mut clipboard = None;
            let mut last_text = None;
            let mut last_image = None;
            while !worker_stopped.load(Ordering::Acquire) {
                if clipboard.is_none() {
                    clipboard = arboard::Clipboard::new().ok();
                    if clipboard.is_none() {
                        thread::sleep(Duration::from_secs(1));
                        continue;
                    }
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
                                } else {
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
                                } else {
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
                            let image = ClipboardImage {
                                width: image.width,
                                height: image.height,
                                rgba: image.bytes.to_vec(),
                            };
                            if image
                                .width
                                .checked_mul(image.height)
                                .and_then(|pixels| pixels.checked_mul(4))
                                == Some(image.rgba.len())
                                && image.rgba.len() <= super::clipboard_image::MAX_BYTES - 124
                            {
                                let fingerprint = image_fingerprint(&image.rgba);
                                if last_image != Some(fingerprint) {
                                    last_image = Some(fingerprint);
                                    *worker_pending_local.lock().unwrap() =
                                        Some(PendingClipboard::Image(image));
                                    ctx.request_repaint();
                                }
                            }
                        } else if let Ok(text) = local_text {
                            if should_forward(&text, &mut last_text) {
                                *worker_pending_local.lock().unwrap() =
                                    Some(PendingClipboard::Text(text));
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
        }
    }

    pub(crate) fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Release);
    }

    pub(crate) fn set_remote_text(&self, text: String) {
        *self.remote.lock().unwrap() = Some(PendingClipboard::Text(text));
    }

    pub(crate) fn set_remote_image(&self, image: ClipboardImage) {
        *self.remote.lock().unwrap() = Some(PendingClipboard::Image(image));
    }

    pub(crate) fn take_pending(&self) -> Option<PendingClipboard> {
        self.pending_local.lock().unwrap().take()
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
    use super::should_forward;

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
}
