# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

from __future__ import annotations

import json
import os
import subprocess
import sys
import urllib.parse
from argparse import ArgumentParser
from dataclasses import dataclass
from pathlib import Path
from subprocess import CompletedProcess
from typing import Any, BinaryIO, Callable, Iterable, Iterator, NoReturn
from xml.etree import ElementTree

import tomlkit
from packaging.specifiers import SpecifierSet
from packaging.version import Version

from scie_pants.log import fatal, info, warn


@dataclass(frozen=True)
class Ptex:
    @classmethod
    def from_exe(cls, exe: str) -> Ptex:
        return cls(exe)

    _exe: str

    def _fetch(self, url: str, stdout: int, **headers: str) -> CompletedProcess:
        args = [self._exe]
        for header, value in headers.items():
            args.extend(("-H", f"{header}: {value}"))
        args.append(url)
        return subprocess.run(args=args, stdout=stdout, check=True)

    def fetch_json(self, url: str, **headers: str) -> dict[str, Any]:
        return json.loads(self._fetch(url, stdout=subprocess.PIPE, **headers).stdout)

    def fetch_text(self, url: str, **headers: str) -> str:
        return self._fetch(url, stdout=subprocess.PIPE, **headers).stdout.decode()

    def fetch_to_fp(self, url: str, fp: BinaryIO, **headers: str) -> None:
        self._fetch(url, stdout=fp.fileno(), **headers)


@dataclass(frozen=True)
class FindLinksRepo:
    url: str

    def iter_pip_options(self) -> Iterator[str]:
        yield "--find-links"
        yield self.url


@dataclass(frozen=True)
class ResolveInfo:
    stable_version: Version
    sha_version: Version
    find_links: FindLinksRepo

    def iter_pip_find_links_options(self) -> Iterator[str]:
        yield from self.find_links.iter_pip_options()

    def pants_find_links_option(self) -> str:
        if self.stable_version in SpecifierSet("<2.14.0", prereleases=True):
            return f"--python-repos-repos={self.find_links.url}"
        return f"--python-repos-find-links={self.find_links.url}"


