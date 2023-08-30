# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import json
import logging
import os
import stat
import subprocess
import sys
import tempfile
from argparse import ArgumentParser
from glob import glob
from pathlib import Path
from typing import Iterable, NoReturn

from packaging.version import Version

from scie_pants.log import fatal, info, init_logging
from scie_pants.pants_version import PANTS_PEX_GITHUB_RELEASE_VERSION
from scie_pants.ptex import Ptex

log = logging.getLogger(__name__)


def venv_pip_install(venv_dir: Path, *args: str, find_links: str | None) -> None:
    subprocess.run(
        args=[
            str(venv_dir / "bin" / "python"),
            "-sE",
            "-m",
            "pip",
            # This internal 1-use pip need not nag the user about its up-to-date-ness.
            "--disable-pip-version-check",
            "--no-python-version-warning",
            "--log",
            str(venv_dir / "pants-install.log"),
            "install",
            "--quiet",
            *(("--find-links", find_links) if find_links else ()),
            *args,
        ],
        check=True,
    )


def install_pants_from_req(
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

    # Pin Pip to 22.3.1 (currently latest). The key semantic that should be preserved by the Pip
    # we use is that --find-links are used as a fallback only and PyPI is preferred. This saves us
    # money by avoiding fetching wheels from our S3 bucket at https://binaries.pantsbuild.org unless
    # absolutely needed.
    #
    # Also, we don't advance setuptools past 58 which drops support for the `setup` kwarg `use_2to3`
    # which Pants 1.x sdist dependencies (pystache) use.
    venv_pip_install(venv_dir, "-U", "pip==22.3.1", "setuptools<58", "wheel", find_links=find_links)
    venv_pip_install(venv_dir, "--progress-bar", "off", *pants_requirements, find_links=find_links)


def install_pants_from_pex(
    venv_dir: Path,
    prompt: str,
    version: Version,
    ptex: Ptex,
    extra_requirements: Iterable[str],
    find_links: str | None,
    bootstrap_urls_path: str | None,
) -> None:
    """Installs Pants into the venv using the platform-specific pre-built PEX."""
    uname = os.uname()
    pex_name = (
        f"pants.{version}-cp39-{uname.sysname.lower()}_{uname.machine.lower()}.pex"
    )

    pex_url = f"https://github.com/pantsbuild/pants/releases/download/release_{version}/{pex_name}"
    if bootstrap_urls_path:
        bootstrap_urls = json.loads(Path(bootstrap_urls_path).read_text())
        urls_info = bootstrap_urls["ptex"]
        pex_url = urls_info.get(pex_name)
        if pex_url is None:
            raise ValueError(
                f"Couldn't find '{pex_name}' in '{bootstrap_urls_path}' under the 'ptex' key."
            )

    with tempfile.NamedTemporaryFile(suffix=".pex") as pants_pex:
        try:
            ptex.fetch_to_fp(pex_url, pants_pex.file)
        except subprocess.CalledProcessError as e:
            fatal(
                f"Wasn't able to fetch the Pants PEX at {pex_url}.\n\n"
                "Check to see if the URL is reachable (i.e. GitHub isn't down) and if"
                f" {pex_name} asset exists within the release."
                " If the asset doesn't exist it may be that this platform isn't yet supported."
                " If that's the case, please reach out on Slack: https://www.pantsbuild.org/docs/getting-help#slack"
                " or file an issue on GitHub: https://github.com/pantsbuild/pants/issues/new/choose.\n\n"
                f"Exception:\n\n{e}"
            )
        subprocess.run(
            args=[
                sys.executable,
                pants_pex.name,
                "venv",
                "--prompt",
                prompt,
                "--compile",
                "--pip",
                "--collisions-ok",
                "--no-emit-warnings",  # Silence `PEXWarning: You asked for --pip ...`
                "--disable-cache",
                str(venv_dir),
            ],
            env={"PEX_TOOLS": "1"},
            check=True,
        )

    if extra_requirements:
        venv_pip_install(
            venv_dir, "--progress-bar", "off", *extra_requirements, find_links=find_links
        )


def chmod_plus_x(path: str) -> None:
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def main() -> NoReturn:
    parser = ArgumentParser()
    get_ptex = Ptex.add_options(parser)
    parser.add_argument(
        "--pants-version", type=Version, required=True, help="The Pants version to install"
    )
    parser.add_argument(
        "--find-links",
        type=str,
        help="The find links repo pointing to Pants pre-built wheels for the given Pants version",
    )
    parser.add_argument(
        "--pants-bootstrap-urls",
        type=str,
        help="The path to the JSON file containing alternate URLs for downloaded artifacts.",
    )
    parser.add_argument("--debug", type=bool, help="Install with debug capabilities.")
    parser.add_argument("--debugpy-requirement", help="The debugpy requirement to install")
    parser.add_argument("base_dir", nargs=1, help="The base directory to create Pants venvs in.")
    options = parser.parse_args()

    ptex = get_ptex(options)

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
    extra_requirements = []
    if options.debug:
        debugpy_requirement = options.debugpy_requirement or "debugpy==1.6.0"
        extra_requirements.append(debugpy_requirement)
        venv_dir = venvs_dir / f"{version}-{debugpy_requirement}"
        prompt = f"Pants {version} [{debugpy_requirement}]"
    else:
        venv_dir = venvs_dir / str(version)
        prompt = f"Pants {version}"

    info(
        f"Installing {' '.join(pants_requirements + extra_requirements)} into a virtual environment at {venv_dir}"
    )
    if version >= PANTS_PEX_GITHUB_RELEASE_VERSION:
        install_pants_from_pex(
            venv_dir=venv_dir,
            prompt=prompt,
            version=version,
            ptex=ptex,
            extra_requirements=extra_requirements,
            find_links=options.find_links,
            bootstrap_urls_path=options.pants_bootstrap_urls,
        )
    else:
        install_pants_from_req(
            venv_dir=venv_dir,
            prompt=prompt,
            pants_requirements=pants_requirements + extra_requirements,
            find_links=options.find_links,
        )

    info(f"New virtual environment successfully created at {venv_dir}.")

    pants_server_exe = str(venv_dir / "bin" / "pants")
    # Added in https://github.com/pantsbuild/pants/commit/558d843549204bbe49c351d00cdf23402da262c1
    native_client_binaries = glob(
        str(venv_dir / "lib/python*/site-packages/pants/bin/native_client")
    )
    pants_client_exe = (
        native_client_binaries[0] if len(native_client_binaries) == 1 else pants_server_exe
    )
    chmod_plus_x(pants_client_exe)

    with open(env_file, "a") as fp:
        print(f"VIRTUAL_ENV={venv_dir}", file=fp)
        print(f"PANTS_SERVER_EXE={pants_server_exe}", file=fp)
        print(f"PANTS_CLIENT_EXE={pants_client_exe}", file=fp)

    sys.exit(0)


if __name__ == "__main__":
    main()
