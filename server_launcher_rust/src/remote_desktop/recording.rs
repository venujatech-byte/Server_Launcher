use super::vnc::Connection;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
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
    output_bytes: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
    pub(super) result: mpsc::Receiver<Result<PathBuf, String>>,
}

impl Recording {
    pub(super) fn start(
        connection: Arc<Mutex<Connection>>,
        ctx: egui::Context,
        folder: Option<PathBuf>,
        ffmpeg_path: Option<PathBuf>,
        fps: u32,
        format: crate::config::RemoteRecordingFormat,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let worker_paused = paused.clone();
        let written_us = Arc::new(AtomicU64::new(0));
        let progress = written_us.clone();
        let output_bytes = Arc::new(AtomicU64::new(0));
        let worker_bytes = output_bytes.clone();
        let (sender, result) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = sender.send(record(
                &connection,
                cancelled,
                &worker_paused,
                &progress,
                &worker_bytes,
                folder,
                ffmpeg_path,
                fps,
                format,
            ));
            ctx.request_repaint();
        });
        Self {
            stop,
            paused,
            written_us,
            output_bytes,
            worker: Some(worker),
            result,
        }
    }

    pub(super) fn duration(&self) -> Duration {
        Duration::from_micros(self.written_us.load(Ordering::Relaxed))
    }

    pub(super) fn output_bytes(&self) -> u64 {
        self.output_bytes.load(Ordering::Relaxed)
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
    stop: Arc<AtomicBool>,
    paused: &AtomicBool,
    progress: &AtomicU64,
    output_bytes: &AtomicU64,
    folder: Option<PathBuf>,
    ffmpeg_path: Option<PathBuf>,
    fps: u32,
    format: crate::config::RemoteRecordingFormat,
) -> Result<PathBuf, String> {
    let fps = normalize_fps(fps);
    let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
    let frame_micros = interval.as_micros() as u64;
    let deadline = Instant::now() + Duration::from_secs(8);
    let first_frame = loop {
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
                    break (frame, connection.framebuffer.revision);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err("No complete remote frame arrived in time.".into());
        }
        thread::sleep(Duration::from_millis(16));
    };
    let size = first_frame.0.size;
    let folder = recording_folder(folder)?;
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let prefix = format!("remote-{}-", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    if format == crate::config::RemoteRecordingFormat::PngSequence {
        return save_png_sequence(
            connection,
            &stop,
            paused,
            first_frame,
            &folder,
            &prefix,
            interval,
            progress,
            output_bytes,
        );
    }
    let Some((suffix, codec_args, container_args)) = ffmpeg_video_options(format) else {
        return Err("The selected recording format is not a video format.".into());
    };
    let fps_arg = fps.to_string();
    let output = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(suffix)
        .tempfile_in(&folder)
        .map_err(|error| error.to_string())?
        .into_temp_path();
    let resolution = format!("{}x{}", size[0], size[1]);
    let mut command = Command::new(
        ffmpeg_path
            .as_deref()
            .unwrap_or_else(|| Path::new("ffmpeg")),
    );
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
            &fps_arg,
            "-i",
            "pipe:0",
            "-an",
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        ])
        .args(codec_args)
        .args(["-pix_fmt", "yuv420p"])
        .args(container_args)
        .args(["-g", &fps_arg, "-flush_packets", "1"])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return save_png_sequence(
                connection,
                &stop,
                paused,
                first_frame,
                &folder,
                &prefix,
                interval,
                progress,
                output_bytes,
            );
        }
        Err(error) => return Err(format!("Could not start FFmpeg: {error}")),
    };
    let pipe = child.stdin.take().expect("FFmpeg stdin was piped");
    let (pipe_sender, pipe_receiver) = mpsc::sync_channel::<(u64, Arc<Vec<u8>>)>(1);
    let target_frames = Arc::new(AtomicU64::new(0));
    let writer_target_frames = Arc::clone(&target_frames);
    let (writer_result_sender, writer_result_receiver) = mpsc::channel();
    let (status_sender, status_receiver) = mpsc::channel();
    let child_stop = Arc::clone(&stop);
    thread::spawn(move || supervise_child(child, child_stop, status_sender));
    let pipe_writer = thread::spawn(move || {
        let mut pipe = pipe;
        let mut last_frame = None;
        let mut written_frames = 0;
        let result = (|| {
            while let Ok((frame_index, pixels)) = pipe_receiver.recv() {
                write_scheduled_frame(
                    &mut pipe,
                    &mut last_frame,
                    &mut written_frames,
                    frame_index,
                    pixels,
                )?;
            }
            if let Some(last_frame) = &last_frame {
                write_frame_repeats(
                    &mut pipe,
                    last_frame,
                    writer_target_frames
                        .load(Ordering::Relaxed)
                        .saturating_sub(written_frames),
                )?;
            }
            Ok::<_, std::io::Error>(())
        })();
        let _ = writer_result_sender.send(result);
    });
    let progress_target = Arc::clone(&target_frames);
    let mut pixels = Arc::new(Vec::with_capacity(size[0] * size[1] * 4));
    let mut frames = 0_u64;
    let write_result = sample_frames(
        connection,
        &stop,
        paused,
        first_frame,
        interval,
        |frame, changed| {
            if changed {
                let buffer = Arc::make_mut(&mut pixels);
                buffer.clear();
                buffer.extend(frame.pixels.iter().flat_map(|pixel| pixel.to_array()));
            }
            enqueue_recording_frame(
                &pipe_sender,
                Arc::clone(&pixels),
                &mut frames,
                frame_micros,
                &progress,
                &progress_target,
            )?;
            if frames % u64::from(fps) == 0 {
                if let Ok(metadata) = std::fs::metadata(&output) {
                    output_bytes.store(metadata.len(), Ordering::Relaxed);
                }
            }
            Ok(())
        },
    );
    stop.store(true, Ordering::Relaxed);
    drop(pipe_sender);
    let pipe_result = writer_result_receiver
        .recv_timeout(Duration::from_secs(4))
        .map_err(|_| "FFmpeg did not finish writing the recording.".to_owned())?
        .map_err(|error| format!("Could not write the recording: {error}"));
    let _ = pipe_writer.join();
    let status = status_receiver
        .recv_timeout(Duration::from_secs(4))
        .map_err(|_| "FFmpeg did not stop after recording ended.".to_owned())?
        .map_err(|error| error.to_string())?;
    write_result?;
    pipe_result?;
    if !status.success() {
        return Err(format!("FFmpeg could not encode the recording ({status})."));
    }
    output.keep().map_err(|error| error.to_string())
}

