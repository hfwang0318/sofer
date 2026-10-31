#!/usr/bin/env python3
"""Fetch the pinned llama.cpp runtime into src-tauri/runtime/<platform>.

This is a build/reproducibility helper, not a user-facing installer. The normal
desktop app downloads only the model; the runtime is bundled in the installer.

    python3 scripts/fetch-runtime.py            # current platform
    python3 scripts/fetch-runtime.py --platform macos-arm64
    python3 scripts/fetch-runtime.py --platform windows-x64
"""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "src-tauri" / "runtime"
TAG = "b12018"
BASE = f"https://github.com/ggml-org/llama.cpp/releases/download/{TAG}"
MAC_ASSET = f"llama-{TAG}-bin-macos-arm64.tar.gz"
WIN_ASSET = f"llama-{TAG}-bin-win-cpu-x64.zip"
MAC_URL = f"{BASE}/{MAC_ASSET}"
WIN_URL = f"{BASE}/{WIN_ASSET}"

EXPECTED_MAC_LIBS = [
    "libllama.0.dylib",
    "libllama-common.0.dylib",
    "libllama-server-impl.dylib",
    "libmtmd.0.dylib",
    "libggml.0.dylib",
    "libggml-base.0.dylib",
    "libggml-cpu.0.dylib",
    "libggml-metal.0.dylib",
    "libggml-blas.0.dylib",
    "libggml-rpc.0.dylib",
]
EXPECTED_WIN_DLLS = [
    "llama-server-impl.dll",
    "llama-common.dll",
    "llama.dll",
    "ggml.dll",
    "ggml-base.dll",
    "ggml-cpu-x64.dll",
    "mtmd.dll",
    "libomp.dll",
    "ggml-rpc.dll",
]


def download(url: str, dest: Path) -> None:
    print(f"downloading {url}")
    with urllib.request.urlopen(url, timeout=120) as response, dest.open("wb") as out:
        shutil.copyfileobj(response, out)


def write_version(dest: Path, platform_name: str, asset_url: str) -> None:
    (dest / "VERSION").write_text(
        f"llama.cpp {TAG} (v0.8.0-dev)\n{platform_name}\n{asset_url}\n",
        encoding="utf-8",
    )


def fetch_macos() -> None:
    dest = RUNTIME / "macos-arm64"
    dest.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        archive = tmp / MAC_ASSET
        download(MAC_URL, archive)
        with tarfile.open(archive) as tar:
            tar.extractall(tmp / "extract")
        source = next(p for p in (tmp / "extract").iterdir() if p.is_dir())
        shutil.copy2(source / "llama-server", dest / "llama-server")
        # The release bundles dylibs with @loader_path rpaths. Copy exactly the
        # closure llama-server needs, under the names the loader asks for.
        needed: set[str] = set()
        pending = ["llama-server"]
        while pending:
            name = pending.pop()
            path = source / name
            output = subprocess.check_output(
                ["otool", "-L", str(path)], text=True, stderr=subprocess.DEVNULL
            )
            for line in output.splitlines()[1:]:
                dep = line.strip().split(" ")[0]
                if dep.startswith("@rpath/"):
                    dep_name = dep[len("@rpath/"):]
                    if dep_name not in needed:
                        needed.add(dep_name)
                        pending.append(dep_name)
        for name in sorted(needed):
            real = Path(os.path.realpath(source / name))
            shutil.copy2(real, dest / name)
        missing = [lib for lib in EXPECTED_MAC_LIBS if not (dest / lib).exists()]
        if missing:
            raise SystemExit(f"missing macOS runtime libs: {missing}")
        shutil.copy2(source / "LICENSE", dest / "LICENSE")
        write_version(dest, "macos-arm64", MAC_URL)
        os.chmod(dest / "llama-server", 0o755)
        subprocess.check_call([str(dest / "llama-server"), "--version"])


def fetch_windows() -> None:
    dest = RUNTIME / "windows-x64"
    dest.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        archive = tmp / WIN_ASSET
        download(WIN_URL, archive)
        with zipfile.ZipFile(archive) as zip_file:
            zip_file.extractall(tmp / "extract")
        source = tmp / "extract"
        for path in source.iterdir():
            if path.suffix.lower() == ".dll" or path.name == "llama-server.exe":
                shutil.copy2(path, dest / path.name)
        shutil.copy2(ROOT / "scripts" / "llama.cpp.LICENSE", dest / "LICENSE")
        for name in ["LICENSE-LLVM-OpenMP"]:
            license_path = source / name
            if license_path.exists():
                shutil.copy2(license_path, dest / name)
        missing = [dll for dll in EXPECTED_WIN_DLLS if not (dest / dll).exists()]
        if missing:
            raise SystemExit(f"missing Windows runtime DLLs: {missing}")
        write_version(dest, "windows-x64 (CPU)", WIN_URL)


def detect_platform() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "darwin" and machine in {"arm64", "aarch64"}:
        return "macos-arm64"
    if system == "windows" and machine in {"amd64", "x86_64"}:
        return "windows-x64"
    return "unsupported"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--platform",
        choices=["macos-arm64", "windows-x64", "auto"],
        default="auto",
    )
    args = parser.parse_args()
    target = detect_platform() if args.platform == "auto" else args.platform
    if target == "unsupported":
        raise SystemExit(
            "this build only vendors macOS arm64 and Windows x64; "
            "add another asset if you need a different target"
        )
    print(f"target: {target}")
    if target == "macos-arm64":
        if platform.system() != "Darwin":
            raise SystemExit("the macOS runtime must be assembled on macOS (needs otool)")
        fetch_macos()
    else:
        fetch_windows()
    print(f"done: {RUNTIME / target}")


if __name__ == "__main__":
    main()
