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
    remote_text: Arc<Mutex<Option<String>>>,
    remote_image: Arc<Mutex<Option<ClipboardImage>>>,
    pending_local: Arc<Mutex<Option<String>>>,
    pending_local_image: Arc<Mutex<Option<ClipboardImage>>>,
}

pub(crate) enum PendingClipboard {
    Text(String),
    Image(ClipboardImage),
}

impl ClipboardSync {
    pub(crate) fn start(ctx: egui::Context, read_local: bool) -> Self {
        let focused = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let remote_text = Arc::new(Mutex::new(None::<String>));
        let remote_image = Arc::new(Mutex::new(None::<ClipboardImage>));
        let pending_local = Arc::new(Mutex::new(None::<String>));
        let pending_local_image = Arc::new(Mutex::new(None::<ClipboardImage>));
        let (worker_focus, worker_stopped) = (focused.clone(), stopped.clone());
        let (worker_remote_text, worker_remote_image) = (remote_text.clone(), remote_image.clone());
        let (worker_pending_local, worker_pending_local_image) =
            (pending_local.clone(), pending_local_image.clone());

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
                    if let Some(text) = worker_remote_text.lock().unwrap().take() {
                        if clipboard.set_text(text.clone()).is_ok() {
                            last_text = Some(text);
                        }
                    }
                    if let Some(image) = worker_remote_image.lock().unwrap().take() {
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
                        }
                    }
                    if read_local {
                        if let Ok(text) = clipboard.get_text() {
                            if should_forward(&text, &mut last_text) {
                                *worker_pending_local.lock().unwrap() = Some(text);
                                ctx.request_repaint();
                            }
                        } else if let Ok(image) = clipboard.get_image() {
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
                                    *worker_pending_local_image.lock().unwrap() = Some(image);
                                    ctx.request_repaint();
                                }
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
            remote_text,
            remote_image,
            pending_local,
            pending_local_image,
        }
    }

    pub(crate) fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Release);
    }

    pub(crate) fn set_remote_text(&self, text: String) {
        *self.remote_text.lock().unwrap() = Some(text);
    }

    pub(crate) fn set_remote_image(&self, image: ClipboardImage) {
        *self.remote_image.lock().unwrap() = Some(image);
    }

    pub(crate) fn take_pending(&self) -> Option<PendingClipboard> {
        self.pending_local
            .lock()
            .unwrap()
            .take()
            .map(PendingClipboard::Text)
            .or_else(|| {
                self.pending_local_image
                    .lock()
                    .unwrap()
                    .take()
                    .map(PendingClipboard::Image)
            })
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
}
