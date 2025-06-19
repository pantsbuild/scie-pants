# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import importlib.resources
import json
import os
import re
import urllib.parse
import urllib.request
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from subprocess import CalledProcessError
from typing import Any, Callable, Iterator, cast
from xml.etree import ElementTree

import tomlkit
from packaging.specifiers import SpecifierSet
from packaging.version import Version

from scie_pants.log import debug, fatal, info, warn
from scie_pants.ptex import Ptex

TIMEOUT = int(os.getenv("PANTS_BOOTSTRAP_URL_REQUEST_TIMEOUT_SECONDS", "10"))
PANTS_PEX_GITHUB_RELEASE_VERSION = Version("2.0.0.dev0")
PANTS_PYTHON_VERSIONS = [
    # Sorted on pants version in descending order. Add a new entry when the python version for a
    # particular pants version changes.
    {"pants": "2.25.0.dev0", "python": "cp311"},
    {"pants": "2.5.0.dev0", "python": "cp39"},
    {"pants": "2.0.0.dev0", "python": "cp38"},
]
PYTHON_IDS = {
    # N.B.: These values must match the lift TOML interpreter ids.
    # Important: all pythons used in pants_python_versions.json must be represented in this list.
    "cp313": "cpython313",
    "cp312": "cpython312",
    "cp311": "cpython311",
    "cp310": "cpython310",
    "cp39": "cpython39",
    "cp38": "cpython38",
}


@dataclass(frozen=True)
class ResolveInfo:
    version: Version
    python: str
    find_links: str | None = None
    pex_url: str | None = None


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
        version=Version(pants_version),
        python="cpython38",  # We only get here for pants versions < 2.0
        find_links=f"file://{find_links_file}",
    )


def determine_tag_version(
    ptex: Ptex,
    pants_version: str,
    find_links_dir: Path,
    github_api_bearer_token: str | None,
    bootstrap_urls_paths: Iterable[str],
) -> ResolveInfo:
    version = Version(pants_version)
    if version.base_version.count(".") < 2:
        fatal(
            f"Pants version must be a full version, including patch level, got: `{version}`.\n"
            "Please add `.<patch_version>` to the end of the version. "
            "For example: `2.18` -> `2.18.0`."
        )

    if version >= PANTS_PEX_GITHUB_RELEASE_VERSION:
        pex_url, python = determine_pex_url_and_python_id(ptex, version, bootstrap_urls_paths)
        return ResolveInfo(version=version, python=python, pex_url=pex_url)

    tag = f"release_{pants_version}"

    # N.B.: The tag database was created with the following in a Pants clone:
    # git tag --list release_* | \
    #   xargs -I@ bash -c 'jq --arg T @ --arg C $(git rev-parse @^{commit}) -n "{(\$T): \$C}"' | \
    #   jq -s 'add' > pants_release_tags.json
    tags = json.loads(importlib.resources.read_text("scie_pants", "pants_release_tags.json"))
    commit_sha = tags.get(tag, "")

    if not commit_sha:
        mapping_file_url = f"https://binaries.pantsbuild.org/tags/pantsbuild.pants/{tag}"
        debug(
            f"Failed to look up the commit for Pants {tag} in the local database, trying a lookup "
            f"at {mapping_file_url} next."
        )
        try:
            commit_sha = ptex.fetch_text(mapping_file_url).strip()
        except CalledProcessError as e:
            debug(
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
    ptex: Ptex,
    pants_config: Path,
    find_links_dir: Path,
    github_api_bearer_token: str | None,
    bootstrap_urls_paths: Iterable[str],
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
        ptex, pants_version, find_links_dir, github_api_bearer_token, bootstrap_urls_paths
    )


def determine_pex_url_and_python_id(
    ptex: Ptex,
    version: Version,
    bootstrap_urls_paths: Iterable[str],
) -> tuple[str, str]:
    uname = os.uname()
    platform = f"{uname.sysname.lower()}_{uname.machine.lower()}"
    pex_url, python = get_pex_url_and_python_id(ptex, version, platform, bootstrap_urls_paths)
    if python not in PYTHON_IDS:
        # Should not happen... but if we mess up, this is a nicer error message rather than blowing up.
        fatal(f"This version of scie-pants does not support {python!r}.")
    return pex_url, PYTHON_IDS[python]


