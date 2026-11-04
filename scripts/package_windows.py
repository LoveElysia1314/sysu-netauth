#!/usr/bin/env python3
"""Package the Windows binaries with the Inno Setup template.

Requires Python 3.11+ and Inno Setup 6 on the packaging host. Application binaries are
never rebuilt or executed by this tool.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import struct
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def find_compiler() -> Path:
    found = shutil.which("ISCC.exe")
    if found:
        return Path(found)
    for variable in ("ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"):
        base = os.environ.get(variable)
        if not base:
            continue
        directory = Path(base)
        if variable == "LOCALAPPDATA":
            directory /= "Programs"
        candidate = directory / "Inno Setup 6" / "ISCC.exe"
        if candidate.is_file():
            return candidate
    raise FileNotFoundError("Install Inno Setup 6 or pass --compiler PATH/ISCC.exe")


def verify_pe_architecture(path: Path, arch: str) -> None:
    """Reject mislabeled or mixed Rust artifacts before invoking the installer compiler."""
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError(f"Invalid Windows executable: {path}")
        stream.seek(struct.unpack_from("<I", header, 0x3c)[0])
        signature = stream.read(6)
    expected = {"x64": 0x8664, "arm64": 0xaa64}[arch]
    if len(signature) != 6 or signature[:4] != b"PE\0\0" or struct.unpack_from("<H", signature, 4)[0] != expected:
        raise ValueError(f"Windows executable does not match {arch}: {path}")


def package(input_dir: Path, output_dir: Path, compiler: Path, *, abi: str = "msvc", arch: str = "x64") -> list[Path]:
    if arch not in {"x64", "arm64"}:
        raise ValueError(f"Unsupported Windows architecture: {arch}")
    if abi not in {"msvc", "gnu"}:
        raise ValueError(f"Unsupported Rust ABI: {abi}")
    if arch == "arm64" and abi != "msvc":
        raise ValueError("Windows ARM64 requires the MSVC build")
    input_dir = input_dir.resolve(strict=True)
    output_dir = output_dir.resolve()
    required = ["sysu_netauth.exe", "sysu_netauth_service.exe", "sysu_netauth_gui.exe"]
    for name in required:
        binary = input_dir / name
        if not binary.is_file() or binary.stat().st_size == 0:
            raise FileNotFoundError(f"Missing or empty Windows executable: {binary}")
    for name in required:
        verify_pe_architecture(input_dir / name, arch)

    actual = json.loads((input_dir / "release-version.json").read_text(encoding="utf-8"))
    release_metadata = ROOT / "build/release-version.json"
    if release_metadata.is_file():
        expected = json.loads(release_metadata.read_text(encoding="utf-8"))
        if actual != expected:
            raise ValueError("Build artifact version/commit does not match this release checkout")

    version = actual["version"]
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-(?:alpha|beta|rc)\.(?:0|[1-9][0-9]*))?", version):
        raise ValueError(f"Unsupported installer version: {version}")
    core = version.split("-", 1)[0]
    if any(int(part) > 65535 for part in core.split(".")):
        raise ValueError("Windows version components must not exceed 65535")
    script = ROOT / "scripts/setup.template.iss"
    defines = [f"/DMyAppVersion={version}", f"/DMyAppFileVersion={core}", f"/DMyAppAbi={abi}", f"/DMyAppArch={arch}", f"/DAppSourceDir={input_dir}"]
    label = f"Rust_{abi}_{arch}"
    identifier = label.lower().replace("_", "-")
    basename = f"SYSUNetAuth_{label}_Setup_v{version}"
    output_dir.mkdir(parents=True, exist_ok=True)
    setup = output_dir / f"{basename}.exe"
    obsolete_archive = output_dir / f"{basename}.zip"
    portable = output_dir / f"SYSUNetAuth_{label}_Portable_v{version}.zip"
    checksums = output_dir / f"SHA256SUMS-{identifier}.txt"
    # A compiler that fails or emits no setup must never reuse stale output.
    for path in (setup, obsolete_archive, portable, checksums):
        path.unlink(missing_ok=True)
    subprocess.run(
        [str(compiler), f"/O{output_dir}", f"/F{basename}", *defines, str(script)],
        cwd=script.parent,
        check=True,
    )
    if not setup.is_file() or setup.stat().st_size == 0:
        raise FileNotFoundError(f"Inno Setup did not produce {setup}")
    payload = [input_dir / name for name in required]
    with zipfile.ZipFile(portable, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in payload:
            bundle.write(path, (Path("sysu_netauth") / path.relative_to(input_dir)).as_posix())
    hashes = []
    for path in (setup, portable):
        with path.open("rb") as stream:
            hashes.append(f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  {path.name}\n")
    checksums.write_text("".join(hashes), encoding="utf-8")
    return [setup, portable, checksums]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", choices=("msvc", "gnu"), default="msvc", help="Rust compiler ABI (default: msvc)")
    parser.add_argument("--arch", choices=("x64", "arm64"), default="x64", help="Windows CPU architecture (default: x64)")
    parser.add_argument("--input-dir", required=True, type=Path, help="Directory containing the built EXEs")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist/windows-installers")
    parser.add_argument("--compiler", type=Path, help="Inno Setup compiler (auto-detected by default)")
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True) if args.compiler else find_compiler()
    for path in package(args.input_dir, args.output_dir, compiler, abi=args.abi, arch=args.arch):
        print(path)


if __name__ == "__main__":
    main()
