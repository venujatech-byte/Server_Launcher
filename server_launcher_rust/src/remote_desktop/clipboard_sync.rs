use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

pub(crate) struct ClipboardSync {
    focused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    remote_text: Arc<Mutex<Option<String>>>,
    pending_local: Arc<Mutex<Option<String>>>,
}

impl ClipboardSync {
    pub(crate) fn start(ctx: egui::Context) -> Self {
        let focused = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let remote_text = Arc::new(Mutex::new(None));
        let pending_local = Arc::new(Mutex::new(None));
        let worker_focus = focused.clone();
        let worker_stopped = stopped.clone();
        let worker_remote_text = remote_text.clone();
        let worker_pending_local = pending_local.clone();

        thread::spawn(move || {
            let Ok(mut clipboard) = arboard::Clipboard::new() else {
                return;
            };
            let mut last_seen = None;
            while !worker_stopped.load(Ordering::Acquire) {
                if worker_focus.load(Ordering::Acquire) {
                    if let Ok(text) = clipboard.get_text() {
                        let forward = should_forward(
                            &text,
                            &mut last_seen,
                            &mut worker_remote_text.lock().unwrap(),
                        );
                        if forward {
                            *worker_pending_local.lock().unwrap() = Some(text);
                            ctx.request_repaint();
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
            pending_local,
        }
    }

    pub(crate) fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Release);
    }

    pub(crate) fn set_remote_text(&self, text: String) {
        *self.remote_text.lock().unwrap() = Some(text);
    }

    pub(crate) fn take_pending(&self) -> Option<String> {
        self.pending_local.lock().unwrap().take()
    }
}

impl Drop for ClipboardSync {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

fn should_forward(
    text: &str,
    last_seen: &mut Option<String>,
    remote_text: &mut Option<String>,
) -> bool {
    if text.len() > super::vnc::CLIPBOARD_LIMIT {
        return false;
    }
    if last_seen.as_deref() == Some(text) {
        return false;
    }
    *last_seen = Some(text.to_owned());
    remote_text.take().as_deref() != Some(text)
}

#[cfg(test)]
mod tests {
    use super::should_forward;

    #[test]
    fn forwards_changes_once_and_does_not_echo_remote_text() {
        let mut last_seen = None;
        let mut remote_text = None;
        assert!(should_forward("local", &mut last_seen, &mut remote_text));
        assert!(!should_forward("local", &mut last_seen, &mut remote_text));

        remote_text = Some("remote".into());
        assert!(!should_forward("remote", &mut last_seen, &mut remote_text));
        assert!(should_forward("new local", &mut last_seen, &mut remote_text));
    }

    #[test]
    fn ignores_clipboard_text_over_the_transfer_limit() {
        let mut last_seen = None;
        let mut remote_text = None;
        let text = "x".repeat(super::super::vnc::CLIPBOARD_LIMIT + 1);
        assert!(!should_forward(&text, &mut last_seen, &mut remote_text));
        assert!(last_seen.is_none());
    }
}
