"""Generate the latest.json consumed by Settings > About."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys
from urllib.parse import quote

root = Path(sys.argv[1])
tag = sys.argv[2]
if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
    raise SystemExit(f"Expected a release tag like v1.2.3, got {tag!r}")
repo = os.environ["GITHUB_REPOSITORY"]
assets = {}
for key, pattern in {
    "windows-x64-exe": "*-setup.exe",
    "windows-x64-msi": "*.msi",
    "linux-x64-deb": "*.deb",
    "linux-x64-rpm": "*.rpm",
    "linux-x64-appimage": "*.AppImage",
}.items():
    matches = sorted(root.rglob(pattern))
    if len(matches) != 1:
        raise SystemExit(f"Expected exactly one {pattern}, found {len(matches)}")
    path = matches[0]
    with path.open("rb") as stream:
        checksum = hashlib.file_digest(stream, "sha256").hexdigest()
    assets[key] = {
        "url": f"https://github.com/{repo}/releases/download/{tag}/{quote(path.name)}",
        "sha256": checksum,
        "size": path.stat().st_size,
    }

(root / "latest.json").write_text(
    json.dumps({"version": tag[1:], "platforms": assets}, indent=2) + "\n",
    encoding="utf-8",
)
