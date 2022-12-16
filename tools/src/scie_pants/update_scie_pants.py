# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import argparse
import atexit
import hashlib
import io
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import sysconfig
import tempfile
from argparse import ArgumentParser
from dataclasses import dataclass
from pathlib import Path, PurePath
from subprocess import CalledProcessError
from typing import Any, Dict, List, NoReturn, cast

from packaging.version import Version

from scie_pants.log import fatal, info, init_logging, warn
from scie_pants.ptex import Ptex

log = logging.getLogger(__name__)


RELEASE_TAG_MATCHER = re.compile(r"^v(?P<version>\d+\.\d+\.\d+)$")
EXE_EXTENSION = sysconfig.get_config_var("EXE") or ""

# This type is not accurate, release data can contain bool values we test, namely "draft" and
# "prerelease" fields, but here Python truthiness lets things typecheck.
ReleaseData = Dict[str, Any]

GITHUB_API_BASE_URL = "https://api.github.com/repos/pantsbuild/scie-pants"
BINARY_NAME = "scie-pants"


@dataclass(frozen=True)
class Release:
    version: Version
    file_name: str
    binary_url: str
    binary_sha256_url: str

    @classmethod
    def from_api_response(
        cls, version: Version, platform: str, release_data: dict[str, Any]
    ) -> Release | None:
        binary_name = f"{BINARY_NAME}-{platform}{EXE_EXTENSION}"
        binary_sha256_name = f"{binary_name}.sha256"
        binary_url = None
        binary_sha256_url = None
        for asset in release_data.get("assets", []):
            name = asset.get("name")
            if binary_name == name:
                binary_url = asset.get("browser_download_url")
            elif binary_sha256_name == name:
                binary_sha256_url = asset.get("browser_download_url")
            if binary_url and binary_sha256_url:
                return cls(version, binary_name, binary_url, binary_sha256_url)
        log.debug(
            f"No release for {BINARY_NAME} {version} compatible with {platform} was found in: "
            f"{json.dumps(release_data, indent=2)}"
        )
        return None


class ReleaseNotFoundError(Exception):
    pass


def get_release(ptex: Ptex, platform: str, version: str) -> Release:
    try:
        release_data = cast(
            dict[str, Any], ptex.fetch_json(f"{GITHUB_API_BASE_URL}/releases/tags/v{version}")
        )
    except (CalledProcessError, OSError) as e:
        raise ReleaseNotFoundError(str(e))

    release = Release.from_api_response(Version(version), platform, release_data)
    if release is None:
        raise ReleaseNotFoundError(f"There were no compatible artifacts for {platform}.")

    return release


def find_latest_production_release(ptex: Ptex, platform: str) -> Release | None:
    releases = cast(list[dict[str, Any]], ptex.fetch_json(f"{GITHUB_API_BASE_URL}/releases"))

    latest_releases = []
    for release_data in releases:
        if release_data.get("draft") or release_data.get("prerelease"):
            continue
        tag_name = release_data.get("tag_name")
        if not tag_name:
            continue
        match = RELEASE_TAG_MATCHER.match(tag_name)
        if not match:
            log.debug(
                f"Skipping tag {tag_name} since it does not match {RELEASE_TAG_MATCHER.pattern}"
            )
            continue
        version = Version(match["version"])
        release = Release.from_api_response(version, platform, release_data)
        if release:
            latest_releases.append(release)

    if not latest_releases:
        log.debug(
            f"No releases for {BINARY_NAME} compatible with {platform} found in: "
            f"{json.dumps(releases, indent=2)}"
        )
        return None

    # We want the highest version; so this needs to be a reverse sort.
    latest_releases.sort(reverse=True, key=lambda rel: rel.version)
    return latest_releases[0]


