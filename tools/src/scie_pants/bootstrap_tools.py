# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import os
import sys
from argparse import ArgumentParser, Namespace
from typing import Any, Callable, NoReturn

from scie_pants import INSTALL_URL, VERSION
from scie_pants.log import fatal

PROG = os.environ.get("SCIE", sys.argv[0])


def versioned(func) -> Callable:
    def wrapper(*args, **kwargs) -> Any:
        version = os.environ.get("PANTS_BOOTSTRAP_TOOLS")
        if not version:
            fatal(
                f"The {func.__name__} command requires PANTS_BOOTSTRAP_TOOLS be set in the "
                f"environment."
            )

        try:
            expected_version = int(version)
        except ValueError:
            fatal(
                f"The bootstrap tools version must be an integer, given: "
                f"PANTS_BOOTSTRAP_TOOLS={version}"
            )

        if expected_version > VERSION:
            fatal(
                f"{PROG} script (bootstrap version {VERSION} is too old for this invocation (with "
                f"PANTS_BOOTSTRAP_TOOLS={expected_version}).\n"
                f"Please update it by following {INSTALL_URL}"
            )
        func(*args, **kwargs)

    return wrapper


@versioned
def bootstrap_cache_key(options: Namespace) -> None:
    scie_base = options.scie_base
    if not scie_base:
        fatal("The --scie-base option is required for the bootstrap-cache-key command.")

    pants_venv_full_path = options.pants_venv_full_path
    if not pants_venv_full_path:
        fatal("The --pants-venv-full-path option is required for the bootstrap-cache-key command.")

    if scie_base != os.path.commonpath(
        (
            scie_base,
            pants_venv_full_path,
        )
    ):
        fatal(
            "The given --pants-venv-full-path is not a subdirectory of --pants-venv-base-path.\n"
            "Given:\n"
            f"--scie-base={scie_base}\n"
            f"--pants-venv-full-path={pants_venv_full_path}"
        )

    # The Pants venvs the installer creates mix the following into their path:
    # + The content hash of the python distribution used to install and run Pants, which includes
    #   the Python version, operating system and chip architecture implicitly by definition.
    # + The Pants version.
    # + The debug mode, which includes the debugpy version.
    #
    # As such, this satisfies the criteria for a bootstrap cache key that will be invalidated at
    # least as often as needed. We relativize away the SCIE_BASE since install location should not
    # contribute to the cache key, just install contents.
    print(
        os.path.relpath(
            pants_venv_full_path,
            scie_base,
        )
    )


def main() -> NoReturn:
    parser = ArgumentParser(prog=PROG)
    parser.add_argument(
        "-V",
        "--version",
        action="version",
        version=f"{VERSION}",
    )
    parser.add_argument(
        "--scie-base",
        help="The absolute path of the SCIE_BASE installs are inside.",
    )
    parser.add_argument(
        "--pants-venv-full-path",
        help="The absolute path of the active Pants configuration's venv",
    )

    sub_commands = parser.add_subparsers()
    cache_key_parser = sub_commands.add_parser(
        "bootstrap-cache-key",
        help=(
            "Print an opaque value that can be used as a key for accurate and safe caching of the "
            "pants bootstrap directories. (Added in bootstrap version 1.)"
        ),
    )
    cache_key_parser.set_defaults(func=bootstrap_cache_key)

    version_parser = sub_commands.add_parser(
        "bootstrap-version",
        help=(
            "Print a version number for the bootstrap script itself. Distributed scripts (such as "
            "reusable CI formulae) that use these bootstrap tools should set PANTS_BOOTSTRAP_TOOLS "
            "to the minimum script version for the features they require. For example, if "
            "'some-tool' was added in version 123: "
            "PANTS_BOOTSTRAP_TOOLS=123 ./pants some-tool (Added in bootstrap version 1.)"
        ),
    )
    version_parser.set_defaults(func=lambda _: print(VERSION))

    help_parser = sub_commands.add_parser(
        "help",
        help="Show this help.",
    )
    help_parser.set_defaults(func=lambda _: parser.print_help())
    parser.set_defaults(func=lambda _: parser.print_help())

    options = parser.parse_args()
    subcommand = options.func
    subcommand(options)

    sys.exit(0)


if __name__ == "__main__":
    main()