fn write_scheduled_frame(
    writer: &mut impl Write,
    last_frame: &mut Option<Arc<Vec<u8>>>,
    written_frames: &mut u64,
    frame_index: u64,
    pixels: Arc<Vec<u8>>,
) -> std::io::Result<()> {
    if frame_index <= *written_frames {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Recording frame indexes must increase.",
        ));
    }
    if let Some(last_frame) = last_frame {
        write_frame_repeats(writer, last_frame, frame_index - *written_frames - 1)?;
    }
    writer.write_all(&pixels)?;
    *written_frames = frame_index;
    *last_frame = Some(pixels);
    Ok(())
}

fn write_frame_repeats(
    writer: &mut impl Write,
    frame: &[u8],
    count: u64,
) -> std::io::Result<()> {
    for _ in 0..count {
        writer.write_all(frame)?;
    }
    Ok(())
}

fn enqueue_recording_frame(
    sender: &mpsc::SyncSender<(u64, Arc<Vec<u8>>)>,
    pixels: Arc<Vec<u8>>,
    frames: &mut u64,
    frame_micros: u64,
    progress: &AtomicU64,
    target_frames: &AtomicU64,
) -> Result<(), String> {
    *frames += 1;
    progress.store(*frames * frame_micros, Ordering::Relaxed);
    target_frames.store(*frames, Ordering::Relaxed);
    match sender.try_send((*frames, pixels)) {
        // The pipe writer fills dropped frame indexes with its last complete frame.
        Ok(()) | Err(mpsc::TrySendError::Full(_)) => Ok(()),
        Err(mpsc::TrySendError::Disconnected(_)) => {
            Err("FFmpeg stopped accepting recording frames.".into())
        }
    }
}

