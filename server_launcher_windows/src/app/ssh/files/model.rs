#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub name: String,
    pub kind: Kind,
    pub modified: String,
    pub size: u64,
    pub mode: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Kind {
    Folder,
    File,
    Archive,
    Symlink,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::File => "File",
            Self::Archive => "Archive",
            Self::Symlink => "Symbolic link",
            Self::Other => "Special file",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}

pub(super) fn join(directory: &str, name: &str) -> String {
    format!("{}/{name}", directory.trim_end_matches('/'))
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\0'])
}

pub(super) fn archive_extension(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".tar", ".zip", ".rar",
    ]
    .into_iter()
    .find(|ext| name.ends_with(ext))
}

// GNU find's NUL records preserve spaces, quotes and embedded newlines.
pub(super) fn parse_listing(bytes: &[u8]) -> Result<Listing, String> {
    let data = std::str::from_utf8(bytes)
        .map_err(|_| "This directory contains names that are not valid UTF-8.")?;
    let data = data
        .strip_suffix('\0')
        .ok_or("Incomplete directory listing.")?;
    let mut fields = data.split('\0');
    let path = fields
        .next()
        .filter(|value| value.starts_with('/'))
        .ok_or("Invalid remote directory.")?;
    let fields: Vec<_> = fields.collect();
    let mut entries = Vec::new();
    for record in fields.chunks(5) {
        let [kind, modified, mode, size, name] = record else {
            return Err("Invalid directory entry.".into());
        };
        if !valid_name(name) {
            return Err("Invalid directory entry name.".into());
        }
        let kind = match *kind {
            "d" => Kind::Folder,
            "l" => Kind::Symlink,
            "f" if archive_extension(name).is_some() => Kind::Archive,
            "f" => Kind::File,
            _ => Kind::Other,
        };
        entries.push(Entry {
            name: (*name).to_owned(),
            kind,
            modified: (*modified).to_owned(),
            size: size.parse().map_err(|_| "Invalid file size.")?,
            mode: (*mode).to_owned(),
        });
    }
    Ok(Listing {
        path: path.to_owned(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_separated_names_and_rejects_truncated_records() {
        let listing = parse_listing(b"/tmp\0f\02026-01-01 12:00\0644\03\0a 'b\nc.zip\0").unwrap();
        assert_eq!(listing.entries[0].kind, Kind::Archive);
        assert_eq!(
            join(&listing.path, &listing.entries[0].name),
            "/tmp/a 'b\nc.zip"
        );
        assert!(parse_listing(b"/tmp\0f\0").is_err());
    }
}
