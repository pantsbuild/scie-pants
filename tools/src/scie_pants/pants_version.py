# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import importlib.resources
import json
import logging
import os
import re
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from subprocess import CalledProcessError
from typing import Any, Callable, Iterator, cast
from xml.etree import ElementTree

import tomlkit
from packaging.specifiers import SpecifierSet
from packaging.version import Version

from scie_pants.log import fatal, info, warn
from scie_pants.ptex import Ptex

log = logging.getLogger(__name__)


@dataclass(frozen=True)
class ResolveInfo:
    stable_version: Version
    sha_version: Version | None
    find_links: str | None
    pex_name: str | None
    python: str

    def pants_find_links_option(self, pants_version_selected: Version) -> str:
        # We only want to add the find-links repo for PANTS_SHA invocations so that plugins can
        # resolve Pants the only place it can be found in that case - our ~private
        # binaries.pantsbuild.org S3 find-links bucket.
        operator = "-" if pants_version_selected == self.stable_version else "+"
        option_name = (
            "repos"
            if self.stable_version in SpecifierSet("<2.14.0", prereleases=True)
            else "find-links"
        )
        value = f"'{self.find_links}'" if self.find_links else ""

        # we usually pass a no-op, e.g. --python-repos-find-links=-[], because this is only used for
        # PANTS_SHA support that is now deprecated and will be removed
        return f"--python-repos-{option_name}={operator}[{value}]"


def determine_find_links(
    ptex: Ptex,
    pants_version: str,
    sha: str,
    find_links_dir: Path,
    include_nonrelease_pants_distributions_in_findlinks: bool,
) -> ResolveInfo:
    abbreviated_sha = sha[:8]
    sha_version = Version(f"{pants_version}+git{abbreviated_sha}")

    list_bucket_results = ElementTree.fromstring(
        ptex.fetch_text(f"https://binaries.pantsbuild.org?prefix=wheels/3rdparty/{sha}")
    )

    find_links_file = find_links_dir / pants_version / abbreviated_sha / "index.html"
    find_links_file.parent.mkdir(parents=True, exist_ok=True)
    find_links_file.unlink(missing_ok=True)
    with find_links_file.open("wb") as fp:
        # N.B.: S3 bucket listings use a default namespace. Although the URI is apparently stable,
        # we decouple from it with the wildcard.
        for key in list_bucket_results.findall("./{*}Contents/{*}Key"):
            bucket_path = str(key.text)
            fp.write(
                f'<a href="https://binaries.pantsbuild.org/{urllib.parse.quote(bucket_path)}">'
                f"{os.path.basename(bucket_path)}"
                f"</a>{os.linesep}".encode()
            )
        fp.flush()
        if include_nonrelease_pants_distributions_in_findlinks:
            pantsbuild_pants_find_links = (
                "https://binaries.pantsbuild.org/wheels/pantsbuild.pants/"
                f"{sha}/{urllib.parse.quote(str(sha_version))}/index.html"
            )
            ptex.fetch_to_fp(pantsbuild_pants_find_links, fp)
        fp.flush()

        ptex.fetch_to_fp("https://wheels.pantsbuild.org/simple/", fp)

    version = Version(pants_version)
    return ResolveInfo(
        stable_version=version,
        sha_version=sha_version,
        find_links=f"file://{find_links_file}",
        pex_name=None,
        python="cpython38" if version < Version("2.5") else "cpython39",
    )


def determine_tag_version(
    ptex: Ptex, pants_version: str, find_links_dir: Path, github_api_bearer_token: str | None = None
) -> ResolveInfo:
    stable_version = Version(pants_version)
    pex_name, python = determine_pex_name_and_python_id(
        ptex, stable_version, github_api_bearer_token
    )
    return ResolveInfo(
        stable_version,
        sha_version=None,
        find_links=None,
        pex_name=pex_name,
        python=python,
    )


def determine_sha_version(ptex: Ptex, sha: str, find_links_dir: Path) -> ResolveInfo:
    version_file_url = (
        f"https://raw.githubusercontent.com/pantsbuild/pants/{sha}/src/python/pants/VERSION"
    )
    pants_version = ptex.fetch_text(version_file_url).strip()
    return determine_find_links(
        ptex,
        pants_version,
        sha,
        find_links_dir,
        include_nonrelease_pants_distributions_in_findlinks=True,
    )


