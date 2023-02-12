# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import logging
import os
import subprocess
import sys
from argparse import ArgumentParser
from pathlib import Path
from typing import Iterable, NoReturn

from packaging.version import Version

from scie_pants.log import fatal, info, init_logging

log = logging.getLogger(__name__)


def install_pants(
    venv_dir: Path, prompt: str, pants_requirements: Iterable[str], find_links: str | None
) -> None:
    subprocess.run(
        args=[
            sys.executable,
            "-m",
            "venv",
            "--clear",
            "--prompt",
            prompt,
            str(venv_dir),
        ],
        check=True,
    )
    python = venv_dir / "bin" / "python"
    install_log = venv_dir / "pants-install.log"

    find_links_options = ("--find-links", find_links) if find_links else ()

    def pip_install(*args: str) -> None:
        subprocess.run(
            args=[
                str(python),
                "-sE",
                "-m",
                "pip",
                # This internal 1-use pip need not nag the user about its up-to-date-ness.
                "--disable-pip-version-check",
                "--no-python-version-warning",
                "--log",
                install_log,
                "install",
                "--quiet",
                *find_links_options,
                *args,
            ],
            check=True,
        )

    # Pin Pip to 22.3.1 (currently latest). The key semantic that should be preserved by the Pip
    # we use is that --find-links are used as a fallback only and PyPI is preferred. This saves us
    # money by avoiding fetching wheels from our S3 bucket at https://binaries.pantsbuild.org unless
    # absolutely needed.
    #
    # Also, we don't advance setuptools past 58 which drops support for the `setup` kwarg `use_2to3`
    # which Pants 1.x sdist dependencies (pystache) use.
    pip_install("-U", "pip==22.3.1", "setuptools<58", "wheel")
    pip_install("--progress-bar", "off", *pants_requirements)


def main() -> NoReturn:
    parser = ArgumentParser()
    parser.add_argument(
        "--pants-version", type=Version, required=True, help="The Pants version to install"
    )
    parser.add_argument(
        "--find-links",
        type=str,
        help="The find links repo pointing to Pants pre-built wheels for the given Pants version",
    )
    parser.add_argument("--debug", type=bool, help="Install with debug capabilities.")
    parser.add_argument("--debugpy-requirement", help="The debugpy requirement to install")
    parser.add_argument("base_dir", nargs=1, help="The base directory to create Pants venvs in.")
    options = parser.parse_args()

    base_dir = Path(options.base_dir[0])
    init_logging(base_dir=base_dir, log_name="install")

    env_file = os.environ.get("SCIE_BINDING_ENV")
    if not env_file:
        fatal("Expected SCIE_BINDING_ENV to be set in the environment")

    venvs_dir = base_dir / "venvs"

    version = options.pants_version
    python_version = ".".join(map(str, sys.version_info[:3]))
    info(f"Bootstrapping Pants {version} using {sys.implementation.name} {python_version}")

    pants_requirements = [f"pantsbuild.pants=={version}"]
    if options.debug:
        debugpy_requirement = options.debugpy_requirement or "debugpy==1.6.0"
        pants_requirements.append(debugpy_requirement)
        venv_dir = venvs_dir / f"{version}-{debugpy_requirement}"
        prompt = f"Pants {version} [{debugpy_requirement}]"
    else:
        venv_dir = venvs_dir / str(version)
        prompt = f"Pants {version}"

    info(f"Installing {' '.join(pants_requirements)} into a virtual environment at {venv_dir}")
    install_pants(
        venv_dir=venv_dir,
        prompt=prompt,
        pants_requirements=pants_requirements,
        find_links=options.find_links,
    )
    info(f"New virtual environment successfully created at {venv_dir}.")

    with open(env_file, "a") as fp:
        print(f"VIRTUAL_ENV={venv_dir}", file=fp)

    sys.exit(0)


if __name__ == "__main__":
    main()
