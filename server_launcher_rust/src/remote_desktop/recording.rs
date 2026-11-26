use super::vnc::Connection;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) struct Recording {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    written_us: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
    pub(super) result: mpsc::Receiver<Result<PathBuf, String>>,
}

impl Recording {
    pub(super) fn start(connection: Arc<Mutex<Connection>>, ctx: egui::Context) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let worker_paused = paused.clone();
        let written_us = Arc::new(AtomicU64::new(0));
        let progress = written_us.clone();
        let (sender, result) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = sender.send(record(&connection, &cancelled, &worker_paused, &progress));
            ctx.request_repaint();
        });
        Self {
            stop,
            paused,
            written_us,
            worker: Some(worker),
            result,
        }
    }

    pub(super) fn duration(&self) -> Duration {
        Duration::from_micros(self.written_us.load(Ordering::Relaxed))
    }

    pub(super) fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub(super) fn toggle_pause(&self) {
        self.paused.fetch_xor(true, Ordering::Relaxed);
    }

    pub(super) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn record(
    connection: &Mutex<Connection>,
    stop: &AtomicBool,
    paused: &AtomicBool,
    progress: &AtomicU64,
) -> Result<PathBuf, String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let frame = loop {
        if stop.load(Ordering::Relaxed) {
            return Err("Recording cancelled before the first frame.".into());
        }
        {
            let connection = connection.lock().unwrap();
            if !connection.connected {
                return Err("The remote session disconnected.".into());
            }
            if !connection.receiving_frame {
                if let Some(frame) = connection.framebuffer.snapshot() {
                    break frame;
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("No complete remote frame arrived in time.".into());
        }
        thread::sleep(Duration::from_millis(16));
    };
    let size = frame.size;
    let folder = super::screenshot::pictures()?.join("ServerLauncher");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let prefix = format!("remote-{}-", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let output = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".mp4")
        .tempfile_in(&folder)
        .map_err(|error| error.to_string())?
        .into_temp_path();
    let resolution = format!("{}x{}", size[0], size[1]);
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &resolution,
            "-framerate",
            "30",
            "-i",
            "pipe:0",
            "-an",
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let sequence = tempfile::Builder::new()
                .prefix(&prefix)
                .tempdir_in(&folder)
                .map_err(|error| error.to_string())?;
            let mut index = 0_u64;
            let result = sample_frames(
                connection,
                stop,
                paused,
                frame,
                Duration::from_millis(200),
                |frame| {
                    super::screenshot::save_in(
                        frame,
                        sequence.path(),
                        &format!("frame-{index:08}"),
                    )
                    .map_err(|error| error.to_string())?;
                    index += 1;
                    progress.store(index * 200_000, Ordering::Relaxed);
                    Ok(())
                },
            );
            if index != 0 {
                let path = sequence.keep();
                result.map_err(|error| {
                    format!("{error} Partial PNG sequence saved to {}", path.display())
                })?;
                return Ok(path);
            }
            return Err(result
                .err()
                .unwrap_or_else(|| "No recording frames were saved.".into()));
        }
        Err(error) => return Err(format!("Could not start FFmpeg: {error}")),
    };
    let mut pipe = child.stdin.take().expect("FFmpeg stdin was piped");
    let mut pixels = Vec::with_capacity(size[0] * size[1] * 4);
    let mut frames = 0_u64;
    let write_result = sample_frames(
        connection,
        stop,
        paused,
        frame,
        Duration::from_secs_f64(1.0 / 30.0),
        |frame| {
            pixels.clear();
            pixels.extend(frame.pixels.iter().flat_map(|pixel| pixel.to_array()));
            pipe.write_all(&pixels)
                .map_err(|error| format!("Could not write the recording: {error}"))?;
            frames += 1;
            progress.store(frames * 1_000_000 / 30, Ordering::Relaxed);
            Ok(())
        },
    );
    drop(pipe);
    let status = child.wait().map_err(|error| error.to_string())?;
    write_result?;
    if !status.success() {
        return Err(format!("FFmpeg could not encode the recording ({status})."));
    }
    output.keep().map_err(|error| error.to_string())
}

fn sample_frames(
    connection: &Mutex<Connection>,
    stop: &AtomicBool,
    paused: &AtomicBool,
    mut frame: egui::ColorImage,
    interval: Duration,
    mut write: impl FnMut(&egui::ColorImage) -> Result<(), String>,
) -> Result<(), String> {
    let size = frame.size;
    let mut next = Instant::now();
    loop {
        if paused.load(Ordering::Relaxed) {
            if stop.load(Ordering::Relaxed) || !connection.lock().unwrap().connected {
                return Ok(());
            }
            thread::sleep(interval);
            next = Instant::now();
            continue;
        }
        write(&frame)?;
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        next += interval;
        let now = Instant::now();
        if next + interval < now {
            next = now;
        }
        thread::sleep(next.saturating_duration_since(now));
        let updated = {
            let connection = connection.lock().unwrap();
            if !connection.connected {
                break Ok(());
            }
            if connection.receiving_frame {
                None
            } else {
                connection.framebuffer.snapshot()
            }
        };
        if let Some(updated) = updated {
            frame = if updated.size == size {
                updated
            } else {
                fit_capture_frame(&updated, size)
            };
        }
    }
}

fn fit_capture_frame(source: &egui::ColorImage, [width, height]: [usize; 2]) -> egui::ColorImage {
    let [source_width, source_height] = source.size;
    let fit_width = width.min((source_width * height / source_height).max(1));
    let fit_height = height.min((source_height * width / source_width).max(1));
    let left = (width - fit_width) / 2;
    let top = (height - fit_height) / 2;
    let mut output = egui::ColorImage::new([width, height], egui::Color32::BLACK);
    for y in 0..fit_height {
        let source_row = y * source_height / fit_height * source_width;
        let target = (top + y) * width + left;
        for x in 0..fit_width {
            output.pixels[target + x] = source.pixels[source_row + x * source_width / fit_width];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Color32, ColorImage};

    #[test]
    fn resized_capture_preserves_canvas_and_aspect_ratio() {
        let source = ColorImage::new([1, 2], Color32::RED);
        let result = fit_capture_frame(&source, [4, 4]);
        assert_eq!(result.size, [4, 4]);
        for row in result.pixels.chunks_exact(4) {
            assert_eq!(
                row,
                [Color32::BLACK, Color32::RED, Color32::RED, Color32::BLACK]
            );
        }
    }
}
