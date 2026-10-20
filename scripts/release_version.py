#!/usr/bin/env python3
"""Validate a release tag and generate SCM version/artifact metadata."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib
from packaging.version import Version
from setuptools_scm import get_version

ROOT = Path(__file__).resolve().parent.parent


def tag_version(tag: str) -> dict:
    parsed = Version(tag.removeprefix("v"))
    if len(parsed.release) != 3 or parsed.dev is not None or parsed.post is not None or parsed.local or parsed.epoch:
        raise ValueError("Use a three-component release tag, optionally with alpha.N/beta.N/rc.N")
    if any(part > 65535 for part in parsed.release):
        raise ValueError("Windows version components must not exceed 65535")
    core = ".".join(map(str, parsed.release))
    rust = core
    if parsed.pre:
        stage, serial = parsed.pre
        stage_name = {"a": "alpha", "b": "beta", "rc": "rc"}[stage]
        rust += f"-{stage_name}.{serial}"
    if tag != f"v{rust}":
        raise ValueError(f"Use the canonical Rust tag v{rust}")
    return {
        "tag": tag,
        "version": rust,
        "file_version": core,
        "python_version": str(parsed),
        "prerelease": parsed.is_prerelease,
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
    return info | {"commit": commit}


def stamp(root: Path, info: dict) -> None:
    # Standard SCM inference generates a constant module for PyInstaller. No
    # source constants, Cargo versions, lockfiles or update feeds are rewritten.
    python_version = generate_python(root)
    if python_version != info["python_version"]:
        raise ValueError(f"SCM version {python_version} does not match release {info['tag']}; use a clean tagged checkout")
    metadata = root / "build/release-version.json"
    metadata.parent.mkdir(parents=True, exist_ok=True)
    metadata.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")


def generate_python(root: Path = ROOT) -> str:
    project = root / "apps/python/pyproject.toml"
    config = tomllib.loads(project.read_text(encoding="utf-8"))["tool"]["setuptools_scm"]
    return get_version(relative_to=str(project), **config)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Defaults to the CI tag or the single v* tag at HEAD")
    parser.add_argument("--write", action="store_true", help="Generate Python SCM version and release artifact metadata")
    parser.add_argument("--generate-python", action="store_true", help="Generate the Python version module for a development build")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    if args.generate_python:
        print(generate_python())
        return
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