def determine_latest_stable_version(
    ptex: Ptex, pants_config: Path, find_links_dir: Path, github_api_bearer_token: str | None = None
) -> tuple[Callable[[], None], ResolveInfo]:
    info(f"Fetching latest stable Pants version since none is configured")

    try:
        latest_tag = ptex.fetch_json(
            "https://github.com/pantsbuild/pants/releases/latest", Accept="application/json"
        )["tag_name"]
    except Exception as e:
        fatal(
            "Couldn't get the latest release by fetching https://github.com/pantsbuild/pants/releases/latest.\n\n"
            + "If this is unexpected (e.g. GitHub isn't down), please reach out on Slack: https://www.pantsbuild.org/docs/getting-help#slack\n\n"
            + f"Exception:\n\n{e}"
        )

    prefix, _, pants_version = latest_tag.partition("_")
    if prefix != "release" or not pants_version:
        fatal(
            f'Expected the GitHub Release tagged "latest" to have the "release_" prefix. Got "{latest_tag}"\n\n'
            + "Please reach out on Slack: https://www.pantsbuild.org/docs/getting-help#slack or file"
            + " an issue on GitHub: https://github.com/pantsbuild/pants/issues/new/choose."
        )

    def configure_version():
        backup = None
        if pants_config.exists():
            info(f'Setting [GLOBAL] pants_version = "{pants_version}" in {pants_config}')
            config = tomlkit.loads(pants_config.read_text())
            backup = f"{pants_config}.bak"
        else:
            info(f"Creating {pants_config} and configuring it to use Pants {pants_version}")
            config = tomlkit.document()
        global_section = config.setdefault("GLOBAL", {})
        global_section["pants_version"] = pants_version
        if backup:
            warn(f"Backing up {pants_config} to {backup}")
            pants_config.replace(backup)
        pants_config.write_text(tomlkit.dumps(config))

    return configure_version, determine_tag_version(
        ptex, pants_version, find_links_dir, github_api_bearer_token
    )


PYTHON_IDS = {
    # N.B.: These values must match the lift TOML interpreter ids.
    "cp38": "cpython38",
    "cp39": "cpython39",
    "cp310": "cpython310",
    "cp311": "cpython311",
}


def determine_pex_name_and_python_id(
    ptex: Ptex, version: Version, github_api_bearer_token: str | None = None
) -> tuple[str, str]:
    uname = os.uname()
    platform = f"{uname.sysname.lower()}_{uname.machine.lower()}"
    res = get_release_asset_and_python_id(ptex, version, platform, github_api_bearer_token)
    if not res:
        fatal(f"Failed to get Python version to use for Pants {version} for platform {platform!r}.")
    pex_name, python = res
    if python not in PYTHON_IDS:
        fatal(f"This version of scie-pants does not support {python!r}.")
    return pex_name, PYTHON_IDS[python]


GITHUB_API_BASE_URL = "https://api.github.com/repos/pantsbuild/pants"


def fetch_github_api(ptex: Ptex, path: str, github_api_bearer_token: str | None = None) -> Any:
    headers = (
        {"Authorization": f"Bearer {github_api_bearer_token}"} if github_api_bearer_token else {}
    )
    return ptex.fetch_json(f"{GITHUB_API_BASE_URL}/{path}", **headers)


def get_release_asset_and_python_id(
    ptex: Ptex, version: Version, platform: str, github_api_bearer_token: str | None = None
) -> tuple[str, str] | None:
    try:
        release_data = cast(
            dict[str, Any],
            fetch_github_api(
                ptex,
                f"releases/tags/release_{version}",
                github_api_bearer_token=github_api_bearer_token,
            ),
        )
    except (CalledProcessError, OSError) as e:
        return None

    for asset in release_data.get("assets", []):
        name = asset.get("name")
        if name and (m := re.match(f"pants\\.{version}-([^-]+)-{platform}\\.pex", name)):
            return name, m.group(1)
    return None
