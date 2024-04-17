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

PANTS_2 = Version("2.0.0")


@dataclass(frozen=True)
class ResolveInfo:
    stable_version: Version
    find_links: str | None


def determine_find_links(
    ptex: Ptex,
    pants_version: str,
    sha: str,
    find_links_dir: Path,
) -> ResolveInfo:
    list_bucket_results = ElementTree.fromstring(
        ptex.fetch_text(f"https://binaries.pantsbuild.org?prefix=wheels/3rdparty/{sha}")
    )

    abbreviated_sha = sha[:8]
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
        ptex.fetch_to_fp("https://wheels.pantsbuild.org/simple/", fp)

    return ResolveInfo(
        stable_version=Version(pants_version),
        find_links=f"file://{find_links_file}",
    )


def determine_tag_version(
    ptex: Ptex, pants_version: str, find_links_dir: Path, github_api_bearer_token: str | None = None
) -> ResolveInfo:
    stable_version = Version(pants_version)
    if stable_version >= PANTS_PEX_GITHUB_RELEASE_VERSION:
        return ResolveInfo(stable_version, find_links=None)

    tag = f"release_{pants_version}"

    # N.B.: The tag database was created with the following in a Pants clone:
    # git tag --list release_* | \
    #   xargs -I@ bash -c 'jq --arg T @ --arg C $(git rev-parse @^{commit}) -n "{(\$T): \$C}"' | \
    #   jq -s 'add' > pants_release_tags.json
    tags = json.loads(importlib.resources.read_text("scie_pants", "pants_release_tags.json"))
    commit_sha = tags.get(tag, "")

    if not commit_sha:
        mapping_file_url = f"https://binaries.pantsbuild.org/tags/pantsbuild.pants/{tag}"
        log.debug(
            f"Failed to look up the commit for Pants {tag} in the local database, trying a lookup "
            f"at {mapping_file_url} next."
        )
        try:
            commit_sha = ptex.fetch_text(mapping_file_url).strip()
        except CalledProcessError as e:
            log.debug(
                f"Failed to look up the commit for Pants {tag} at binaries.pantsbuild.org, trying "
                f"GitHub API requests next: {e}"
            )

    # The GitHub API requests are rate limited to 60 per hour un-authenticated; so we guard
    # these with the database of old releases and then the binaries.pantsbuild.org lookups above.
    if not commit_sha:
        github_api_url = (
            f"https://api.github.com/repos/pantsbuild/pants/git/refs/tags/{urllib.parse.quote(tag)}"
        )
        headers = (
            {"Authorization": f"Bearer {github_api_bearer_token}"}
            if github_api_bearer_token
            else {}
        )
        github_api_tag_url = ptex.fetch_json(github_api_url, **headers)["object"]["url"]
        commit_sha = ptex.fetch_json(github_api_tag_url, **headers)["object"]["sha"]

    if not commit_sha:
        fatal(f"Unknown Pants release: {pants_version}")

    return determine_find_links(
        ptex,
        pants_version,
        commit_sha,
        find_links_dir,
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
