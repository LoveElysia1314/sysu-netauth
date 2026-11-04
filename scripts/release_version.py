#!/usr/bin/env python3
"""Validate a release tag and generate SCM version/artifact metadata."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def tag_version(tag: str) -> dict:
    number = r"(?:0|[1-9][0-9]*)"
    match = re.fullmatch(rf"v({number})\.({number})\.({number})(?:-(alpha|beta|rc)\.({number}))?", tag)
    if not match:
        raise ValueError("Use a canonical vMAJOR.MINOR.PATCH tag, optionally with alpha.N/beta.N/rc.N")
    if any(int(part) > 65535 for part in match.groups()[:3]):
        raise ValueError("Windows version components must not exceed 65535")
    core = ".".join(match.groups()[:3])
    return {
        "tag": tag,
        "version": tag[1:],
        "file_version": core,
        "prerelease": match.group(4) is not None,
    }



def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def resolve(root: Path = ROOT, tag: str | None = None) -> dict:
    tag = tag or os.environ.get("NETAUTH_RELEASE_TAG")
    if not tag and os.environ.get("GITHUB_REF_TYPE") == "tag":
        tag = os.environ.get("GITHUB_REF_NAME")
    if not tag:
        tags = git(root, "tag", "--points-at", "HEAD", "--list", "v*").splitlines()
        if len(tags) != 1:
            raise ValueError("Select exactly one release tag at HEAD; branch builds are not release builds")
        tag = tags[0]
    info = tag_version(tag)
    commit = git(root, "rev-parse", "HEAD")
    if git(root, "rev-parse", f"refs/tags/{tag}^{{commit}}") != commit:
        raise ValueError(f"Release tag {tag} does not point to the checked-out commit")
    package = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]
    if info["version"] != package["version"]:
        raise ValueError("Release tag must match Cargo.toml package version")
    return info | {"commit": commit}


def stamp(root: Path, info: dict) -> None:
    metadata = root / "build/release-version.json"
    metadata.parent.mkdir(parents=True, exist_ok=True)
    metadata.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Defaults to the CI tag or the single v* tag at HEAD")
    parser.add_argument("--write", action="store_true", help="Generate release artifact metadata")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    info = resolve(tag=args.tag)
    if args.write:
        stamp(ROOT, info)
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as stream:
            for key in ("tag", "version", "prerelease"):
                stream.write(f"{key}={str(info[key]).lower() if isinstance(info[key], bool) else info[key]}\n")
    print(json.dumps(info))


if __name__ == "__main__":
    main()
