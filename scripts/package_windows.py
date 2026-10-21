#!/usr/bin/env python3
"""Package existing Windows build artifacts with the applications' Inno templates.

Requires Python 3.11+ and Inno Setup 6 on the packaging host. Neither application
is rebuilt or executed, and no application dependencies are imported.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
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


def package(variant: str, input_dir: Path, output_dir: Path, compiler: Path, *, abi: str | None = None) -> list[Path]:
    if variant == "rust":
        abi = abi or "msvc"
        if abi not in {"msvc", "gnu"}:
            raise ValueError(f"Unsupported Rust ABI: {abi}")
    elif abi is not None:
        raise ValueError("--abi applies only to Rust builds")
    input_dir = input_dir.resolve(strict=True)
    output_dir = output_dir.resolve()
    required = ["sysu_netauth.exe", "sysu_netauth_service.exe"]
    if variant == "rust":
        required.append("sysu_netauth_gui.exe")
    elif variant == "python":
        if not (input_dir / "_internal").is_dir():
            raise FileNotFoundError("Python artifact is missing its _internal runtime directory")
    else:
        raise ValueError(f"Unknown application variant: {variant}")
    for name in required:
        binary = input_dir / name
        if not binary.is_file() or binary.stat().st_size == 0:
            raise FileNotFoundError(f"Missing or empty Windows executable: {binary}")

    actual = json.loads((input_dir / "release-version.json").read_text(encoding="utf-8"))
    release_metadata = ROOT / "build/release-version.json"
    if release_metadata.is_file():
        expected = json.loads(release_metadata.read_text(encoding="utf-8"))
        if actual != expected:
            raise ValueError("Build artifact version/commit does not match this release checkout")

    generated_script = None
    if variant == "rust":
        app_root = ROOT / "apps/rust"
        version = actual["version"]
        script = app_root / "scripts/setup.template.iss"
        core = re.match(r"\d+\.\d+\.\d+", version)[0]
        defines = [f"/DMyAppVersion={version}", f"/DMyAppFileVersion={core}", f"/DMyAppAbi={abi}", f"/DAppSourceDir={input_dir}"]
    else:
        # Reuse the Python installer generator without running PyInstaller.
        build = runpy.run_path(str(ROOT / "apps/python/scripts/build.py"))
        config = build["BuildConfig"]()
        config.app_dir = input_dir
        version = build["VERSION"]
        if version != actual["python_version"]:
            raise ValueError("Generated Python version does not match the build artifact")
        script = build["generate_iss_file"](config)
        generated_script = script
        defines = []

    try:
        if not re.fullmatch(r"\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+|(?:a|b|rc)\d+)?", version):
            raise ValueError(f"Unsupported installer version: {version}")
        label = f"Rust_{abi}_x64" if variant == "rust" else "Python_x64"
        identifier = label.lower().replace("_", "-")
        basename = f"SYSUNetAuth_{label}_Setup_v{version}"
        output_dir.mkdir(parents=True, exist_ok=True)
        setup = output_dir / f"{basename}.exe"
        archive = output_dir / f"{basename}.zip"
        portable = output_dir / f"SYSUNetAuth_{label}_Portable_v{version}.zip"
        checksums = output_dir / f"SHA256SUMS-{identifier}.txt"
        # A compiler that fails or emits no setup must never reuse stale output.
        for path in (setup, archive, portable, checksums):
            path.unlink(missing_ok=True)
        subprocess.run(
            [str(compiler), f"/O{output_dir}", f"/F{basename}", *defines, str(script)],
            cwd=script.parent,
            check=True,
        )
        if not setup.is_file() or setup.stat().st_size == 0:
            raise FileNotFoundError(f"Inno Setup did not produce {setup}")
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            bundle.write(setup, setup.name)
        payload = [input_dir / name for name in required] if variant == "rust" else sorted(
            path for path in input_dir.rglob("*") if path.is_file()
        )
        with zipfile.ZipFile(portable, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for path in payload:
                bundle.write(path, (Path("sysu_netauth") / path.relative_to(input_dir)).as_posix())
        hashes = []
        for path in (setup, archive, portable):
            with path.open("rb") as stream:
                hashes.append(f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  {path.name}\n")
        checksums.write_text("".join(hashes), encoding="utf-8")
        return [setup, archive, portable, checksums]
    finally:
        if generated_script is not None:
            generated_script.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", required=True, choices=("rust", "python"))
    parser.add_argument("--abi", choices=("msvc", "gnu"), help="Rust compiler ABI (default: msvc)")
    parser.add_argument("--input-dir", required=True, type=Path, help="Directory containing the built EXEs")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist/windows-installers")
    parser.add_argument("--compiler", type=Path, help="Inno Setup compiler (auto-detected by default)")
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True) if args.compiler else find_compiler()
    for path in package(args.variant, args.input_dir, args.output_dir, compiler, abi=args.abi):
        print(path)


if __name__ == "__main__":
    main()
