"""Normalize GitHub release asset names and generate the updater manifest."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
tag = sys.argv[2]
if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
    raise SystemExit(f"Expected a release tag like v1.2.3, got {tag!r}")
repo = os.environ["GITHUB_REPOSITORY"]
version = tag[1:]
packages = (
    ("windows-x64-exe", "*-setup.exe", f"server-launcher-{version}-windows-x64-setup.exe"),
    ("windows-x64-msi", "*.msi", f"server-launcher-{version}-windows-x64.msi"),
    ("linux-x64-deb", "*.deb", f"server-launcher-{version}-linux-x64.deb"),
    ("linux-x64-rpm", "*.rpm", f"server-launcher-{version}-linux-x64.rpm"),
    ("linux-x64-appimage", "*.AppImage", f"server-launcher-{version}-linux-x64.AppImage"),
)

sources = []
for key, pattern, filename in packages:
    matches = sorted(root.rglob(pattern))
    if len(matches) != 1 or not matches[0].is_file() or not matches[0].stat().st_size:
        raise SystemExit(f"Expected exactly one nonempty {pattern} package")
    sources.append((key, matches[0], root / filename))

assets = {}
for key, source, path in sources:
    if source != path:
        if path.exists():
            raise SystemExit(f"Release asset already exists: {path}")
        source.rename(path)
    with path.open("rb") as stream:
        checksum = hashlib.file_digest(stream, "sha256").hexdigest()
    assets[key] = {
        "url": f"https://github.com/{repo}/releases/download/{tag}/{path.name}",
        "sha256": checksum,
        "size": path.stat().st_size,
    }

(root / "latest.json").write_text(
    json.dumps({"version": version, "platforms": assets}, indent=2) + "\n",
    encoding="utf-8",
)
