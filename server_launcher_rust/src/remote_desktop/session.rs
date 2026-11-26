use super::credentials::DesktopProtocol;
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Sessions {
    open: HashMap<(String, DesktopProtocol), Session>,
}

struct Session {
    focus: SyncSender<()>,
    edit: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}

pub(super) const EDIT_SETTINGS: &str = "server-launcher:edit-remote-desktop";

impl Sessions {
    pub(crate) fn open(
        &mut self,
        host_id: &str,
        protocol: DesktopProtocol,
        config_path: &Path,
        theme: &str,
        ctx: &egui::Context,
    ) -> io::Result<()> {
        let key = (host_id.to_owned(), protocol);
        if let Some(focus) = self.open.get(&key) {
            if !matches!(
                focus.focus.try_send(()),
                Err(TrySendError::Disconnected(()))
            ) {
                return Ok(());
            }
        }
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--remote-session")
            .arg(host_id)
            .arg(protocol.label())
            .arg(config_path)
            .arg(theme)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut pipe = child.stdin.take().expect("viewer stdin was piped");
        let (focus, commands) = mpsc::sync_channel(1);
        let output = child.stdout.take().expect("viewer stdout was piped");
        let edit = Arc::new(AtomicBool::new(false));
        let pending_edit = Arc::clone(&edit);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if line == EDIT_SETTINGS {
                    pending_edit.store(true, Ordering::Release);
                    repaint.request_repaint();
                }
            }
        });
        let closed = Arc::new(AtomicBool::new(false));
        let child_closed = Arc::clone(&closed);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            while matches!(child.try_wait(), Ok(None)) {
                match commands.recv_timeout(Duration::from_millis(500)) {
                    Ok(()) if pipe.write_all(b"f").is_err() => break,
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            drop(commands);
            drop(pipe);
            let deadline = Instant::now() + Duration::from_secs(2);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
                let _ = child.wait();
            }
            child_closed.store(true, Ordering::Release);
            ctx.request_repaint();
        });
        self.open.insert(
            key,
            Session {
                focus,
                edit,
                closed,
            },
        );
        Ok(())
    }

    pub(crate) fn open_count(&mut self, id: &str) -> usize {
        self.open
            .retain(|_, session| !session.closed.load(Ordering::Acquire));
        self.open
            .keys()
            .filter(|(host_id, _)| host_id == id)
            .count()
    }

    pub(crate) fn take_edit_request(&self) -> Option<(String, DesktopProtocol)> {
        self.open.iter().find_map(|(key, session)| {
            session
                .edit
                .swap(false, Ordering::AcqRel)
                .then(|| key.clone())
        })
    }

    pub(crate) fn close_host(&mut self, id: &str) {
        self.open.retain(|(host_id, _), _| host_id != id);
    }
}
