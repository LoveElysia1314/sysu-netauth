#!/usr/bin/env python3
"""Collect Unix binaries and verify the complete release asset set."""

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
            f"{stem}_Portable_v{version}.zip",
        ]
    groups["SHA256SUMS-rust-linux-x64.txt"] = [
        f"SYSUNetAuth_Rust_linux_x64_v{info['version']}.tar.gz"
    ]
    for arch in ("arm64", "x64"):
        groups[f"SHA256SUMS-rust-macos-{arch}.txt"] = [
            f"SYSUNetAuth_Rust_macos_{arch}_v{info['version']}.tar.gz"
        ]
    return groups


def prepare(metadata: Path, linux_input: Path, output: Path, *, macos_inputs: dict[str, Path]) -> list[Path]:
    info = json.loads(metadata.read_text(encoding="utf-8"))
    groups = expected_assets(info)
    if set(macos_inputs) != {"arm64", "x64"}:
        raise ValueError("macOS release requires both arm64 and x64 archives")
    unix_inputs = {"linux-x64": linux_input} | {f"macos-{arch}": path for arch, path in macos_inputs.items()}
    for platform, path in unix_inputs.items():
        required = ("sysu_netauth", "sysu_netauth_service", "release-version.json")
        if platform == "linux-x64":
            required += ("sysu-netauth.service", "install-linux.sh")
        else:
            required += ("io.github.peterrobot.sysu-netauth.plist", "install-macos.sh", "uninstall-macos.sh", "launchd-common.sh")
        with tarfile.open(path, "r:gz") as archive:
            prefix = f"sysu-netauth-{platform}"
            for name in required:
                member = archive.getmember(f"{prefix}/{name}")
                if not member.isfile() or member.size == 0:
                    raise ValueError(f"{platform} archive requires a nonempty regular file: {name}")
            with archive.extractfile(f"{prefix}/release-version.json") as stream:
                if json.load(stream) != info:
                    raise ValueError(f"{platform} artifact version/commit does not match this release")
    output.mkdir(parents=True, exist_ok=True)
    for platform, path in unix_inputs.items():
        checksum = f"SHA256SUMS-rust-{platform}.txt"
        name = groups[checksum][0]
        destination = output / name
        shutil.copyfile(path, destination)
        (output / checksum).write_text(f"{digest(destination)}  {name}\n", encoding="utf-8")

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
    parser.add_argument("--macos-arm64-input", type=Path, required=True)
    parser.add_argument("--macos-x64-input", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    for path in prepare(args.metadata, args.linux_input, args.output_dir,
                        macos_inputs={"arm64": args.macos_arm64_input, "x64": args.macos_x64_input}):
        print(path.name)


if __name__ == "__main__":
    main()
