use super::credentials::DesktopProtocol;
use std::{
    collections::HashMap,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Sessions {
    open: HashMap<(String, DesktopProtocol), SyncSender<()>>,
}

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
            if !matches!(focus.try_send(()), Err(TrySendError::Disconnected(()))) {
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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut pipe = child.stdin.take().expect("viewer stdin was piped");
        let (focus, commands) = mpsc::sync_channel(1);
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
            ctx.request_repaint();
        });
        self.open.insert(key, focus);
        Ok(())
    }

    pub(crate) fn close_host(&mut self, id: &str) {
        self.open.retain(|(host_id, _), _| host_id != id);
    }
}