def get_bootstrap_urls(bootstrap_urls_paths: Iterable[str]) -> dict[str, str] | None:
    if not bootstrap_urls_paths:
        return None

    ptex_urls: dict[str, str] = {}
    for bootstrap_urls_path in bootstrap_urls_paths:
        bootstrap_urls = json.loads(Path(bootstrap_urls_path).read_text())
        candidate_ptex_urls = bootstrap_urls.get("ptex")
        if candidate_ptex_urls is None:
            raise ValueError(
                f"Missing 'ptex' key in PANTS_BOOTSTRAP_URLS file: {bootstrap_urls_path}"
            )
        for key, url in candidate_ptex_urls.items():
            if not isinstance(url, str):
                raise TypeError(
                    f"The value for the key '{key}' in PANTS_BOOTSTRAP_URLS file: '{bootstrap_urls_path}' "
                    f"under the 'ptex' key was expected to be a string. Got a {type(url).__name__}"
                )
        ptex_urls.update(candidate_ptex_urls)

    return ptex_urls


def get_pex_url_and_python_id(
    ptex: Ptex,
    version: Version,
    platform: str,
    bootstrap_urls_paths: Iterable[str],
) -> tuple[str, str]:
    ptex_urls = get_bootstrap_urls(bootstrap_urls_paths)
    py = get_python_id_for_pants_version(version)
    error: str | None = None
    if py:
        pex_url, error = get_download_url(version, platform, py, ptex_urls)
        if pex_url:
            return pex_url, py

    # Else, try all known Pythons...
    for maybe_py in PYTHON_IDS.keys():
        pex_url, err = get_download_url(version, platform, maybe_py, ptex_urls)
        if pex_url:
            return pex_url, maybe_py
        elif not error:
            error = err

    fatal(
        f"Failed to determine release URL for Pants: {version}: {error or 'unknown reason'}\n\n"
        "If this is unexpected (you are using a known good Pants version), try upgrading scie-pants first.\n"
        f"It may also be that the platform {platform} isn't supported for this version of Pants, or some other intermittent network/service issue.\n"
        "To get help, please visit: https://www.pantsbuild.org/community/getting-help\n\n"
    )


def get_python_id_for_pants_version(version: Version) -> str | None:
    for version_break in PANTS_PYTHON_VERSIONS:
        # We pick the first version that is less than or equal to the one we're looking for. The
        # list of pants versions must therefore be sorted in descending order.
        if Version(version_break["pants"]) <= version:
            return version_break["python"]

    return None


def get_download_url(
    version: Version, platform: str, python: str, ptex_urls: dict[str, str] | None
) -> tuple[str, None] | tuple[None, str]:
    pex_name = f"pants.{version}-{python}-{platform}.pex"
    if ptex_urls:
        pex_url = ptex_urls.get(pex_name)
        if not pex_url:
            return None, f"{pex_name}: has no URL in PANTS_BOOTSTRAP_URLS file."
    else:
        pex_url = (
            f"https://github.com/pantsbuild/pants/releases/download/release_{version}/{pex_name}"
        )
    req = urllib.request.Request(pex_url, method="HEAD")
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as rsp:
            if rsp.status == 200:
                return pex_url, None
            elif (
                ptex_urls
                and rsp.status is None
                and pex_url.startswith("file://")
                and rsp.headers.get("Content-Length")
            ):
                return pex_url, None
            else:
                return None, f"{pex_name}: URL check failed: {pex_url}: {rsp.status}"
    except Exception as e:
        debug(f"{pex_name}: URL check failed for {pex_url}: {e}")
        if ptex_urls:
            return None, f"{pex_name}: URL check failed, from PANTS_BOOTSTRAP_URLS: {pex_url}: {e}"
        else:
            return None, f"{pex_name}: URL check failed: {pex_url}: {e}"
