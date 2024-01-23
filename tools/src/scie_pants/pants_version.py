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
    version: Version
    pex_name: str | None
    python: str


def determine_tag_version(
    ptex: Ptex, pants_version: str, github_api_bearer_token: str | None = None
) -> ResolveInfo:
    version = Version(pants_version)
    pex_name, python = determine_pex_name_and_python_id(ptex, version, github_api_bearer_token)
    return ResolveInfo(
        version=version,
        pex_name=pex_name,
        python=python,
    )


def determine_latest_stable_version(
    ptex: Ptex, pants_config: Path, github_api_bearer_token: str | None = None
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

    return configure_version, determine_tag_version(ptex, pants_version, github_api_bearer_token)


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
        # If there's only one dot in version, specifically suggest adding the `.patch`
        suggestion = (
            "Pants version format not recognized. Please add `.<patch_version>` to the end of the version. "
            "For example: `2.18` -> `2.18.0`.\n\n"
            if version.base_version.count(".") < 2
            else ""
        )
        fatal(
            f"Unknown Pants release: {version}.\n\n{suggestion}"
            f"Check to see if the URL is reachable ( {GITHUB_API_BASE_URL} ) and if"
            f" an asset exists within the release for the platform {platform}."
            " If the asset doesn't exist it may be that this platform isn't yet supported."
            " If that's the case, please reach out on Slack: https://www.pantsbuild.org/community/getting-help#slack"
            " or file an issue on GitHub: https://github.com/pantsbuild/pants/issues/new/choose.\n\n"
        )
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
