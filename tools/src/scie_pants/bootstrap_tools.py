# Copyright 2022 Pants project contributors.
# Licensed under the Apache License, Version 2.0 (see LICENSE).

import argparse
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
    def require(option: str) -> str:
        value = getattr(options, option, None)
        if not value:
            fatal(
                f"The --{option.replace('_', '-')} option is required for the bootstrap-cache-key "
                "command."
            )
        return str(value)

    cache_key = [
        f"python_distribution_hash={require('python_distribution_hash')}",
        f"pants_version={require('pants_version')}",
    ]
    print(" ".join(cache_key))


def main() -> NoReturn:
    parser = ArgumentParser(prog=PROG)
    parser.add_argument("-V", "--version", action="version", version=f"{VERSION}")
    parser.add_argument(
        "--python-distribution-hash",
        # The content hash of the Python distribution being used.
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--pants-version",
        # The version of Pants being used.
        help=argparse.SUPPRESS,
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