fn supervise_child(
    mut child: Child,
    stop: Arc<AtomicBool>,
    status: mpsc::Sender<std::io::Result<ExitStatus>>,
) {
    let mut stop_deadline = None;
    loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                let _ = status.send(Ok(exit));
                return;
            }
            Err(error) => {
                let _ = status.send(Err(error));
                return;
            }
            Ok(None) => {}
        }
        if stop.load(Ordering::Relaxed) {
            let deadline = stop_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
            if Instant::now() >= *deadline {
                let _ = child.kill();
                let _ = status.send(child.wait());
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn save_png_sequence(
    connection: &Mutex<Connection>,
    stop: &AtomicBool,
    paused: &AtomicBool,
    first_frame: (egui::ColorImage, u64),
    folder: &Path,
    prefix: &str,
    interval: Duration,
    progress: &AtomicU64,
    output_bytes: &AtomicU64,
) -> Result<PathBuf, String> {
    let sequence = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(folder)
        .map_err(|error| error.to_string())?;
    let mut index = 0_u64;
    let result = sample_frames(
        connection,
        stop,
        paused,
        first_frame,
        interval,
        |frame, _| {
            let path =
                super::screenshot::save_in(frame, sequence.path(), &format!("frame-{index:08}"))
                    .map_err(|error| error.to_string())?;
            if let Ok(metadata) = std::fs::metadata(path) {
                output_bytes.fetch_add(metadata.len(), Ordering::Relaxed);
            }
            index += 1;
            progress.store(index * interval.as_micros() as u64, Ordering::Relaxed);
            Ok(())
        },
    );
    if index != 0 {
        let path = sequence.keep();
        result
            .map_err(|error| format!("{error} Partial PNG sequence saved to {}", path.display()))?;
        return Ok(path);
    }
    Err(result
        .err()
        .unwrap_or_else(|| "No recording frames were saved.".into()))
}

fn recording_folder(folder: Option<PathBuf>) -> Result<PathBuf, String> {
    match folder {
        Some(folder) => Ok(folder),
        None => super::screenshot::pictures().map(|path| path.join("ServerLauncher")),
    }
}

fn ffmpeg_video_options(
    format: crate::config::RemoteRecordingFormat,
) -> Option<(
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
)> {
    match format {
        crate::config::RemoteRecordingFormat::Mp4 => Some((
            ".mp4",
            &["-c:v", "libx264", "-preset", "ultrafast"],
            &["-movflags", "+empty_moov+frag_keyframe+default_base_moof"],
        )),
        crate::config::RemoteRecordingFormat::Webm => Some((
            ".webm",
            &[
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "8",
                "-crf",
                "34",
                "-b:v",
                "0",
            ],
            &[],
        )),
        crate::config::RemoteRecordingFormat::PngSequence => None,
    }
}

fn normalize_fps(fps: u32) -> u32 {
    match fps {
        15 | 24 | 30 | 60 => fps,
        _ => 30,
    }
}

fn sample_frames(
    connection: &Mutex<Connection>,
    stop: &AtomicBool,
    paused: &AtomicBool,
    (mut frame, mut revision): (egui::ColorImage, u64),
    interval: Duration,
    mut write: impl FnMut(&egui::ColorImage, bool) -> Result<(), String>,
) -> Result<(), String> {
    let size = frame.size;
    let mut next = Instant::now();
    let mut changed = true;
    loop {
        if paused.load(Ordering::Relaxed) {
            if stop.load(Ordering::Relaxed) || !connection.lock().unwrap().connected {
                return Ok(());
            }
            thread::sleep(interval);
            next = Instant::now();
            continue;
        }
        write(&frame, changed)?;
        changed = false;
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
            if connection.receiving_frame || connection.framebuffer.revision == revision {
                None
            } else {
                revision = connection.framebuffer.revision;
                connection.framebuffer.snapshot()
            }
        };
        if let Some(updated) = updated {
            changed = true;
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

    #[cfg(unix)]
    #[test]
    fn stopped_encoder_is_killed_and_reaped_after_grace_period() {
        let child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let stop = Arc::new(AtomicBool::new(true));
        let (sender, receiver) = mpsc::channel();
        let started = Instant::now();

        supervise_child(child, stop, sender);

        let status = receiver.recv().unwrap().unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

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

    #[test]
    fn selected_folder_is_used_for_recordings() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            recording_folder(Some(folder.path().to_owned())).unwrap(),
            folder.path()
        );
    }

    #[test]
    fn recording_fps_is_limited_to_supported_presets() {
        assert_eq!(normalize_fps(15), 15);
        assert_eq!(normalize_fps(24), 24);
        assert_eq!(normalize_fps(30), 30);
        assert_eq!(normalize_fps(60), 60);
        assert_eq!(normalize_fps(240), 30);
    }

    #[test]
    fn recording_formats_select_the_matching_encoder_and_container() {
        let mp4 = ffmpeg_video_options(crate::config::RemoteRecordingFormat::Mp4).unwrap();
        assert_eq!(mp4.0, ".mp4");
        assert!(mp4.1.contains(&"libx264"));
        assert!(mp4.2.contains(&"-movflags"));

        let webm = ffmpeg_video_options(crate::config::RemoteRecordingFormat::Webm).unwrap();
        assert_eq!(webm.0, ".webm");
        assert!(webm.1.contains(&"libvpx-vp9"));
        assert!(webm.2.is_empty());
        assert!(ffmpeg_video_options(crate::config::RemoteRecordingFormat::PngSequence).is_none());
    }

    #[test]
    fn dropped_encoder_frames_repeat_last_frame_and_preserve_duration() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let progress = AtomicU64::new(0);
        let target_frames = AtomicU64::new(0);
        let mut frames = 0;

        enqueue_recording_frame(
            &sender,
            Arc::new(vec![1]),
            &mut frames,
            10,
            &progress,
            &target_frames,
        )
        .unwrap();
        enqueue_recording_frame(
            &sender,
            Arc::new(vec![2]),
            &mut frames,
            10,
            &progress,
            &target_frames,
        )
        .unwrap();
        let first = receiver.recv().unwrap();
        enqueue_recording_frame(
            &sender,
            Arc::new(vec![3]),
            &mut frames,
            10,
            &progress,
            &target_frames,
        )
        .unwrap();
        enqueue_recording_frame(
            &sender,
            Arc::new(vec![4]),
            &mut frames,
            10,
            &progress,
            &target_frames,
        )
        .unwrap();
        let third = receiver.recv().unwrap();

        let mut output = Vec::new();
        let mut last_frame = None;
        let mut written_frames = 0;
        write_scheduled_frame(
            &mut output,
            &mut last_frame,
            &mut written_frames,
            first.0,
            first.1,
        )
        .unwrap();
        write_scheduled_frame(
            &mut output,
            &mut last_frame,
            &mut written_frames,
            third.0,
            third.1,
        )
        .unwrap();
        write_frame_repeats(
            &mut output,
            last_frame.as_ref().unwrap(),
            target_frames.load(Ordering::Relaxed) - written_frames,
        )
        .unwrap();

        assert_eq!(frames, 4);
        assert_eq!(progress.load(Ordering::Relaxed), 40);
        assert_eq!(output, [1, 1, 3, 3]);
    }
}