def install_release(ptex: Ptex, release: Release, scie: Path) -> Path:
    # The `.sha256` checksum file format is a single line with two fields, space separated.
    # The 1st field is the hexadecimal checksum and the second field the name of the file it applies
    # to. See: https://man7.org/linux/man-pages/man1/sha256sum.1.html
    expected_sha256, _ = ptex.fetch_text(release.binary_sha256_url).strip().split(" ", maxsplit=1)

    download_dir = Path(tempfile.mkdtemp())
    atexit.register(shutil.rmtree, download_dir, ignore_errors=True)

    binary = download_dir / release.file_name
    with open(binary, "wb") as fp:
        ptex.fetch_to_fp(release.binary_url, fp)

    # TODO(John Sirois): Ideally this would not be 2-pass and we'd hash the download stream inline.
    digest = hashlib.sha256()
    with open(binary, "rb") as fp:
        for chunk in iter(lambda: fp.read(io.DEFAULT_BUFFER_SIZE), b""):
            digest.update(chunk)
    actual_sha256 = digest.hexdigest()
    if expected_sha256 != actual_sha256:
        eol = os.linesep
        raise ValueError(
            f"The binary downloaded from {release.binary_url} is invalid.{eol}"
            f"The expected fingerprint from {release.binary_sha256_url} was:{eol}"
            f"  {expected_sha256}{eol}"
            f"The actual fingerprint of the downloaded file is:{eol}"
            f"  {actual_sha256}",
        )

    # Mark the binary as executable. This is needed on Unix but not on Windows, where its harmless.
    binary.chmod(0o755)

    # We first move the running scie before replacing it. This satisfies Windows constraints
    # surrounding manipulation of running binaries.
    backup = scie.rename(scie.with_suffix(".bak"))
    try:
        shutil.move(str(binary), str(scie))
    except OSError:
        warn(f"A backup is saved in {backup}")
        raise
    return backup


def verify_release(scie: PurePath) -> str:
    return (
        subprocess.run(
            args=[str(scie)],
            env={**os.environ, "SCIE_BOOT": "version"},
            stdout=subprocess.PIPE,
            check=True,
        )
        .stdout.decode()
        .strip()
    )


def main() -> NoReturn:
    parser = ArgumentParser()
    get_ptex = Ptex.add_options(parser)
    parser.add_argument(
        "--platform",
        required=True,
        # The current platform tag (<OS>-<ARCH>)
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--base-dir",
        type=Path,
        required=True,
        # The base directory of this scie's bindings.
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--scie",
        type=Path,
        required=True,
        # The path of the current scie executable.
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--current-version",
        type=Version,
        required=True,
        # The version of the current scie executable.
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "version",
        nargs="?",
        type=str,
        default=None,
        help="The version of scie-pants to update to; defaults to the latest stable version",
    )
    options = parser.parse_args()

    # N.B.: This installs an excepthook that gracefully handles uncaught exceptions; so any raises
    # or uncaught exceptions below here are clean ways to exit non-zero with useful console output.
    init_logging(base_dir=options.base_dir, log_name="update")

    ptex = get_ptex(options)
    if options.version is not None:
        try:
            release = get_release(ptex, version=options.version, platform=options.platform)
        except ReleaseNotFoundError as e:
            fatal(f"Failed to find {BINARY_NAME} release for version {options.version}: {e}")
    else:
        maybe_release = find_latest_production_release(ptex, platform=options.platform)
        if not maybe_release or maybe_release.version < options.current_version:
            info("No new releases of scie-pants were found.")
            sys.exit(0)
        release = maybe_release

    scie = options.scie
    backup = install_release(ptex, release, scie)
    try:
        version = verify_release(scie)
    except (CalledProcessError, OSError):
        warn(f"Failed to verify scie-pants {release.version} installation at {scie}.")
        warn(f"A backup is saved in {backup}")
        raise

    if release.version != Version(version):
        warn(f"A backup is saved in {backup}")
        fatal(
            f"Installed scie-pants {release.version} to {scie} but the installation reports "
            f"version {version}."
        )

    info(f"Successfully installed sie=pants {version} to {scie}")
    try:
        backup.unlink(missing_ok=True)
    except OSError as e:
        warn(f"Failed to remove old version of scie-pants at {backup}: {e}")
    sys.exit(0)


if __name__ == "__main__":
    main()
