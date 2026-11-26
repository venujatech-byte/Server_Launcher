use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) type SaveResult =
    Result<(PathBuf, Option<Result<arboard::Clipboard, arboard::Error>>), String>;

pub(crate) fn save_with_clipboard(
    frame: egui::ColorImage,
    host: &str,
    protocol: &str,
    copy: bool,
) -> SaveResult {
    let clipboard = copy.then(|| copy_frame(&frame));
    save(frame, host, protocol).map(|path| (path, clipboard))
}

fn copy_frame(frame: &egui::ColorImage) -> Result<arboard::Clipboard, arboard::Error> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_image(arboard::ImageData {
        width: frame.size[0],
        height: frame.size[1],
        bytes: frame
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect::<Vec<_>>()
            .into(),
    })?;
    Ok(clipboard)
}

pub(crate) fn save(frame: egui::ColorImage, host: &str, protocol: &str) -> Result<PathBuf, String> {
    let folder = pictures()?.join("ServerLauncher");
    let time = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let stem = format!(
        "{}_{}_{time}",
        safe_host(host),
        protocol.to_ascii_lowercase()
    );
    save_in(&frame, &folder, &stem).map_err(|error| format!("Could not save screenshot: {error}"))
}

pub(super) fn pictures() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Ok(output) = Command::new("xdg-user-dir").arg("PICTURES").output() {
        if output.status.success() {
            if let Ok(value) = std::str::from_utf8(&output.stdout) {
                let path = PathBuf::from(value.trim());
                if path.is_absolute() {
                    return Ok(path);
                }
            }
        }
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("Pictures"))
        .ok_or_else(|| "Could not find your Pictures folder.".into())
}

fn safe_host(host: &str) -> String {
    let name: String = host
        .chars()
        .take(64)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_matches('_');
    if name.is_empty() {
        "remote".into()
    } else {
        name.into()
    }
}

pub(super) fn save_in(frame: &egui::ColorImage, folder: &Path, stem: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let mut png = tempfile::NamedTempFile::new_in(folder)?;
    let rgb: Vec<u8> = frame
        .pixels
        .iter()
        .flat_map(|pixel| [pixel.r(), pixel.g(), pixel.b()])
        .collect();
    PngEncoder::new(png.as_file_mut())
        .write_image(
            &rgb,
            frame.size[0] as u32,
            frame.size[1] as u32,
            ExtendedColorType::Rgb8,
        )
        .map_err(io::Error::other)?;
    for index in 0..1000 {
        let filename = if index == 0 {
            format!("{stem}.png")
        } else {
            format!("{stem}-{index}.png")
        };
        let path = folder.join(filename);
        match png.persist_noclobber(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => png = error.file,
            Err(error) => return Err(error.error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Too many screenshots have the same timestamp.",
    ))
}

pub(crate) fn open(path: &Path) -> io::Result<()> {
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = Command::new(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_native_pixels_and_preserves_existing_captures() {
        let folder = tempfile::tempdir().unwrap();
        let sanitized = safe_host("../Lab/Room");
        assert_eq!(sanitized, "Lab_Room");
        let frame = egui::ColorImage {
            size: [2, 1],
            pixels: vec![egui::Color32::RED, egui::Color32::BLUE],
        };
        let first = save_in(&frame, folder.path(), &sanitized).unwrap();
        let next = save_in(&frame, folder.path(), &sanitized).unwrap();
        assert_ne!(first, next);
        let png = image::open(first).unwrap().to_rgb8();
        assert_eq!(png.dimensions(), (2, 1));
        assert_eq!(png.as_raw(), &[255, 0, 0, 0, 0, 255]);
    }
}
