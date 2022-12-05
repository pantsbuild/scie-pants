# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import json
import os
import subprocess
import sys
import urllib.parse
from argparse import ArgumentParser
from pathlib import Path
from typing import NoReturn, Callable, Iterable

import tomlkit
from colors import green, red, yellow


def log(message: str) -> None:
    print(message, file=sys.stderr)


def info(message: str) -> None:
    log(green(message))


def warn(message: str) -> None:
    log(yellow(message))


def fatal(message: str) -> NoReturn:
    sys.exit(red(message))


def determine_sha_version(ptex: str, sha: str) -> tuple[str, str]:
    version_file_url = (
        f"https://raw.githubusercontent.com/pantsbuild/pants/{sha}/src/python/pants/VERSION"
    )
    result = subprocess.run(args=[ptex, version_file_url], stdout=subprocess.PIPE, check=True)
    pants_version = result.stdout.decode().strip()
    version = f"{pants_version}+git{sha[:8]}"
    find_links = (
        "https://binaries.pantsbuild.org/wheels/pantsbuild.pants/"
        f"{sha}/{urllib.parse.quote(version)}/index.html"
    )
    return find_links, version


def determine_latest_stable_version(ptex: str, pants_config: Path) -> tuple[Callable[[], None], str]:
    info(f"Fetching latest stable Pants version since none is configured")
    data_url = "https://pypi.org/pypi/pantsbuild.pants/json"
    result = subprocess.run(args=[ptex, data_url], stdout=subprocess.PIPE, check=True)
    version = json.loads(result.stdout)["info"]["version"]

    def configure_version():
        backup = None
        if pants_config.exists():
            info(f"Setting [GLOBAL] pants_version = \"{version}\" in {pants_config}")
            config = tomlkit.loads(pants_config.read_text())
            backup = f"{pants_config}.bak"
        else:
            info(f"Creating {pants_config} and configuring it to use Pants {version}")
            config = tomlkit.document()
        global_section = config.setdefault("GLOBAL", {})
        global_section["pants_version"] = version
        if backup:
            warn(f"Backing up {pants_config} to {backup}")
            pants_config.replace(backup)
        pants_config.write_text(tomlkit.dumps(config))

    return configure_version, version


def install_pants(
    venv_dir: Path,
    prompt: str,
    pants_requirements: Iterable[str],
    find_links: str | None = None,
) -> str:
    subprocess.run(
        args=[
            sys.executable,
            "-m",
            "venv",
            "--clear",
            "--prompt",
            prompt,
            str(venv_dir)
        ],
        check=True
    )
    python = venv_dir / "bin" / "python"

    if not find_links:
        null_find_links_repo = venv_dir / ".null-find-links-repo"
        null_find_links_repo.mkdir()
        find_links = str(null_find_links_repo)

    def pip_install(*args: str) -> None:
        subprocess.run(
            args=[
                str(python),
                "-sE",
                "-m",
                "pip",
                "install",
                "--quiet",
                "--find-links",
                find_links,
                *args
            ],
            check=True
        )

    # Grab the latest pip, but don't advance setuptools past 58 which drops support for the
    # `setup` kwarg `use_2to3` which Pants 1.x sdist dependencies (pystache) use.
    pip_install("-U", "pip", "setuptools<58")
    pip_install("--progress-bar", "off", *pants_requirements)

    return find_links


def main() -> NoReturn:
    parser = ArgumentParser()
    parser.add_argument("--sha", help="The Pants sha to install (trumps --version)")
    parser.add_argument("--version", help="The Pants version to install")
    parser.add_argument(
        "--ptex",
        help=(
            "The path of a ptex binary for performing lookups of the latest stable Pants version "
            "as well as lookups of PANTS_SHA information."
        )
    )
    parser.add_argument("--config", help="The path of the pants.toml file")
    parser.add_argument("--debug", type=bool, help="Install with debug capabilities.")
    parser.add_argument("--debugpy-requirement", help="The debugpy requirement to install")
    parser.add_argument("base_dir", nargs=1, help="The base directory to create Pants venvs in.")
    options = parser.parse_args()

    env_file = os.environ.get("SCIE_BINDING_ENV")
    if not env_file:
        fatal("Expected SCIE_BINDING_ENV to be set in the environment")

    finalizers = []
    find_links = None
    if options.sha:
        if not options.ptex:
            fatal("The --ptex option must be set when --sha is set.")
        find_links, version = determine_sha_version(ptex=options.ptex, sha=options.sha)
    elif options.version:
        version = options.version
    else:
        if not options.ptex:
            fatal("The --ptex option must be set when neither --sha nor --version is set.")
        if not options.config:
            fatal("The --config option must be set when neither --sha nor --version is set.")
        configure_version, version = determine_latest_stable_version(
            ptex=options.ptex, pants_config=Path(options.config)
        )
        finalizers.append(configure_version)

    python_version = ".".join(map(str, sys.version_info[:3]))
    info(f"Bootstrapping Pants {version} using {sys.implementation.name} {python_version}")

    base_dir = Path(options.base_dir[0])
    pants_requirements = [f"pantsbuild.pants=={version}"]
    if options.debug:
        debugpy_requirement = options.debugpy_requirement or "debugpy==1.6.0"
        pants_requirements.append("debugpy==1.6.0")
        venv_dir = base_dir / f"{version}-{debugpy_requirement}"
        prompt = f"Pants {version} [{debugpy_requirement}]"
    else:
        venv_dir = base_dir / version
        prompt = f"Pants {version}"

    info(f"Installing {' '.join(pants_requirements)} into a virtual environment at {venv_dir}")
    find_links = install_pants(
        venv_dir=venv_dir,
        prompt=prompt,
        pants_requirements=pants_requirements,
        find_links=find_links,
    )
    for finalizer in finalizers:
        finalizer()
    info(f"New virtual environment successfully created at {venv_dir}.")

    with open(env_file, "a") as fp:
        print(f"PANTS_VERSION={version}", file=fp)
        print(f"PANTS_PYTHON_REPOS_REPOS={find_links}", file=fp)
        print(f"VIRTUALENV={venv_dir}", file=fp)

    sys.exit(0)


if __name__ == '__main__':
    main()
