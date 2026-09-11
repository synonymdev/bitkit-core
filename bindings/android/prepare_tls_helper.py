#!/usr/bin/env python3
"""Bundle the Android TLS helper matching the locked Rust dependency."""

import json
from pathlib import Path
import subprocess
import zipfile

root = Path(__file__).resolve().parents[2]
result = subprocess.run(
    ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-linux-android"],
    cwd=root,
    check=True,
    capture_output=True,
    text=True,
)
packages = json.loads(result.stdout)["packages"]
package = next(p for p in packages if p["name"] == "rustls-platform-verifier-android")
crate = Path(package["manifest_path"]).parent
version = package["version"]
aar = crate / "maven/rustls/rustls-platform-verifier" / version / f"rustls-platform-verifier-{version}.aar"
destination = root / "bindings/android/lib/libs/rustls-platform-verifier.jar"
destination.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(aar) as archive:
    destination.write_bytes(archive.read("classes.jar"))