def determine_find_links(
    ptex: Ptex,
    pants_version: str,
    sha: str,
    find_links_dir: Path,
    include_pants_distributions_in_findlinks: bool,
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
        if include_pants_distributions_in_findlinks:
            pantsbuild_pants_find_links = (
                "https://binaries.pantsbuild.org/wheels/pantsbuild.pants/"
                f"{sha}/{urllib.parse.quote(str(sha_version))}/index.html"
            )
            ptex.fetch_to_fp(pantsbuild_pants_find_links, fp)

    return ResolveInfo(
        stable_version=Version(pants_version),
        sha_version=sha_version,
        find_links=FindLinksRepo(f"file://{find_links_file}"),
    )


def determine_tag_version(
    ptex: Ptex, pants_version: str, find_links_dir: Path, github_api_bearer_token: str | None = None
) -> ResolveInfo:
    github_api_url = (
        "https://api.github.com/repos/pantsbuild/pants/git/refs/tags/"
        f"release_{urllib.parse.quote(pants_version)}"
    )
    headers = (
        {"Authorization": f"Bearer {github_api_bearer_token}"} if github_api_bearer_token else {}
    )
    github_api_tag_url = ptex.fetch_json(github_api_url, **headers)["object"]["url"]
    sha = ptex.fetch_json(github_api_tag_url, **headers)["object"]["sha"]
    return determine_find_links(
        ptex, pants_version, sha, find_links_dir, include_pants_distributions_in_findlinks=False
    )


def determine_sha_version(ptex: Ptex, sha: str, find_links_dir: Path) -> ResolveInfo:
    version_file_url = (
        f"https://raw.githubusercontent.com/pantsbuild/pants/{sha}/src/python/pants/VERSION"
    )
    pants_version = ptex.fetch_text(version_file_url).strip()
    return determine_find_links(
        ptex, pants_version, sha, find_links_dir, include_pants_distributions_in_findlinks=True
    )


def determine_latest_stable_version(
    ptex: Ptex, pants_config: Path, find_links_dir: Path, github_api_bearer_token: str | None = None
) -> tuple[Callable[[], None], ResolveInfo]:
    info(f"Fetching latest stable Pants version since none is configured")
    pants_version = ptex.fetch_json("https://pypi.org/pypi/pantsbuild.pants/json")["info"][
        "version"
    ]

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


def install_pants(
    resolve_info: ResolveInfo, venv_dir: Path, prompt: str, pants_requirements: Iterable[str]
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

    def pip_install(*args: str) -> None:
        subprocess.run(
            args=[
                str(python),
                "-sE",
                "-m",
                "pip",
                "install",
                "--quiet",
                *resolve_info.iter_pip_find_links_options(),
                *args,
            ],
            check=True,
        )

    # Grab the latest pip, but don't advance setuptools past 58 which drops support for the
    # `setup` kwarg `use_2to3` which Pants 1.x sdist dependencies (pystache) use.
    pip_install("-U", "pip", "setuptools<58", "wheel")
    pip_install("--progress-bar", "off", *pants_requirements)


def main() -> NoReturn:
    parser = ArgumentParser()
    parser.add_argument("--pants-sha", help="The Pants sha to install (trumps --version)")
    parser.add_argument("--pants-version", type=str, help="The Pants version to install")
    parser.add_argument(
        "--ptex-path",
        dest="ptex",
        required=True,
        type=Ptex.from_exe,
        help=(
            "The path of a ptex binary for performing lookups of the latest stable Pants version "
            "as well as lookups of PANTS_SHA information."
        ),
    )
    parser.add_argument("--pants-config", help="The path of the pants.toml file")
    parser.add_argument(
        "--github-api-bearer-token", help="The GITHUB_TOKEN to use if running in CI context."
    )
    parser.add_argument("--debug", type=bool, help="Install with debug capabilities.")
    parser.add_argument("--debugpy-requirement", help="The debugpy requirement to install")
    parser.add_argument("base_dir", nargs=1, help="The base directory to create Pants venvs in.")
    options = parser.parse_args()

    env_file = os.environ.get("SCIE_BINDING_ENV")
    if not env_file:
        fatal("Expected SCIE_BINDING_ENV to be set in the environment")

    base_dir = Path(options.base_dir[0])
    venvs_dir = base_dir / "venvs"
    find_links_dir = base_dir / "find_links"

    finalizers = []
    if options.pants_sha:
        resolve_info = determine_sha_version(
            ptex=options.ptex, sha=options.pants_sha, find_links_dir=find_links_dir
        )
        version = resolve_info.sha_version
    elif options.pants_version:
        resolve_info = determine_tag_version(
            ptex=options.ptex,
            pants_version=options.pants_version,
            find_links_dir=find_links_dir,
            github_api_bearer_token=options.github_api_bearer_token,
        )
        version = resolve_info.stable_version
    else:
        if not options.pants_config:
            fatal(
                "The --pants-config option must be set when neither --pants-sha nor "
                "--pants-version is set."
            )
        configure_version, resolve_info = determine_latest_stable_version(
            ptex=options.ptex,
            pants_config=Path(options.pants_config),
            find_links_dir=find_links_dir,
            github_api_bearer_token=options.github_api_bearer_token,
        )
        finalizers.append(configure_version)
        version = resolve_info.stable_version

    python_version = ".".join(map(str, sys.version_info[:3]))
    info(f"Bootstrapping Pants {version} using {sys.implementation.name} {python_version}")

    pants_requirements = [f"pantsbuild.pants=={version}"]
    if options.debug:
        debugpy_requirement = options.debugpy_requirement or "debugpy==1.6.0"
        pants_requirements.append("debugpy==1.6.0")
        venv_dir = venvs_dir / f"{version}-{debugpy_requirement}"
        prompt = f"Pants {version} [{debugpy_requirement}]"
    else:
        venv_dir = venvs_dir / str(version)
        prompt = f"Pants {version}"

    info(f"Installing {' '.join(pants_requirements)} into a virtual environment at {venv_dir}")
    install_pants(
        resolve_info=resolve_info,
        venv_dir=venv_dir,
        prompt=prompt,
        pants_requirements=pants_requirements,
    )
    for finalizer in finalizers:
        finalizer()

    info(f"New virtual environment successfully created at {venv_dir}.")

    with open(env_file, "a") as fp:
        print(f"PANTS_VERSION={version}", file=fp)
        print(f"PANTS_SHA_FIND_LINKS={resolve_info.pants_find_links_option()}", file=fp)
        print(f"VIRTUAL_ENV={venv_dir}", file=fp)

    sys.exit(0)


if __name__ == "__main__":
    main()
