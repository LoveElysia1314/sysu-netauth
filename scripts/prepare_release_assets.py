#!/usr/bin/env python3
"""Collect Linux binaries and verify the complete release asset set."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def expected_assets(info: dict) -> dict[str, list[str]]:
    groups = {}
    for label, version in (
        ("Rust_msvc_x64", info["version"]),
        ("Rust_gnu_x64", info["version"]),
        ("Python_x64", info["python_version"]),
    ):
        stem = f"SYSUNetAuth_{label}"
        identifier = label.lower().replace("_", "-")
        groups[f"SHA256SUMS-{identifier}.txt"] = [
            f"{stem}_Setup_v{version}.exe",
            f"{stem}_Setup_v{version}.zip",
            f"{stem}_Portable_v{version}.zip",
        ]
    groups["SHA256SUMS-rust-linux-x64.txt"] = [
        f"SYSUNetAuth_Rust_linux_x64_v{info['version']}.tar.gz"
    ]
    return groups


def prepare(metadata: Path, linux_input: Path, output: Path) -> list[Path]:
    info = json.loads(metadata.read_text(encoding="utf-8"))
    groups = expected_assets(info)
    with tarfile.open(linux_input, "r:gz") as archive:
        for name in ("sysu_netauth", "sysu_netauth_service", "sysu-netauth.service", "install-linux.sh", "release-version.json"):
            member = archive.getmember(f"sysu-netauth-linux-x64/{name}")
            if not member.isfile():
                raise ValueError(f"Linux archive requires a regular file: {name}")
        with archive.extractfile("sysu-netauth-linux-x64/release-version.json") as stream:
            if json.load(stream) != info:
                raise ValueError("Linux artifact version/commit does not match this release")

    output.mkdir(parents=True, exist_ok=True)
    linux_name = groups["SHA256SUMS-rust-linux-x64.txt"][0]
    linux_archive = output / linux_name
    shutil.copyfile(linux_input, linux_archive)
    (output / "SHA256SUMS-rust-linux-x64.txt").write_text(
        f"{digest(linux_archive)}  {linux_name}\n", encoding="utf-8"
    )

    expected = {name for checksum, names in groups.items() for name in [checksum, *names]}
    actual = {path.name for path in output.iterdir()}
    if actual != expected:
        raise ValueError(f"Incomplete release assets; missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}")
    for checksum, names in groups.items():
        entries = [line.split("  ", 1) for line in (output / checksum).read_text(encoding="utf-8").splitlines()]
        if len(entries) != len(names) or {name for _, name in entries} != set(names):
            raise ValueError(f"Invalid checksum manifest: {checksum}")
        for expected_digest, name in entries:
            if digest(output / name) != expected_digest:
                raise ValueError(f"Checksum mismatch: {name}")
    return [output / name for name in sorted(expected)]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--linux-input", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    for path in prepare(args.metadata, args.linux_input, args.output_dir):
        print(path.name)


if __name__ == "__main__":
    main()
