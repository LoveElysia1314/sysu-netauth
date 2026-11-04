#!/usr/bin/env python3
"""Collect and verify release assets for the selected support tiers."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile

PLATFORM_TIERS = {
    "windows-x64": 1,
    "windows-arm64": 2,
    "linux-x64": 2,
    "macos-arm64": 2,
    "macos-x64": 2,
}


def select_platforms(platforms: set[str] | None) -> set[str]:
    selected = set(PLATFORM_TIERS) if platforms is None else set(platforms)
    if not selected or selected - PLATFORM_TIERS.keys():
        raise ValueError(f"Invalid release platform selection: {sorted(selected)}")
    return selected


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def expected_assets(info: dict, *, platforms: set[str] | None = None) -> dict[str, list[str]]:
    selected = select_platforms(platforms)
    groups = {}
    for label, version in (
        ("windows_x64", info["version"]),
        ("windows_arm64", info["version"]),
    ):
        stem = f"SYSUNetAuth_{label}"
        identifier = label.lower().replace("_", "-")
        if identifier not in selected:
            continue
        groups[f"SHA256SUMS-{identifier}.txt"] = [
            f"{stem}_Setup_v{version}.exe",
        ]
    if "linux-x64" in selected:
        groups["SHA256SUMS-linux-x64.txt"] = [
            f"SYSUNetAuth_linux_x64_v{info['version']}.tar.gz"
        ]
    for arch in ("arm64", "x64"):
        if f"macos-{arch}" not in selected:
            continue
        groups[f"SHA256SUMS-macos-{arch}.txt"] = [
            f"SYSUNetAuth_macos_{arch}_v{info['version']}.pkg"
        ]
    return groups


def prepare(metadata: Path, linux_input: Path | None, output: Path, *, macos_inputs: dict[str, Path], macos_metadata: dict[str, Path] | None = None, platforms: set[str] | None = None) -> list[Path]:
    selected = select_platforms(platforms)
    info = json.loads(metadata.read_text(encoding="utf-8"))
    groups = expected_assets(info, platforms=selected)
    required_macos = {arch for arch in ("arm64", "x64") if f"macos-{arch}" in selected}
    if not required_macos <= macos_inputs.keys():
        raise ValueError(f"Missing macOS packages: {sorted(required_macos - macos_inputs.keys())}")
    macos_metadata = macos_metadata or {}
    for arch in sorted(required_macos):
        if arch not in macos_metadata:
            raise ValueError(f"Missing macOS installer metadata: {arch}")
        package = macos_inputs[arch]
        provenance = json.loads(macos_metadata[arch].read_text(encoding="utf-8"))
        with package.open("rb") as stream:
            if stream.read(4) != b"xar!":
                raise ValueError(f"macos-{arch} input is not a flat installer package")
        if provenance.get("release") != info or provenance.get("arch") != arch:
            raise ValueError(f"macos-{arch} artifact version/commit or architecture does not match this release")
        if provenance.get("sha256") != digest(package):
            raise ValueError(f"macos-{arch} installer checksum mismatch")
    unix_inputs = {}
    if "linux-x64" in selected:
        if linux_input is None:
            raise ValueError("Missing Linux service package")
        unix_inputs["linux-x64"] = linux_input
    for platform, path in unix_inputs.items():
        required = ("sysu_netauth", "sysu_netauth_service", "release-version.json", "sysu-netauth.service", "install-linux.sh")
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
    packages = unix_inputs | {f"macos-{arch}": macos_inputs[arch] for arch in sorted(required_macos)}
    for platform, path in packages.items():
        checksum = f"SHA256SUMS-{platform}.txt"
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
    parser.add_argument("--linux-input", type=Path)
    parser.add_argument("--macos-arm64-input", type=Path)
    parser.add_argument("--macos-x64-input", type=Path)
    parser.add_argument("--macos-arm64-metadata", type=Path)
    parser.add_argument("--macos-x64-metadata", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    macos_inputs = {arch: path for arch, path in (("arm64", args.macos_arm64_input), ("x64", args.macos_x64_input)) if path is not None}
    macos_metadata = {arch: path for arch, path in (("arm64", args.macos_arm64_metadata), ("x64", args.macos_x64_metadata)) if path is not None}
    for path in prepare(args.metadata, args.linux_input, args.output_dir,
                        macos_inputs=macos_inputs, macos_metadata=macos_metadata):
        print(path.name)


if __name__ == "__main__":
    main()
