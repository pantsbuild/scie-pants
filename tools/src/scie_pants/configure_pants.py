# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from argparse import ArgumentParser
from pathlib import Path
from typing import NoReturn

from packaging.version import Version

from scie_pants.log import fatal, info, init_logging, warn
from scie_pants.pants_version import (
    determine_latest_stable_version,
    determine_sha_version,
    determine_tag_version,
)
from scie_pants.ptex import Ptex


def prompt(message: str, default: bool) -> bool:
    raw_answer = input(f"{message} ({'Y/n' if default else 'N/y'}): ")
    answer = raw_answer.strip().lower()
    if not answer:
        return default
    return answer in ("y", "yes")


def prompt_for_pants_version(pants_config: Path) -> bool:
    warn(
        f"The `pants.toml` at {pants_config} has no `pants_version` configured in the `GLOBAL` "
        f"section."
    )
    return prompt(f"Would you like set `pants_version` to the latest stable release?", default=True)


def prompt_for_pants_config() -> Path | None:
    cwd = os.getcwd()
    buildroot = Path(cwd)
    if shutil.which("git"):
        result = subprocess.run(
            args=["git", "rev-parse", "--show-toplevel"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        if result.returncode == 0:
            buildroot = Path(os.fsdecode(result.stdout.strip()))

    info(f"No Pants configuration was found at or above {cwd}.")
    if prompt(f"Would you like to configure {buildroot} as a Pants project?", default=True):
        return buildroot / "pants.toml"
    return None


def main() -> NoReturn:
    parser = ArgumentParser()
    get_ptex = Ptex.add_options(parser)
    parser.add_argument("--pants-sha", help="The Pants sha to install (trumps --version)")
    parser.add_argument("--pants-version", help="The Pants version to install")
    parser.add_argument("--pants-config", help="The path of the pants.toml file")
    parser.add_argument(
        "--github-api-bearer-token", help="The GITHUB_TOKEN to use if running in CI context."
    )
    parser.add_argument("base_dir", nargs=1, help="The base directory to create Pants venvs in.")
    options = parser.parse_args()

    base_dir = Path(options.base_dir[0])
    init_logging(base_dir=base_dir, log_name="configure")

    env_file = os.environ.get("SCIE_BINDING_ENV")
    if not env_file:
        fatal("Expected SCIE_BINDING_ENV to be set in the environment")

    ptex = get_ptex(options)

    find_links_dir = base_dir / "find_links"

    finalizers = []
    newly_created_build_root = None
    pants_config = Path(options.pants_config) if options.pants_config else None
    if options.pants_sha:
        resolve_info = determine_sha_version(
            ptex=ptex, sha=options.pants_sha, find_links_dir=find_links_dir
        )
        assert resolve_info.sha_version is not None
        version = resolve_info.sha_version
    elif options.pants_version:
        resolve_info = determine_tag_version(
            ptex=ptex,
            pants_version=options.pants_version,
            find_links_dir=find_links_dir,
            github_api_bearer_token=options.github_api_bearer_token,
        )
        version = resolve_info.stable_version
    else:
        if pants_config:
            if not prompt_for_pants_version(options.pants_config):
                sys.exit(1)
        else:
            maybe_pants_config = prompt_for_pants_config()
            if not maybe_pants_config:
                sys.exit(1)
            pants_config = maybe_pants_config
            newly_created_build_root = pants_config.parent

        configure_version, resolve_info = determine_latest_stable_version(
            ptex=ptex,
            pants_config=pants_config,
            find_links_dir=find_links_dir,
            github_api_bearer_token=options.github_api_bearer_token,
        )
        finalizers.append(configure_version)
        version = resolve_info.stable_version

    # N.B.: These values must match the lift TOML interpreter ids.
    python = "cpython38" if version < Version("2.5") else "cpython39"

    for finalizer in finalizers:
        finalizer()

    with open(env_file, "a") as fp:
        if resolve_info.find_links:
            print(f"FIND_LINKS={resolve_info.find_links}", file=fp)
            print(f"PANTS_SHA_FIND_LINKS={resolve_info.pants_find_links_option(version)}", file=fp)
        if newly_created_build_root:
            print(f"PANTS_BUILDROOT_OVERRIDE={newly_created_build_root}", file=fp)
        print(f"PANTS_VERSION={version}", file=fp)
        print(f"PYTHON={python}", file=fp)

    sys.exit(0)


if __name__ == "__main__":
    main()
